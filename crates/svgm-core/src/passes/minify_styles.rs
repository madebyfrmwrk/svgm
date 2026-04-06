use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};
use crate::passes::convert_colors::shorten_color;

pub struct MinifyStyles;

impl Pass for MinifyStyles {
    fn name(&self) -> &'static str {
        "minifyStyles"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for nid in ids {
            if let NodeKind::Element(ref elem) = doc.node(nid).kind
                && elem.name == "style"
                && elem.prefix.is_none()
            {
                // Collect text content
                let children: Vec<_> = doc.children(nid).collect();
                for child_id in children {
                    let node = doc.node_mut(child_id);
                    match &mut node.kind {
                        NodeKind::Text(text) => {
                            let minified = minify_css(text);
                            if minified != *text {
                                *text = minified;
                                changed = true;
                            }
                        }
                        NodeKind::CData(text) => {
                            let minified = minify_css(text);
                            if minified != *text {
                                *text = minified;
                                changed = true;
                            }
                        }
                        _ => {}
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

/// Minify CSS text: strip comments, collapse whitespace, shorten colors.
fn minify_css(css: &str) -> String {
    let mut result = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip CSS comments
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2; // skip */
            continue;
        }

        let ch = bytes[i];

        // Collapse whitespace
        if ch.is_ascii_whitespace() {
            // Look at what's around the whitespace
            let before = result.as_bytes().last().copied();
            // Skip all consecutive whitespace
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let after = if i < bytes.len() {
                Some(bytes[i])
            } else {
                None
            };

            // Only keep a space if both sides are "word-like" characters
            // Remove space around structural chars: { } : ; ,
            let structural = |b: Option<u8>| matches!(b, Some(b'{' | b'}' | b':' | b';' | b','));
            if !structural(before) && !structural(after) {
                result.push(' ');
            }
            continue;
        }

        // Remove trailing semicolon before }
        if ch == b';' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'}' {
                i += 1;
                continue;
            }
        }

        result.push(ch as char);
        i += 1;
    }

    // Shorten colors in property values
    let shortened = shorten_colors_in_css(&result);
    // Trim leading/trailing whitespace that may remain from collapsed newlines
    shortened.trim().to_string()
}

/// Find color values in CSS and shorten them.
fn shorten_colors_in_css(css: &str) -> String {
    let mut result = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Look for ':' which precedes a value
        if bytes[i] == b':' {
            result.push(':');
            i += 1;

            // Find the end of the value (';' or '}')
            let val_start = i;
            let mut val_end = i;
            while val_end < bytes.len() && bytes[val_end] != b';' && bytes[val_end] != b'}' {
                val_end += 1;
            }

            let value = &css[val_start..val_end];
            if let Some(shortened) = shorten_color(value) {
                result.push_str(&shortened);
            } else {
                result.push_str(value);
            }
            i = val_end;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = MinifyStyles.run(&mut doc);
        (result, serialize(&doc))
    }

    #[test]
    fn whitespace_collapsed() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>\n  .cls {\n    fill: red;\n    stroke: blue;\n  }\n</style><rect class=\"cls\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // "blue" gets shortened to "#00f" by color shortener
        assert!(output.contains(".cls{fill:red;stroke:#00f}"));
    }

    #[test]
    fn comments_removed() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>/* comment */.cls{fill:red}</style><rect class=\"cls\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("comment"));
        assert!(output.contains(".cls{fill:red}"));
    }

    #[test]
    fn trailing_semicolons_removed() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red;}</style><rect class=\"cls\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains(".cls{fill:red}"));
    }

    #[test]
    fn no_style_unchanged() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn already_minified_unchanged() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red}</style><rect class=\"cls\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn colors_shortened() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:#ff0000}</style><rect class=\"cls\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // #ff0000 should be shortened to red or #f00
        assert!(output.contains("red") || output.contains("#f00"));
    }
}
