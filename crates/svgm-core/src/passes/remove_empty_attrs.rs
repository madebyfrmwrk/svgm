use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Conditional processing attributes (SVGO `attrsGroups.conditionalProcessing`).
/// An empty value evaluates to false and keeps the element from rendering,
/// so removing it would make hidden content visible.
const CONDITIONAL_PROCESSING_ATTRS: &[&str] =
    &["requiredExtensions", "requiredFeatures", "systemLanguage"];

pub struct RemoveEmptyAttrs;

impl Pass for RemoveEmptyAttrs {
    fn name(&self) -> &'static str {
        "removeEmptyAttrs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                let before = elem.attributes.len();
                elem.attributes.retain(|attr| {
                    // Empty attributes are useless, except the conditional
                    // processing ones, which hide the element when empty.
                    !attr.value.is_empty()
                        || (attr.prefix.is_none()
                            && CONDITIONAL_PROCESSING_ATTRS.contains(&attr.name.as_str()))
                });
                if elem.attributes.len() != before {
                    changed = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_empty_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect class="" fill="red"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("class"));
        assert!(output.contains("fill"));
    }

    #[test]
    fn svgo_fixture_02_keeps_empty_conditional_processing_attrs() {
        // SVGO removeEmptyAttrs.02: empty conditional processing attributes
        // prevent elements from rendering and must be preserved.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g requiredFeatures=""></g><g requiredExtensions=""></g><g systemLanguage=""></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyAttrs.run(&mut doc), PassResult::Unchanged);
        assert_eq!(serialize(&doc), input);

        // Other empty attributes on the same element still go.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect systemLanguage="" class="" width="1"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyAttrs.run(&mut doc), PassResult::Changed);
        assert_eq!(
            serialize(&doc),
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect systemLanguage="" width="1"/></svg>"#
        );
    }
}
