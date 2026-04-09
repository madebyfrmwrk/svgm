use std::collections::HashMap;

use super::{Pass, PassResult};
use crate::ast::{Document, NodeId, NodeKind};

/// Sorts children of `<defs>` by element name frequency for better gzip/brotli compression.
pub struct SortDefsChildren;

impl Pass for SortDefsChildren {
    fn name(&self) -> &'static str {
        "sortDefsChildren"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        // Count element name frequencies across the entire document
        let mut freq: HashMap<String, usize> = HashMap::new();
        for id in doc.traverse() {
            if let NodeKind::Element(ref e) = doc.node(id).kind {
                *freq.entry(e.name.clone()).or_default() += 1;
            }
        }

        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let is_defs = matches!(&doc.node(id).kind, NodeKind::Element(e) if e.name == "defs");
            if !is_defs {
                continue;
            }

            let children: Vec<NodeId> = doc.children(id).collect();
            if children.len() < 2 {
                continue;
            }

            // Build sortable entries: (freq, name, original_index, node_id)
            let entries: Vec<(usize, String, usize, NodeId)> = children
                .iter()
                .enumerate()
                .map(|(i, &child_id)| {
                    let name = match &doc.node(child_id).kind {
                        NodeKind::Element(e) => e.name.clone(),
                        _ => String::new(),
                    };
                    let f = freq.get(&name).copied().unwrap_or(0);
                    (f, name, i, child_id)
                })
                .collect();

            // Sort: highest frequency first, then alphabetically by name
            let mut sorted = entries.clone();
            sorted.sort_by(|a, b| {
                b.0.cmp(&a.0) // frequency descending
                    .then_with(|| a.1.cmp(&b.1)) // name ascending
            });

            // Check if order changed
            let order_changed = entries.iter().zip(sorted.iter()).any(|(a, b)| a.3 != b.3);

            if order_changed {
                // Rewrite the children vec on the defs node
                let new_children: Vec<NodeId> = sorted.iter().map(|e| e.3).collect();
                doc.node_mut(id).children = new_children;
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
    fn sorts_by_frequency() {
        // 3 rects in the body, 1 circle — rect is more frequent
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><circle id="c"/><rect id="r1"/></defs><rect/><rect/><rect/><circle/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(SortDefsChildren.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        // rect should come first (frequency 4 vs 2)
        let rect_pos = output.find("rect id").unwrap();
        let circle_pos = output.find("circle id").unwrap();
        assert!(rect_pos < circle_pos);
    }

    #[test]
    fn unchanged_when_already_sorted() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><rect id="r1"/><circle id="c"/></defs><rect/><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        // rect has freq 3, circle has freq 1 — already in order
        assert_eq!(SortDefsChildren.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn unchanged_single_child() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><rect id="r"/></defs></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(SortDefsChildren.run(&mut doc), PassResult::Unchanged);
    }
}
