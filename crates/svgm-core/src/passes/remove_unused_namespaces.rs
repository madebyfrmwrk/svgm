use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};
use std::collections::HashSet;

/// Conservative pass: only removes namespace declarations for editor-specific prefixes
/// that are not referenced by any element or attribute in the document.
pub struct RemoveUnusedNamespaces;

impl Pass for RemoveUnusedNamespaces {
    fn name(&self) -> &'static str {
        "removeUnusedNamespaces"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        // Collect all prefixes actually used in element names and attribute names
        let mut used_prefixes = HashSet::new();
        let ids = doc.traverse();
        for &id in &ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind {
                if let Some(ref prefix) = elem.prefix {
                    used_prefixes.insert(prefix.clone());
                }
                for attr in &elem.attributes {
                    if let Some(ref prefix) = attr.prefix {
                        used_prefixes.insert(prefix.clone());
                    }
                }
            }
        }

        // Remove unused namespace declarations
        let mut changed = false;
        for &id in &ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                let before = elem.namespaces.len();
                elem.namespaces.retain(|ns| {
                    // Always keep the default namespace (empty prefix)
                    if ns.prefix.is_empty() {
                        return true;
                    }
                    // Keep if the prefix is actually used
                    used_prefixes.contains(&ns.prefix)
                });
                if elem.namespaces.len() != before {
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
    fn removes_unused_xlink_namespace() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUnusedNamespaces.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("xlink"));
    }

    #[test]
    fn keeps_used_namespace() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><use xlink:href=\"#a\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUnusedNamespaces.run(&mut doc), PassResult::Unchanged);
    }
}
