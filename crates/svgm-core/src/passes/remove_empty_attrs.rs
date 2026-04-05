use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

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
                    // Keep attributes that are meaningful when empty (e.g., alt, title)
                    // For SVG, empty string attributes are generally useless
                    !attr.value.is_empty()
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
}
