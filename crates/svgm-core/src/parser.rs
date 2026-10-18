use crate::ast::{Attribute, Document, Element, Namespace, NodeId, NodeKind};
use std::borrow::Cow;
use std::collections::HashMap;
use thiserror::Error;

/// Deepest chain of entity references expanded through other entities
/// (`&a;` -> `&b;` -> ...). Deeper chains keep the reference text literally.
const MAX_ENTITY_DEPTH: usize = 8;

/// Total bytes a document's internal-subset entities may expand to, across
/// all attribute values and text nodes. Once spent, remaining references are
/// kept literally (guards against "billion laughs" inputs).
const MAX_ENTITY_EXPANSION: usize = 1 << 20;

/// Entities declared in the DOCTYPE internal subset, plus the expansion
/// budget they share for the whole document.
struct Entities {
    values: HashMap<String, String>,
    budget: usize,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("XML parse error at position {position}: {message}")]
    Xml { position: usize, message: String },
    #[error("unexpected end of input")]
    UnexpectedEof,
    #[error("mismatched closing tag: expected </{expected}>, found </{found}>")]
    MismatchedTag { expected: String, found: String },
    #[error("unclosed tag: <{name}> has no closing tag")]
    UnclosedTag { name: String },
    #[error("document has no root element")]
    NoRootElement,
}

