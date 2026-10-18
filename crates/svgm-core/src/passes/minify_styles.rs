use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};
use crate::passes::convert_colors::{COLOR_ATTRS, shorten_color};

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
///
/// Works on raw bytes. Every decision is made on ASCII bytes and every
/// byte that is kept is copied verbatim, so multi-byte UTF-8 sequences pass
/// through untouched (issue #23: casting bytes to `char` re-encoded them as
/// Latin-1 and doubled the text every iteration).
fn minify_css(css: &str) -> String {
    let mut result: Vec<u8> = Vec::with_capacity(css.len());
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
            let before = result.last().copied();
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
                result.push(b' ');
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

        result.push(ch);
        i += 1;
    }

    // Only ASCII bytes were dropped or inserted, so the result is valid UTF-8
    // whenever the input was. Fall back to the input if that ever fails.
    let result = String::from_utf8(result).unwrap_or_else(|_| css.to_string());

    // Shorten colors in property values
    let shortened = shorten_colors_in_css(&result);
    // Trim leading/trailing whitespace that may remain from collapsed newlines
    shortened.trim().to_string()
}

/// Shorten colour values of colour properties inside declaration blocks.
///
/// Tracks brace depth so a `:` only starts a value inside a block (never in
/// `.a:hover`; a selector nested in `@media{...}` is recognised by the `{`
/// that follows it), reads the property name back to the previous `;`/`{`,
/// keeps a `!important` suffix, and only touches [`COLOR_ATTRS`] properties
/// with a replacement that is no longer than the original (`green` stays
/// `green`, not `#008000`).
fn shorten_colors_in_css(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    // Everything before `copied` has been pushed to `out`.
    let mut copied = 0;
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut prop_start = 0;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'{' => {
                depth += 1;
                prop_start = i + 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                prop_start = i + 1;
            }
            b';' => prop_start = i + 1,
            b':' if depth > 0 => {
                let prop = css[prop_start..i].trim();
                let end = value_end(bytes, i + 1);
                let value = &css[i + 1..end];
                if value.contains('{') {
                    // `@media print{.a:hover{...}}`: a pseudo-class in a
                    // nested selector, not a declaration.
                    i += 1;
                    continue;
                }
                let trimmed = value.trim();
                let (color, suffix) = match trimmed.find('!') {
                    Some(bang) => (trimmed[..bang].trim_end(), &trimmed[bang..]),
                    None => (trimmed, ""),
                };
                if COLOR_ATTRS.contains(&prop)
                    && let Some(short) = shorten_color(color)
                    && short.len() <= color.len()
                    && short != color
                {
                    out.push_str(&css[copied..=i]);
                    out.push_str(&short);
                    out.push_str(suffix);
                    copied = end;
                }
                // `end` sits on the `;`/`}` (or the end of input), which the
                // loop handles next.
                i = end;
                continue;
            }
            _ => {}
        }
        i += 1;
    }

    out.push_str(&css[copied.min(css.len())..]);
    out
}

/// Index of the `;` or `}` ending the value that starts at `from`, skipping
/// quoted strings and parentheses (`url(data:...;base64,...)`). All stop
/// bytes are ASCII, so the result is always a char boundary.
fn value_end(bytes: &[u8], from: usize) -> usize {
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut j = from;
    while j < bytes.len() {
        let c = bytes[j];
        if let Some(q) = quote {
            if c == b'\\' {
                j += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else {
            match c {
                b'"' | b'\'' => quote = Some(c),
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b';' | b'}' if depth == 0 => return j,
                _ => {}
            }
        }
        j += 1;
    }
    bytes.len()
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

    #[test]
    fn issue_23_non_ascii_preserved_and_stable() {
        // Bytes used to be re-encoded as Latin-1 chars, doubling the text (issue #23).
        let css = ".ǎccent{fill:red}";
        assert_eq!(minify_css(css), css);

        let css = ".a { font-family: \"Söhne\"; content: \"→\" }";
        let minified = minify_css(css);
        assert_eq!(minified, ".a{font-family:\"Söhne\";content:\"→\"}");
        assert_eq!(minify_css(&minified), minified, "must be idempotent");

        // Non-ASCII inside a would-be color value is left alone, not corrupted.
        assert_eq!(minify_css(".a{fill:#aǎ}"), ".a{fill:#aǎ}");

        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.ǎccent{fill:red}</style><rect class=\"ǎccent\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains(".ǎccent{fill:red}"), "{output}");
        assert!(output.len() <= input.len());
    }

    // --- WP6-38: colour shortening must never lengthen or misfire ---

    #[test]
    fn named_colors_are_not_lengthened() {
        assert_eq!(minify_css(".a{fill:green}"), ".a{fill:green}");
        assert_eq!(minify_css(".a{color:gray}"), ".a{color:gray}");
        assert_eq!(minify_css(".a{stop-color:white}"), ".a{stop-color:#fff}");
        let (result, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{fill:green;color:gray}</style></svg>",
        );
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn pseudo_class_colon_is_not_a_declaration() {
        assert_eq!(
            minify_css(".a:hover{stroke:#FF0000}"),
            ".a:hover{stroke:red}"
        );
        assert_eq!(
            minify_css("a:not(.x){fill:#FFFFFF;stroke:#000000}"),
            "a:not(.x){fill:#fff;stroke:#000}"
        );
    }

    #[test]
    fn nested_pseudo_class_colon_is_not_a_declaration() {
        // Inside @media the selector's `:hover` sits at brace depth 1; it
        // must not swallow the first declaration of the block.
        let (_, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>@media print{.a:hover{fill:#FFFFFF;stroke:#FF0000}}@media (min-width:100px){.a:hover{fill:#FFFFFF}}</style><rect class=\"a\"/></svg>",
        );
        assert!(
            output.contains("@media print{.a:hover{fill:#fff;stroke:red}}"),
            "{output}"
        );
        assert!(
            output.contains("@media (min-width:100px){.a:hover{fill:#fff}}"),
            "{output}"
        );
    }

    #[test]
    fn important_suffix_is_kept() {
        let (_, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{fill:#FFFFFF!important;stroke:#FF0000 !important}</style><rect class=\"a\"/></svg>",
        );
        assert!(
            output.contains(".a{fill:#fff!important;stroke:red!important}"),
            "{output}"
        );
    }

    #[test]
    fn only_color_properties_are_shortened() {
        assert_eq!(
            minify_css(".a{font-family:White;content:\"#ffffff\"}"),
            ".a{font-family:White;content:\"#ffffff\"}"
        );
        assert_eq!(
            minify_css("@media (min-width:#fff){.a{fill:#ffffff}}"),
            "@media (min-width:#fff){.a{fill:#fff}}"
        );
        assert_eq!(
            minify_css(".a{fill:url(data:image/svg+xml;base64,AA==);stroke:#ff0000}"),
            ".a{fill:url(data:image/svg+xml;base64,AA==);stroke:red}"
        );
        assert_eq!(
            minify_css(".a{fill:#aǎ;stroke:#FFF}"),
            ".a{fill:#aǎ;stroke:#fff}"
        );
    }
}
