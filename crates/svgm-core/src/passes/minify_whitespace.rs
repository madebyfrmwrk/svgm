use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

/// Remove whitespace-only text nodes that exist purely as formatting.
/// Preserves whitespace inside <text>, <tspan>, <textPath>, <title>, <desc>,
/// <style>, <script>, and <foreignObject>.
pub struct MinifyWhitespace;

const TEXT_CONTENT_ELEMENTS: &[&str] = &[
    "text", "tspan", "textPath", "title", "desc",
    "style", "script", "foreignObject",
];

impl Pass for MinifyWhitespace {
    fn name(&self) -> &'static str {
        "minifyWhitespace"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            if let NodeKind::Text(ref text) = node.kind {
                if text.trim().is_empty() {
                    // Check if parent is a text-content element
                    if let Some(parent_id) = node.parent {
                        if let NodeKind::Element(ref parent_elem) = doc.node(parent_id).kind {
                            if TEXT_CONTENT_ELEMENTS.contains(&parent_elem.name.as_str()) {
                                continue;
                            }
                        }
                    }
                    doc.remove(id);
                    changed = true;
                }
            }
        }

        if changed { PassResult::Changed } else { PassResult::Unchanged }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_formatting_whitespace() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\">\n  <rect/>\n  <circle r=\"5\"/>\n</svg>";
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
}