/// Parse an SVG/XML string into a Document.
pub fn parse(input: &str) -> Result<Document, ParseError> {
    // A UTF-8 byte-order mark is not part of the document.
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut doc = Document::new();
    let mut stack: Vec<NodeId> = vec![doc.root];

    let tokenizer = xmlparser::Tokenizer::from(input);

    // Collect attributes and namespaces for the current element being opened.
    let mut pending_attrs: Vec<Attribute> = Vec::new();
    let mut pending_ns: Vec<Namespace> = Vec::new();
    let mut pending_name: Option<(Option<String>, String)> = None;

    // Entities from the DOCTYPE internal subset. The DTD always precedes the
    // root element, so the map is complete before any attribute or text is
    // decoded.
    let mut entities = Entities {
        values: HashMap::new(),
        // Scale with the input so large valid documents never run out, while
        // tiny billion-laughs inputs stay capped at MAX_ENTITY_EXPANSION.
        budget: MAX_ENTITY_EXPANSION.max(input.len().saturating_mul(16)),
    };
    // Byte offset where an open `<!DOCTYPE ... [` started, so the raw
    // declaration can be stored on the Doctype node at `]>`.
    let mut dtd_start: Option<(NodeId, usize)> = None;

    for token in tokenizer {
        let token = token.map_err(|e| ParseError::Xml {
            position: e.pos().col as usize,
            message: e.to_string(),
        })?;

        match token {
            xmlparser::Token::Declaration { span, .. } => {
                // XML declaration `<?xml version="1.0"?>`, kept as a
                // processing instruction with target `xml` so that
                // removeProcInst owns its removal (and disabling that pass
                // round-trips it).
                let content = span
                    .as_str()
                    .strip_prefix("<?xml")
                    .and_then(|s| s.strip_suffix("?>"))
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let id = doc.alloc(NodeKind::ProcessingInstruction {
                    target: "xml".to_string(),
                    content,
                });
                let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                doc.append_child(parent, id);
            }

            xmlparser::Token::ProcessingInstruction {
                target, content, ..
            } => {
                let target_str = target.as_str().to_string();
                // Skip the xml declaration PI
                if target_str == "xml" {
                    continue;
                }
                let content_str = content.map(|c| c.as_str().to_string()).unwrap_or_default();
                let id = doc.alloc(NodeKind::ProcessingInstruction {
                    target: target_str,
                    content: content_str,
                });
                let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                doc.append_child(parent, id);
            }

            xmlparser::Token::Comment { text, .. } => {
                let id = doc.alloc(NodeKind::Comment(text.as_str().to_string()));
                let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                doc.append_child(parent, id);
            }

            xmlparser::Token::EmptyDtd { span, .. } => {
                // `<!DOCTYPE svg PUBLIC "..." "...">` without an internal subset.
                let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                let id = doc.alloc(NodeKind::Doctype(doctype_body(span.as_str())));
                doc.append_child(parent, id);
            }

            xmlparser::Token::DtdStart { span, .. } => {
                // `<!DOCTYPE svg [` — the raw text is filled in at `]>` so
                // that a disabled removeDoctype pass round-trips it.
                let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                let id = doc.alloc(NodeKind::Doctype(String::new()));
                doc.append_child(parent, id);
                dtd_start = Some((id, span.start()));
            }

            xmlparser::Token::DtdEnd { span } => {
                if let Some((id, start)) = dtd_start.take()
                    && let Some(raw) = input.get(start..span.end())
                    && let NodeKind::Doctype(ref mut text) = doc.node_mut(id).kind
                {
                    *text = doctype_body(raw);
                }
            }

            xmlparser::Token::EntityDeclaration {
                name,
                definition,
                span,
            } => {
                // `<!ENTITY ns_svg "http://www.w3.org/2000/svg">` (SVGO
                // lib/parser.js: sax.ENTITIES). External entities
                // (`SYSTEM`/`PUBLIC`) cannot be resolved; parameter entities
                // (`<!ENTITY % x ...>`) are not referenceable from content.
                // The first declaration of a name wins, per the XML spec.
                let is_parameter_entity = span
                    .as_str()
                    .strip_prefix("<!ENTITY")
                    .is_some_and(|rest| rest.trim_start().starts_with('%'));
                if let xmlparser::EntityDefinition::EntityValue(value) = definition
                    && !is_parameter_entity
                {
                    entities
                        .values
                        .entry(name.as_str().to_string())
                        .or_insert_with(|| value.as_str().to_string());
                }
            }

            xmlparser::Token::ElementStart { prefix, local, .. } => {
                pending_attrs.clear();
                pending_ns.clear();
                let prefix_str = if prefix.is_empty() {
                    None
                } else {
                    Some(prefix.as_str().to_string())
                };
                pending_name = Some((prefix_str, local.as_str().to_string()));
            }

            xmlparser::Token::Attribute {
                prefix,
                local,
                value,
                ..
            } => {
                let prefix_str = prefix.as_str();
                let local_str = local.as_str();
                let value_str = decode_xml_entities(value.as_str(), &mut entities).into_owned();

                if prefix_str == "xmlns" {
                    // Namespace declaration: xmlns:prefix="uri"
                    pending_ns.push(Namespace {
                        prefix: local_str.to_string(),
                        uri: value_str,
                    });
                } else if prefix_str.is_empty() && local_str == "xmlns" {
                    // Default namespace: xmlns="uri"
                    pending_ns.push(Namespace {
                        prefix: String::new(),
                        uri: value_str,
                    });
                } else {
                    let attr_prefix = if prefix_str.is_empty() {
                        None
                    } else {
                        Some(prefix_str.to_string())
                    };
                    pending_attrs.push(Attribute {
                        prefix: attr_prefix,
                        name: local_str.to_string(),
                        value: value_str,
                    });
                }
            }

            xmlparser::Token::ElementEnd { end, .. } => {
                match end {
                    xmlparser::ElementEnd::Open => {
                        // <tag ...> — push element onto stack
                        let (prefix, name) =
                            pending_name.take().ok_or(ParseError::UnexpectedEof)?;
                        let elem = Element {
                            name,
                            prefix,
                            attributes: std::mem::take(&mut pending_attrs),
                            namespaces: std::mem::take(&mut pending_ns),
                        };
                        let id = doc.alloc(NodeKind::Element(elem));
                        let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                        doc.append_child(parent, id);
                        stack.push(id);
                    }
                    xmlparser::ElementEnd::Close(prefix, local) => {
                        // </tag> — pop from stack
                        let current = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                        if let NodeKind::Element(ref elem) = doc.node(current).kind {
                            let expected_name = elem.qualified_name();
                            let close_prefix = prefix.as_str();
                            let close_name = if close_prefix.is_empty() {
                                local.as_str().to_string()
                            } else {
                                format!("{}:{}", close_prefix, local.as_str())
                            };
                            if expected_name != close_name {
                                return Err(ParseError::MismatchedTag {
                                    expected: expected_name,
                                    found: close_name,
                                });
                            }
                        }
                        stack.pop();
                    }
                    xmlparser::ElementEnd::Empty => {
                        // <tag .../> — create element but don't push onto stack
                        let (prefix, name) =
                            pending_name.take().ok_or(ParseError::UnexpectedEof)?;
                        let elem = Element {
                            name,
                            prefix,
                            attributes: std::mem::take(&mut pending_attrs),
                            namespaces: std::mem::take(&mut pending_ns),
                        };
                        let id = doc.alloc(NodeKind::Element(elem));
                        let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                        doc.append_child(parent, id);
                    }
                }
            }

            xmlparser::Token::Text { text } => {
                let text_str = text.as_str();
                if !text_str.is_empty() {
                    let decoded = decode_xml_entities(text_str, &mut entities);
                    let id = doc.alloc(NodeKind::Text(decoded.into_owned()));
                    let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                    doc.append_child(parent, id);
                }
            }

            xmlparser::Token::Cdata { text, .. } => {
                let id = doc.alloc(NodeKind::CData(text.as_str().to_string()));
                let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                doc.append_child(parent, id);
            }
        }
    }

    // xmlparser does not report EOF inside an unfinished document, so check
    // the final state ourselves (issue #24: `<svg` used to become an empty
    // document and serialize to "").
    if pending_name.is_some() {
        // `<svg` / `<rect width="1"` — start tag never finished
        return Err(ParseError::UnexpectedEof);
    }
    if stack.len() > 1 {
        // `<svg>` / `<svg><g>` — elements never closed
        let name = match doc.node(*stack.last().unwrap()).kind {
            NodeKind::Element(ref elem) => elem.qualified_name(),
            _ => String::new(),
        };
        return Err(ParseError::UnclosedTag { name });
    }
    let has_root_element = doc
        .children(doc.root)
        .any(|id| matches!(doc.node(id).kind, NodeKind::Element(_)));
    if !has_root_element {
        // empty / whitespace-only / declaration-only / comment-only input
        return Err(ParseError::NoRootElement);
    }

    Ok(doc)
}

