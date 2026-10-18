use super::{Pass, PassResult, split_declarations, split_property};
use crate::ast::{Document, NodeKind};

pub struct CleanupAttrs;

impl Pass for CleanupAttrs {
    fn name(&self) -> &'static str {
        "cleanupAttrs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                for attr in &mut elem.attributes {
                    // Minify inline style attributes
                    if attr.name == "style" && attr.prefix.is_none() {
                        let minified = minify_inline_style(&attr.value);
                        if minified != attr.value {
                            attr.value = minified;
                            changed = true;
                        }
                    } else {
                        let cleaned = cleanup_whitespace(&attr.value);
                        if cleaned != attr.value {
                            attr.value = cleaned;
                            changed = true;
                        }
                    }
                }
                // xmlns / xmlns:* are ordinary attributes in SVGO and get the
                // same cleanup; svgm keeps them in `namespaces`. A padded SVG
                // namespace URI is not the SVG namespace.
                for ns in &mut elem.namespaces {
                    let cleaned = cleanup_whitespace(&ns.uri);
                    if cleaned != ns.uri {
                        ns.uri = cleaned;
                        changed = true;
                    }
                }
            }
        }

        if changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

/// Minify an inline style attribute: strip spaces around colons/semicolons,
/// remove trailing semicolons. Text inside quotes or `url(...)` is left
/// untouched.
fn minify_inline_style(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for decl in split_declarations(s) {
        if !result.is_empty() {
            result.push(';');
        }
        if let Some((prop, val)) = split_property(decl) {
            result.push_str(prop);
            result.push(':');
            result.push_str(val);
        } else {
            result.push_str(decl);
        }
    }
    result
}

/// Collapse runs of whitespace (spaces, tabs, newlines) into a single space,
/// and trim leading/trailing whitespace.
fn cleanup_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_ws = true; // trim leading
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                result.push(' ');
                prev_ws = true;
            }
        } else {
            result.push(ch);
            prev_ws = false;
        }
    }
    // Trim trailing space
    if result.ends_with(' ') {
        result.pop();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn collapses_whitespace_in_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect class=\"  a   b  \"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("class=\"a b\""));
    }

    #[test]
    fn handles_newlines_in_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M 0\n0 L\n10 10\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("d=\"M 0 0 L 10 10\""));
    }

    #[test]
    fn minifies_inline_style() {
        assert_eq!(
            minify_inline_style(" fill : red ; stroke:blue; "),
            "fill:red;stroke:blue"
        );
        assert_eq!(minify_inline_style(";;"), "");
    }

    #[test]
    fn inline_style_keeps_url_data_uri_intact() {
        let style = "fill:url(data:image/svg+xml;base64,PHN2Zy8+);stroke:red";
        assert_eq!(minify_inline_style(style), style);
        let input =
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\"><rect style=\"{style}\"/></svg>");
        let mut doc = parse(&input).unwrap();
        assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Unchanged);
        assert!(serialize(&doc).contains(style));
    }

    #[test]
    fn inline_style_keeps_quoted_strings_intact() {
        let style = "font-family:\"a; b\"";
        assert_eq!(minify_inline_style(style), style);
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><text style='font-family:\"a; b\"'/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Unchanged);
        assert!(serialize(&doc).contains("style=\"font-family:&quot;a; b&quot;\""));
    }

    #[test]
    fn normalizes_whitespace_in_namespace_declarations() {
        // Port of SVGO cleanupAttrs.01 (pass only: the text node is left
        // alone, as in the fixture).
        let input = "<svg xmlns=\"  http://www.w3.org/2000/svg\n  \" attr=\"a      b\" attr2=\"a\nb\">\n    test\n</svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Changed);
        assert_eq!(
            serialize(&doc),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" attr=\"a b\" attr2=\"a b\">\n    test\n</svg>"
        );
    }

    #[test]
    fn normalizes_prefixed_namespace_declarations() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\" http://www.w3.org/1999/xlink \"><use xlink:href=\"#a\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Changed);
        assert!(serialize(&doc).starts_with(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">"
        ));
    }

    #[test]
    fn clean_namespace_declarations_are_unchanged() {
        for input in [
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><use xlink:href=\"#a\"/></svg>",
            // Un-declaring the default namespace stays as is.
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g xmlns=\"\"/></svg>",
        ] {
            let mut doc = parse(input).unwrap();
            assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Unchanged, "{input}");
        }
    }
}
