use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

/// Container elements that can be removed when empty.
const REMOVABLE_CONTAINERS: &[&str] = &[
    "g", "defs", "symbol", "clipPath", "mask", "pattern",
    "linearGradient", "radialGradient", "filter", "marker",
];

pub struct RemoveEmptyContainers;

impl Pass for RemoveEmptyContainers {
    fn name(&self) -> &'static str {
        "removeEmptyContainers"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            if let NodeKind::Element(ref elem) = node.kind
                && REMOVABLE_CONTAINERS.contains(&elem.name.as_str()) {
                    // Consider empty if no children, or only whitespace text children
                    let has_meaningful_children = doc.children(id).any(|child| {
                        match &doc.node(child).kind {
                            NodeKind::Text(t) => !t.trim().is_empty(),
                            _ => true,
                        }
                    });
                    if !has_meaningful_children {
                        // Don't remove if it has an id — it might be referenced
                        if elem.attr("id").is_none() {
                            doc.remove(id);
                            changed = true;
                        }
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
    fn removes_empty_g() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g></g><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g>"));
    }

    #[test]
    fn keeps_g_with_children() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn keeps_empty_g_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g id="target"></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn removes_nested_empty_containers_across_passes() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><g></g></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // First pass removes inner <g>
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
        // Second pass removes now-empty outer <g>
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g>"));
    }
}
