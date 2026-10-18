use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

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
                && elem.name == "metadata"
                // `<foo:metadata>` is foreign content, not SVG metadata
                // (SVGO compares the qualified name).
                && elem.prefix.is_none()
            {
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
    fn removes_metadata() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><metadata><rdf:RDF/></metadata><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveMetadata.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("metadata"));
    }

    #[test]
    fn keeps_prefixed_metadata_elements() {
        // SVGO parity: node.name is "foo:metadata", which !== "metadata".
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:foo=\"http://example.com/foo\"><foo:metadata>keep</foo:metadata><metadata>drop</metadata><rect/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveMetadata.run(&mut doc), PassResult::Changed);
        assert_eq!(
            serialize(&doc),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:foo=\"http://example.com/foo\"><foo:metadata>keep</foo:metadata><rect/></svg>"
        );
    }

    #[test]
    fn prefixed_metadata_only_is_unchanged() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:foo=\"http://example.com/foo\"><foo:metadata>keep</foo:metadata></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveMetadata.run(&mut doc), PassResult::Unchanged);
        assert_eq!(serialize(&doc), input);
    }
}
