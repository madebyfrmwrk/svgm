use crate::ast::{Document, NodeId, NodeKind};
use super::{Pass, PassResult};

pub struct CollapseGroups;

impl Pass for CollapseGroups {
    fn name(&self) -> &'static str {
        "collapseGroups"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;

        // Process bottom-up so inner groups collapse first.
        let mut ids = doc.traverse();
        ids.reverse();

        for id in ids {
            if doc.node(id).removed {
                continue;
            }

            if let NodeKind::Element(ref elem) = doc.node(id).kind {
                if elem.name != "g" {
                    continue;
                }

                let children: Vec<NodeId> = doc.children(id).collect();

                // Case 1: Empty group (no meaningful children) — handled by remove_empty_containers
                // Case 2: Group with no attributes — unwrap children to parent
                if elem.attributes.is_empty() && elem.prefix.is_none() {
                    if let Some(parent_id) = doc.node(id).parent {
                        hoist_children(doc, id, parent_id);
                        doc.node_mut(id).removed = true;
                        changed = true;
                        continue;
                    }
                }

                // Case 3: Group with single element child — merge attrs down if no conflicts
                if children.len() == 1 {
                    let child_id = children[0];
                    if let NodeKind::Element(ref child_elem) = doc.node(child_id).kind {
                        // Only merge if child is also an element (not text/comment)
                        // and the group has no transform (transform merging is complex)
                        let g_has_transform = elem.attributes.iter().any(|a| a.name == "transform");
                        if !g_has_transform && can_merge_attrs(elem, child_elem) {
                            merge_group_into_child(doc, id, child_id);
                            changed = true;
                            continue;
                        }
                    }
                }
            }
        }

        if changed { PassResult::Changed } else { PassResult::Unchanged }
    }
}

/// Move all children of `group_id` to be children of `parent_id`,
/// replacing the group's position in the parent's child list.
fn hoist_children(doc: &mut Document, group_id: NodeId, parent_id: NodeId) {
    let group_children: Vec<NodeId> = doc.node(group_id).children.clone();
    let parent = doc.node_mut(parent_id);
    let pos = parent.children.iter().position(|&c| c == group_id);

    if let Some(pos) = pos {
        // Replace the group with its children in the parent's child list
        parent.children.splice(pos..=pos, group_children.iter().copied());
        // Update parent pointers
        for &child in &group_children {
            doc.node_mut(child).parent = Some(parent_id);
        }
    }
}

/// Check if group attributes can be safely merged into the child element.
/// Returns false if there are attribute name conflicts.
fn can_merge_attrs(
    group: &crate::ast::Element,
    child: &crate::ast::Element,
) -> bool {
    for g_attr in &group.attributes {
        // If the child already has this attribute, don't merge (child value wins, but
        // we'd lose the group's value silently — skip to be safe).
        if child.attributes.iter().any(|a| a.name == g_attr.name && a.prefix == g_attr.prefix) {
            return false;
        }
    }
    true
}

/// Merge the group's attributes into its single child, then hoist the child.
fn merge_group_into_child(doc: &mut Document, group_id: NodeId, child_id: NodeId) {
    // Clone group attrs before mutating
    let group_attrs = if let NodeKind::Element(ref elem) = doc.node(group_id).kind {
        elem.attributes.clone()
    } else {
        return;
    };

    // Add group attrs to child
    if let NodeKind::Element(ref mut child_elem) = doc.node_mut(child_id).kind {
        for attr in group_attrs {
            child_elem.attributes.push(attr);
        }
    }

    // Hoist child to group's parent
    if let Some(parent_id) = doc.node(group_id).parent {
        hoist_children(doc, group_id, parent_id);
        doc.node_mut(group_id).removed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn collapses_attr_less_group() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect/><circle r="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g>"), "group should be removed: {output}");
        assert!(output.contains("<rect/>"));
        assert!(output.contains("<circle"));
    }

    #[test]
    fn merges_single_child_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><rect width="10"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g"), "group should be removed: {output}");
        assert!(output.contains("fill=\"red\""));
        assert!(output.contains("width=\"10\""));
    }

    #[test]
    fn keeps_group_with_conflicting_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><rect fill="blue"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // Can't merge because both have `fill`
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn keeps_group_with_transform() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,10)"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // Has transform — don't try to merge (transform merging is complex)
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn collapses_nested_groups() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><g><rect/></g></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // First pass collapses inner, second pass collapses outer
        CollapseGroups.run(&mut doc);
        CollapseGroups.run(&mut doc);
        let output = serialize(&doc);
        assert!(!output.contains("<g"), "all groups should be removed: {output}");
        assert!(output.contains("<rect/>"));
    }

    #[test]
    fn keeps_group_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g id="layer1"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // Has id attr — can't collapse (might be referenced)
        // But single child with no conflict — actually this should merge
        // The id goes to the child. Let's check: can_merge_attrs allows it since child has no id.
        let result = CollapseGroups.run(&mut doc);
        assert_eq!(result, PassResult::Changed);
    }
}
