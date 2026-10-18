use crate::ast::{Attribute, Document, Element, Namespace, NodeId, NodeKind};
use std::borrow::Cow;
use thiserror::Error;

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

    for token in tokenizer {
        let token = token.map_err(|e| ParseError::Xml {
            position: e.pos().col as usize,
            message: e.to_string(),
        })?;

        match token {
            xmlparser::Token::Declaration { .. } => {
                // XML declaration like <?xml version="1.0"?> — we drop it for minification
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

            xmlparser::Token::DtdStart { .. }
            | xmlparser::Token::DtdEnd { .. }
            | xmlparser::Token::EmptyDtd { .. }
            | xmlparser::Token::EntityDeclaration { .. } => {
                // Capture DOCTYPE as a doctype node.
                // xmlparser splits DTD into multiple tokens; we capture the raw text.
                // For simplicity, store the DTD range as a doctype node on first DTD token.
                let parent = *stack.last().ok_or(ParseError::UnexpectedEof)?;
                // Only add on DtdStart or EmptyDtd to avoid duplicates.
                if matches!(
                    token,
                    xmlparser::Token::DtdStart { .. } | xmlparser::Token::EmptyDtd { .. }
                ) {
                    let id = doc.alloc(NodeKind::Doctype(String::new()));
                    doc.append_child(parent, id);
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
                let value_str = decode_xml_entities(value.as_str()).into_owned();

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
                    let decoded = decode_xml_entities(text_str);
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

/// Decode the 5 standard XML entities. Returns a borrowed Cow if no entities present.
fn decode_xml_entities(input: &str) -> Cow<'_, str> {
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
                    // Unknown entity — preserve as-is
                    result.push('&');
                    result.push_str(&entity);
                    result.push(';');
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
