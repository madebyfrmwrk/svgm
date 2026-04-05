use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

pub struct RemoveMetadata;

impl Pass for RemoveMetadata {
    fn name(&self) -> &'static str {
        "removeMetadata"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        for id in ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind
                && elem.name == "metadata" {
                    doc.remove(id);
                    changed = true;
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
    fn removes_metadata() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><metadata><rdf:RDF/></metadata><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveMetadata.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("metadata"));
    }
}
