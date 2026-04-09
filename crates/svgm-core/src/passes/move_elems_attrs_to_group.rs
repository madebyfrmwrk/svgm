use super::{Pass, PassResult};
use crate::ast::{Document, NodeId, NodeKind};

/// Moves common inheritable presentation attributes from all children of a `<g>`
/// up to the group element, reducing redundancy and improving compression.
pub struct MoveElemsAttrsToGroup;

/// Inheritable presentation attributes that can safely be promoted to a group.
const INHERITABLE_ATTRS: &[&str] = &[
    "clip-rule",
    "color",
    "color-interpolation",
    "color-interpolation-filters",
    "cursor",
    "direction",
    "dominant-baseline",
    "fill",
    "fill-opacity",
    "fill-rule",
    "font",
    "font-family",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-variant",
    "font-weight",
    "image-rendering",
    "letter-spacing",
    "marker",
    "marker-start",
    "marker-mid",
    "marker-end",
    "paint-order",
    "pointer-events",
    "shape-rendering",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "text-anchor",
    "text-rendering",
    "visibility",
    "word-spacing",
    "writing-mode",
];

impl Pass for MoveElemsAttrsToGroup {
    fn name(&self) -> &'static str {
        "moveElemsAttrsToGroup"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let ids = doc.traverse();

        // Bail if <style> elements exist — CSS selectors may target specific elements
        for &id in &ids {
            if let NodeKind::Element(ref e) = doc.node(id).kind
                && e.name == "style"
            {
                return PassResult::Unchanged;
            }
        }

        let mut changed = false;

        for &id in &ids {
            let node = doc.node(id);
            let is_group =
                matches!(&node.kind, NodeKind::Element(e) if e.name == "g" && e.prefix.is_none());
            if !is_group {
                continue;
            }

            // Collect element children
            let children: Vec<NodeId> = doc.children(id).collect();
            let elem_children: Vec<NodeId> = children
                .iter()
                .copied()
                .filter(|&c| matches!(&doc.node(c).kind, NodeKind::Element(_)))
                .collect();

            if elem_children.len() < 2 {
                continue;
            }

            // Find common inheritable attrs across ALL element children
            let common = find_common_attrs(doc, &elem_children);
            if common.is_empty() {
                continue;
            }

            // Check which common attrs the group doesn't already have
            let group_elem = match &doc.node(id).kind {
                NodeKind::Element(e) => e,
                _ => continue,
            };
            let attrs_to_move: Vec<(String, String)> = common
                .into_iter()
                .filter(|(name, _)| group_elem.attr(name).is_none())
                .collect();

            if attrs_to_move.is_empty() {
                continue;
            }

            // Move: add to group, remove from children
            for (name, value) in &attrs_to_move {
                // Add to group
                let node = doc.node_mut(id);
                if let NodeKind::Element(ref mut elem) = node.kind {
                    elem.attributes.push(crate::ast::Attribute {
                        prefix: None,
                        name: name.clone(),
                        value: value.clone(),
                    });
                }

                // Remove from each child
                for &child_id in &elem_children {
                    let child = doc.node_mut(child_id);
                    if let NodeKind::Element(ref mut child_elem) = child.kind {
                        child_elem
                            .attributes
                            .retain(|a| a.prefix.is_some() || a.name != *name);
                    }
                }
            }
            changed = true;
        }

        if changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

/// Find inheritable attrs that ALL element children share with the same value.
fn find_common_attrs(doc: &Document, children: &[NodeId]) -> Vec<(String, String)> {
    if children.is_empty() {
        return Vec::new();
    }

    // Start with attrs from the first child
    let first = match &doc.node(children[0]).kind {
        NodeKind::Element(e) => e,
        _ => return Vec::new(),
    };

    let mut candidates: Vec<(String, String)> = first
        .attributes
        .iter()
        .filter(|a| a.prefix.is_none() && INHERITABLE_ATTRS.contains(&a.name.as_str()))
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect();

    // Intersect with remaining children
    for &child_id in &children[1..] {
        let child_elem = match &doc.node(child_id).kind {
            NodeKind::Element(e) => e,
            _ => {
                candidates.clear();
                break;
            }
        };

        candidates.retain(|(name, value)| {
            child_elem
                .attributes
                .iter()
                .any(|a| a.prefix.is_none() && a.name == *name && a.value == *value)
        });

        if candidates.is_empty() {
            break;
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn moves_common_fill_to_group() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect fill="red"/><circle fill="red"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveElemsAttrsToGroup.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("<g fill=\"red\""));
        // Children should not have fill anymore
        assert!(!output.contains("rect fill") && !output.contains("circle fill"));
    }

    #[test]
    fn does_not_move_when_values_differ() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect fill="red"/><circle fill="blue"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveElemsAttrsToGroup.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn skips_single_child() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect fill="red"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveElemsAttrsToGroup.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn skips_when_group_already_has_attr() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="blue"><rect fill="red"/><circle fill="red"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveElemsAttrsToGroup.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn bails_when_style_present() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><style>.a{fill:blue}</style><g><rect fill="red"/><circle fill="red"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveElemsAttrsToGroup.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn does_not_move_non_inheritable() {
        // clip-path is not inheritable — should not be moved
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect clip-path="url(#c)"/><circle clip-path="url(#c)"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveElemsAttrsToGroup.run(&mut doc), PassResult::Unchanged);
    }
}
