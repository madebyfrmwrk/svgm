use super::{Pass, PassResult};
use crate::ast::{Document, NodeId, NodeKind};

/// Moves a group's `transform` attribute down to its children when safe.
/// This enables `collapseGroups` to remove the now-empty group wrapper.
/// Only processes multi-child groups (single-child is handled by `convertTransform`).
pub struct MoveGroupAttrsToElems;

/// Elements that can safely receive a transform.
const MOVEABLE_ELEMENTS: &[&str] = &[
    "circle", "ellipse", "line", "path", "polygon", "polyline", "rect", "g", "text", "use",
    "image", "svg",
];

/// Attributes that may contain url() references — if any child has these,
/// moving the group's transform would break the coordinate space for the reference.
const URL_REF_ATTRS: &[&str] = &[
    "clip-path",
    "fill",
    "filter",
    "marker-end",
    "marker-mid",
    "marker-start",
    "mask",
    "stroke",
];

impl Pass for MoveGroupAttrsToElems {
    fn name(&self) -> &'static str {
        "moveGroupAttrsToElems"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for &id in &ids {
            if doc.node(id).removed {
                continue;
            }

            let node = doc.node(id);
            let group_elem = match &node.kind {
                NodeKind::Element(e) if e.name == "g" && e.prefix.is_none() => e,
                _ => continue,
            };

            // Only process groups that have ONLY a transform attribute
            // (and possibly xmlns-related attrs)
            let transform_value = match group_elem.attr("transform") {
                Some(v) => v.to_string(),
                None => continue,
            };

            // Check that transform is the only presentation attribute on the group
            let has_other_presentation = group_elem.attributes.iter().any(|a| {
                a.prefix.is_none()
                    && a.name != "transform"
                    && a.name != "id"
                    && !a.name.starts_with("xmlns")
            });
            if has_other_presentation {
                continue;
            }

            // Group must have an id-less group
            if group_elem.attr("id").is_some() {
                continue;
            }

            // Get element children
            let children: Vec<NodeId> = doc.children(id).collect();
            let elem_children: Vec<NodeId> = children
                .iter()
                .copied()
                .filter(|&c| matches!(&doc.node(c).kind, NodeKind::Element(_)))
                .collect();

            // Need at least 2 element children (single-child handled by convertTransform)
            if elem_children.len() < 2 {
                continue;
            }

            // Verify all children are moveable elements
            let all_moveable = elem_children.iter().all(|&c| {
                matches!(&doc.node(c).kind, NodeKind::Element(e) if MOVEABLE_ELEMENTS.contains(&e.name.as_str()))
            });
            if !all_moveable {
                continue;
            }

            // Check no child has url() references that would break with transform change
            let has_url_refs = elem_children.iter().any(|&c| {
                if let NodeKind::Element(ref e) = doc.node(c).kind {
                    e.attributes.iter().any(|a| {
                        a.prefix.is_none()
                            && URL_REF_ATTRS.contains(&a.name.as_str())
                            && a.value.contains("url(")
                    })
                } else {
                    false
                }
            });
            if has_url_refs {
                continue;
            }

            // Check no child has id (may be referenced from elsewhere)
            let has_id = elem_children.iter().any(
                |&c| matches!(&doc.node(c).kind, NodeKind::Element(e) if e.attr("id").is_some()),
            );
            if has_id {
                continue;
            }

            // Safe to move: prepend group transform to each child's transform
            for &child_id in &elem_children {
                let child = doc.node_mut(child_id);
                if let NodeKind::Element(ref mut child_elem) = child.kind {
                    if let Some(existing) = child_elem
                        .attributes
                        .iter_mut()
                        .find(|a| a.prefix.is_none() && a.name == "transform")
                    {
                        // Prepend group transform
                        existing.value = format!("{} {}", transform_value, existing.value);
                    } else {
                        // Add transform
                        child_elem.attributes.push(crate::ast::Attribute {
                            prefix: None,
                            name: "transform".to_string(),
                            value: transform_value.clone(),
                        });
                    }
                }
            }

            // Remove transform from group
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                elem.attributes
                    .retain(|a| !(a.prefix.is_none() && a.name == "transform"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn moves_transform_to_children() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><path d="M0 0"/><rect width="5" height="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveGroupAttrsToElems.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        // Group should no longer have transform
        assert!(!output.contains("g transform="));
        // Children should have it
        assert!(output.contains("path d=\"M0 0\" transform=\"translate(10,20)\""));
    }

    #[test]
    fn prepends_to_existing_child_transform() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><path d="M0 0" transform="scale(2)"/><rect width="5" height="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveGroupAttrsToElems.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("transform=\"translate(10,20) scale(2)\""));
    }

    #[test]
    fn skips_when_child_has_url_ref() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><path d="M0 0" fill="url(#grad)"/><rect width="5" height="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveGroupAttrsToElems.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn skips_when_child_has_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><path id="p" d="M0 0"/><rect width="5" height="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveGroupAttrsToElems.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn skips_group_with_other_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)" fill="red"><path d="M0 0"/><rect width="5" height="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveGroupAttrsToElems.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn skips_group_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g id="g1" transform="translate(10,20)"><path d="M0 0"/><rect width="5" height="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveGroupAttrsToElems.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn skips_single_child() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><path d="M0 0"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(MoveGroupAttrsToElems.run(&mut doc), PassResult::Unchanged);
    }
}
