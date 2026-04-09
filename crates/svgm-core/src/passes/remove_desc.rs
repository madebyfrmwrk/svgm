use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Removes `<desc>` elements that contain editor-generated descriptions
/// (e.g., "Created with Figma", "Created using Inkscape") or are empty.
/// Custom descriptions are preserved for accessibility.
pub struct RemoveDesc;

/// Patterns that indicate an editor-generated description.
const EDITOR_PATTERNS: &[&str] = &[
    "Created with",
    "Created using",
    "Generator:",
    "Made with",
    "Produced by",
];

impl Pass for RemoveDesc {
    fn name(&self) -> &'static str {
        "removeDesc"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        for id in ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind
                && elem.name == "desc"
                && elem.prefix.is_none()
                && should_remove(doc, id)
            {
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

/// Check if a `<desc>` element should be removed:
/// - Empty (no children or only whitespace text)
/// - Contains text matching an editor pattern
fn should_remove(doc: &Document, id: crate::ast::NodeId) -> bool {
    let children: Vec<_> = doc.children(id).collect();

    // Empty desc
    if children.is_empty() {
        return true;
    }

    // Single text child — check content
    if children.len() == 1
        && let NodeKind::Text(ref text) = doc.node(children[0]).kind
    {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return true;
        }
        // Check editor patterns
        for pattern in EDITOR_PATTERNS {
            if trimmed.starts_with(pattern) {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_editor_desc() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><desc>Created with Figma</desc><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDesc.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("desc"));
    }

    #[test]
    fn removes_empty_desc() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><desc></desc><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDesc.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("desc"));
    }

    #[test]
    fn removes_whitespace_only_desc() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><desc>   </desc><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDesc.run(&mut doc), PassResult::Changed);
    }

    #[test]
    fn keeps_custom_desc() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><desc>A chart showing revenue growth</desc><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDesc.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(output.contains("desc"));
    }

    #[test]
    fn does_not_remove_title() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><title>My SVG</title><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDesc.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(output.contains("title"));
    }

    #[test]
    fn removes_created_using_pattern() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><desc>Created using Inkscape</desc><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDesc.run(&mut doc), PassResult::Changed);
    }

    #[test]
    fn removes_generator_pattern() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><desc>Generator: Adobe Illustrator 24.0</desc><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDesc.run(&mut doc), PassResult::Changed);
    }
}
