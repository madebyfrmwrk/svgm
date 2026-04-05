use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

pub struct RemoveComments;

impl Pass for RemoveComments {
    fn name(&self) -> &'static str {
        "removeComments"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        for id in ids {
            if matches!(doc.node(id).kind, NodeKind::Comment(_)) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_comments() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- hello --><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveComments.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<!--"));
        assert!(output.contains("<rect/>"));
    }

    #[test]
    fn no_change_without_comments() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveComments.run(&mut doc), PassResult::Unchanged);
    }
}
