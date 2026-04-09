use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Removes elements inside `<defs>` that have no `id` and are not `<style>`.
/// If `<defs>` becomes empty, removes it entirely.
pub struct RemoveUselessDefs;

impl Pass for RemoveUselessDefs {
    fn name(&self) -> &'static str {
        "removeUselessDefs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        // First pass: remove useless children of <defs>
        for &id in &ids {
            let node = doc.node(id);
            let is_defs = matches!(&node.kind, NodeKind::Element(e) if e.name == "defs");
            if !is_defs {
                continue;
            }

            let children: Vec<_> = doc.children(id).collect();
            for child_id in children {
                if should_remove(doc, child_id) {
                    doc.remove(child_id);
                    changed = true;
                }
            }
        }

        // Second pass: remove now-empty <defs>
        if changed {
            let ids = doc.traverse();
            for id in ids {
                let node = doc.node(id);
                let is_defs = matches!(&node.kind, NodeKind::Element(e) if e.name == "defs");
                if is_defs && doc.children(id).next().is_none() {
                    doc.remove(id);
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

/// An element in defs is useless if it has no `id` and is not a `<style>`.
fn should_remove(doc: &Document, id: crate::ast::NodeId) -> bool {
    let node = doc.node(id);
    match &node.kind {
        NodeKind::Element(e) => {
            if e.name == "style" {
                return false;
            }
            if e.attr("id").is_some() {
                return false;
            }
            true
        }
        // Keep text nodes (whitespace etc.)
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_defs_child_without_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient><stop offset="0"/></linearGradient></defs><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUselessDefs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("defs"));
        assert!(!output.contains("linearGradient"));
    }

    #[test]
    fn keeps_defs_child_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="g1"><stop offset="0"/></linearGradient></defs><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUselessDefs.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(output.contains("linearGradient"));
    }

    #[test]
    fn keeps_style_in_defs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><style>.a{fill:red}</style></defs><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUselessDefs.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(output.contains("style"));
    }

    #[test]
    fn removes_empty_defs_after_cleanup() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><clipPath/></defs><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUselessDefs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("defs"));
    }

    #[test]
    fn unchanged_when_no_defs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUselessDefs.run(&mut doc), PassResult::Unchanged);
    }
}
