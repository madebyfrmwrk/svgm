use super::{Pass, PassResult, has_ancestor};
use crate::ast::{Document, NodeKind};

/// Remove whitespace-only text nodes that exist purely as formatting.
/// Preserves whitespace inside text content elements (SVGO's `textElems`:
/// <text>, <tspan>, <textPath>, <tref>, <altGlyph>, ..., <pre>, <title>),
/// <desc>, <style>, <script>, <foreignObject>, and anywhere below a <text>
/// element (an `<a>` nested in `<text>` still carries rendered whitespace
/// between words). `<a>` outside text content is a plain container, so its
/// formatting whitespace is dropped.
pub struct MinifyWhitespace;

const TEXT_CONTENT_ELEMENTS: &[&str] = &[
    "altGlyph",
    "altGlyphDef",
    "altGlyphItem",
    "glyph",
    "glyphRef",
    "pre",
    "text",
    "textPath",
    "title",
    "tref",
    "tspan",
    "desc",
    "style",
    "script",
    "foreignObject",
];

/// Elements whose entire subtree is rendered text: whitespace-only text nodes
/// anywhere below them are significant.
const TEXT_ROOT_ELEMENTS: &[&str] = &["text", "tspan", "textPath", "foreignObject"];

impl Pass for MinifyWhitespace {
    fn name(&self) -> &'static str {
        "minifyWhitespace"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            if let NodeKind::Text(ref text) = node.kind
                && text.trim().is_empty()
            {
                // Check if parent is a text-content element
                if let Some(parent_id) = node.parent
                    && let NodeKind::Element(ref parent_elem) = doc.node(parent_id).kind
                    && TEXT_CONTENT_ELEMENTS.contains(&parent_elem.name.as_str())
                {
                    continue;
                }
                // ... or if any ancestor renders its subtree as text.
                if has_ancestor(doc, id, TEXT_ROOT_ELEMENTS) {
                    continue;
                }
                doc.remove(id);
                changed = true;
            }
        }

        if changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_formatting_whitespace() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\">\n  <rect/>\n  <circle r=\"5\"/>\n</svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(MinifyWhitespace.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains('\n'));
        assert!(output.contains("<rect/><circle"));
    }

    #[test]
    fn preserves_text_content_whitespace() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text> hello </text></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(MinifyWhitespace.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn preserves_whitespace_in_link_nested_in_text() {
        // The space between the tspans is rendered: "read more", not "readmore".
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text><a href=\"/x\"><tspan>read</tspan> <tspan>more</tspan></a></text></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(MinifyWhitespace.run(&mut doc), PassResult::Unchanged);
        assert_eq!(serialize(&doc), input);
    }

    #[test]
    fn preserves_whitespace_in_tref_and_altglyph() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text><tref href=\"#t\"> </tref><altGlyph href=\"#g\"> </altGlyph></text></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(MinifyWhitespace.run(&mut doc), PassResult::Unchanged);
        assert_eq!(serialize(&doc), input);
    }

    #[test]
    fn still_strips_whitespace_around_link_outside_text() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\">\n  <a href=\"/x\">\n    <rect/>\n  </a>\n</svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(MinifyWhitespace.run(&mut doc), PassResult::Changed);
        assert_eq!(
            serialize(&doc),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><a href=\"/x\"><rect/></a></svg>"
        );
    }
}