/// `<!DOCTYPE svg PUBLIC "..." [...]>` -> `svg PUBLIC "..." [...]`, the form
/// the serializer wraps back into `<!DOCTYPE ...>`.
fn doctype_body(raw: &str) -> String {
    raw.strip_prefix("<!DOCTYPE")
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(raw)
        .trim()
        .to_string()
}

/// Decode the 5 standard XML entities, numeric character references and
/// entities declared in the DOCTYPE internal subset. Returns a borrowed Cow
/// if no entities are present.
fn decode_xml_entities<'a>(input: &'a str, entities: &mut Entities) -> Cow<'a, str> {
    decode_xml_entities_at(input, entities, 0)
}

fn decode_xml_entities_at<'a>(
    input: &'a str,
    entities: &mut Entities,
    depth: usize,
) -> Cow<'a, str> {
    if !input.contains('&') {
        return Cow::Borrowed(input);
    }
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            // Scan a bounded reference name; a bare `&` with no terminating
            // `;` (or with whitespace/markup before it) is literal text and
            // must not swallow the rest of the string or grow a `;`.
            let mut entity = String::new();
            let mut terminated = false;
            while let Some(&c) = chars.peek() {
                if c == ';' {
                    chars.next();
                    terminated = true;
                    break;
                }
                if entity.len() >= 32
                    || !(c.is_ascii_alphanumeric()
                        || c == '#'
                        || c == '_'
                        || c == '-'
                        || c == '.'
                        || c == ':')
                {
                    break;
                }
                entity.push(c);
                chars.next();
            }
            if !terminated {
                result.push('&');
                result.push_str(&entity);
                continue;
            }
            match entity.as_str() {
                "amp" => result.push('&'),
                "lt" => result.push('<'),
                "gt" => result.push('>'),
                "quot" => result.push('"'),
                "apos" => result.push('\''),
                s if s.starts_with('#') => {
                    // Numeric character reference: &#123; or &#x1a;
                    let num = &s[1..];
                    let code = if let Some(hex) = num.strip_prefix('x') {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        num.parse::<u32>().ok()
                    };
                    if let Some(c) = code.and_then(char::from_u32).filter(|&c| is_xml_char(c)) {
                        result.push(c);
                    } else {
                        // Unrecognized — preserve as-is
                        result.push('&');
                        result.push_str(s);
                        result.push(';');
                    }
                }
                _ => {
                    // Entity from the DOCTYPE internal subset: substitute
                    // its (recursively decoded) value while the depth and
                    // document-wide expansion budget allow; otherwise, or
                    // for an unknown entity, preserve the reference as-is.
                    //
                    // The raw value (plus one byte) is charged up front, so
                    // every attempt consumes budget even when the nested
                    // decode ends up keeping its references literal; the
                    // output is therefore bounded by the budget and so is
                    // the number of decode calls.
                    let mut expansion = None;
                    // Check depth and budget *before* copying the value, and
                    // charge every attempt (even a refused or unknown one) so
                    // lookup work stays bounded too.
                    let value = match entities.values.get(&entity) {
                        Some(v) if depth < MAX_ENTITY_DEPTH && v.len() < entities.budget => {
                            Some(v.clone())
                        }
                        _ => None,
                    };
                    if let Some(value) = value {
                        entities.budget -= value.len() + 1;
                        expansion =
                            Some(decode_xml_entities_at(&value, entities, depth + 1).into_owned());
                    } else {
                        entities.budget = entities.budget.saturating_sub(1);
                    }
                    match expansion {
                        Some(text) => result.push_str(&text),
                        None => {
                            result.push('&');
                            result.push_str(&entity);
                            result.push(';');
                        }
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }
    Cow::Owned(result)
}

/// XML 1.0 `Char` production: control characters other than tab/LF/CR, the
/// surrogate range (already excluded by `char`) and U+FFFE/U+FFFF are not
/// allowed, so a reference to them is kept as literal text.
fn is_xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_svg() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="50" height="50"/></svg>"#;
        let doc = parse(svg).unwrap();

        // Root has one child: the <svg> element
        assert_eq!(doc.children(doc.root).count(), 1);

        let svg_id = doc.children(doc.root).next().unwrap();
        if let NodeKind::Element(ref elem) = doc.node(svg_id).kind {
            assert_eq!(elem.name, "svg");
            assert_eq!(elem.attr("width"), Some("100"));
            assert_eq!(elem.attr("height"), Some("100"));
            assert_eq!(elem.namespaces.len(), 1);
            assert_eq!(elem.namespaces[0].uri, "http://www.w3.org/2000/svg");
        } else {
            panic!("expected element");
        }

        // <svg> has one child: <rect>
        assert_eq!(doc.children(svg_id).count(), 1);
        let rect_id = doc.children(svg_id).next().unwrap();
        if let NodeKind::Element(ref elem) = doc.node(rect_id).kind {
            assert_eq!(elem.name, "rect");
            assert_eq!(elem.attr("width"), Some("50"));
        } else {
            panic!("expected element");
        }
    }

    #[test]
    fn parse_with_comments_and_text() {
        let svg =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- a comment --><text>Hello</text></svg>"#;
        let doc = parse(svg).unwrap();
        let svg_id = doc.children(doc.root).next().unwrap();
        let children: Vec<_> = doc.children(svg_id).collect();
        assert_eq!(children.len(), 2);

        assert!(matches!(doc.node(children[0]).kind, NodeKind::Comment(_)));
        assert!(matches!(doc.node(children[1]).kind, NodeKind::Element(_)));
    }

    #[test]
    fn parse_nested_groups() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><g><rect/></g></g></svg>"#;
        let doc = parse(svg).unwrap();
        let all = doc.traverse();
        // root, svg, g, g, rect
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn parse_namespaced_attributes() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><use xlink:href=\"#icon\"/></svg>";
        let doc = parse(svg).unwrap();
        let svg_id = doc.children(doc.root).next().unwrap();
        if let NodeKind::Element(ref elem) = doc.node(svg_id).kind {
            assert!(elem.namespaces.iter().any(|ns| ns.prefix == "xlink"));
        }
        let use_id = doc.children(svg_id).next().unwrap();
        if let NodeKind::Element(ref elem) = doc.node(use_id).kind {
            assert_eq!(elem.name, "use");
            assert!(
                elem.attributes
                    .iter()
                    .any(|a| a.prefix.as_deref() == Some("xlink") && a.name == "href")
            );
        }
    }

    #[test]
    fn issue_24_truncated_and_rootless_inputs_error() {
        // `<svg` used to parse to an empty document and serialize to "".
        for input in [
            "<svg",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"",
            "<rect width=\"1\"",
        ] {
            assert!(
                matches!(
                    parse(input),
                    Err(ParseError::UnexpectedEof) | Err(ParseError::Xml { .. })
                ),
                "{input:?}"
            );
        }
        for input in ["<svg>", "<svg><g>", "<svg><rect/>", "<svg><g></g>"] {
            assert!(
                matches!(parse(input), Err(ParseError::UnclosedTag { .. })),
                "{input:?}"
            );
        }
        for input in [
            "",
            "   \n",
            "<?xml version=\"1.0\"?>",
            "<!-- only a comment -->",
            "<?xml version=\"1.0\"?>\n<!-- c -->\n",
        ] {
            assert!(
                matches!(
                    parse(input),
                    Err(ParseError::NoRootElement) | Err(ParseError::Xml { .. })
                ),
                "{input:?}"
            );
        }
        // Non-svg roots are still accepted (SVGO processes any XML).
        assert!(parse("<html><svg/></html>").is_ok());
        // A UTF-8 BOM does not break parsing.
        assert!(parse("\u{feff}<svg xmlns=\"http://www.w3.org/2000/svg\"/>").is_ok());
    }

    #[test]
    fn doctype_internal_subset_entities_are_expanded() {
        use crate::serializer::serialize;
        // Classic Adobe Illustrator export: the SVG namespace itself is an
        // entity reference, so without expansion nothing renders.
        let input = "<?xml version=\"1.0\"?>\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\" [\n <!ENTITY ns_svg \"http://www.w3.org/2000/svg\">\n <!ENTITY brand \"#ff0000\">\n <!ENTITY brand \"ignored-duplicate\">\n <!ENTITY nested \"&brand;&amp;&#65;\">\n <!ENTITY ext SYSTEM \"ext.xml\">\n <!ENTITY % param \"x\">\n]>\n<svg xmlns=\"&ns_svg;\" width=\"10\"><rect fill=\"&brand;\"/><text x=\"1\">&brand; &nested; &unknown; &ext; &param;</text></svg>";
        let doc = parse(input).unwrap();
        let svg_id = doc
            .children(doc.root)
            .find(|&id| matches!(doc.node(id).kind, NodeKind::Element(_)))
            .unwrap();
        let NodeKind::Element(ref svg) = doc.node(svg_id).kind else {
            panic!("expected element");
        };
        assert_eq!(svg.namespaces[0].uri, "http://www.w3.org/2000/svg");
        let children: Vec<_> = doc.children(svg_id).collect();
        let NodeKind::Element(ref rect) = doc.node(children[0]).kind else {
            panic!("expected rect");
        };
        assert_eq!(rect.attr("fill"), Some("#ff0000"));
        let text_id = doc.children(children[1]).next().unwrap();
        let NodeKind::Text(ref text) = doc.node(text_id).kind else {
            panic!("expected text");
        };
        // Nested references inside an entity value are decoded; external,
        // parameter and unknown entities are kept as literal text.
        assert_eq!(text, "#ff0000 #ff0000&A &unknown; &ext; &param;");

        // The XML declaration and the raw DOCTYPE round-trip when
        // removeProcInst / removeDoctype are disabled.
        let out = serialize(&doc);
        assert!(
            out.starts_with(
                "<?xml version=\"1.0\"?><!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\""
            ),
            "{out}"
        );
        assert!(
            out.contains("<!ENTITY ns_svg \"http://www.w3.org/2000/svg\">"),
            "{out}"
        );
        assert!(
            out.contains("]><svg xmlns=\"http://www.w3.org/2000/svg\""),
            "{out}"
        );
        assert!(!out.contains("&amp;ns_svg;"), "{out}");

        // A DOCTYPE without an internal subset round-trips too.
        let doc = parse("<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\"><svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
        let out = serialize(&doc);
        assert!(
            out.starts_with("<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\"><svg"),
            "{out}"
        );
    }

    #[test]
    fn entity_expansion_is_bounded() {
        use crate::serializer::serialize;
        // "Billion laughs": 10^7 expansions (30 MB) without a cap, within
        // the depth limit. The byte budget keeps the output around 1 MiB and
        // the parse fast; references past the budget stay literal.
        let mut dtd = String::from("<!DOCTYPE svg [<!ENTITY lol0 \"lol\">");
        for level in 1..8 {
            let prev = format!("&lol{};", level - 1);
            dtd.push_str(&format!("<!ENTITY lol{level} \"{}\">", prev.repeat(10)));
        }
        dtd.push_str("]>");
        let input = format!(
            "{dtd}<svg xmlns=\"http://www.w3.org/2000/svg\"><text>&lol7;</text><rect title=\"&lol7;\"/></svg>"
        );
        let doc = parse(&input).unwrap();
        let out = serialize(&doc);
        assert!(out.len() < 2 * MAX_ENTITY_EXPANSION, "{}", out.len());
        assert!(out.contains("lollol"), "some expansion happened");
        assert!(
            out.contains("&amp;lol"),
            "budget exhaustion keeps references literal"
        );

        // Self-referential entities terminate.
        let doc = parse("<!DOCTYPE svg [<!ENTITY a \"&b;\"><!ENTITY b \"&a;\">]><svg xmlns=\"http://www.w3.org/2000/svg\"><text>&a;</text></svg>").unwrap();
        let out = serialize(&doc);
        assert!(
            out.contains("<text>&amp;a;</text>") || out.contains("<text>&amp;b;</text>"),
            "{out}"
        );
    }

    #[test]
    fn bare_ampersand_and_invalid_references_are_kept_literally() {
        use crate::serializer::serialize;
        let doc = parse("<svg xmlns=\"http://www.w3.org/2000/svg\"><text>a & b &lt; c &amp d</text><rect title=\"x & y\"/></svg>").unwrap();
        let out = serialize(&doc);
        assert!(out.contains("a &amp; b &lt; c &amp;amp d"), "{out}");
        assert!(out.contains("title=\"x &amp; y\""), "{out}");
        assert!(!out.contains("b;"), "a ';' must not be invented: {out}");

        // Control characters are not XML Chars: keep the reference text, and
        // make sure the output still parses (idempotence).
        let doc = parse(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>a&#0;b&#1;c&#x9;d&#65;</text></svg>",
        )
        .unwrap();
        let out = serialize(&doc);
        assert!(out.contains("a&amp;#0;b&amp;#1;c\td"), "{out}");
        assert!(out.contains("A</text>"), "{out}");
        assert!(!out.contains('\0'));
        assert!(parse(&out).is_ok());
    }
}
