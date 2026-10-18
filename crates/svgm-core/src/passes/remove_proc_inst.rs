use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Removes the XML declaration (`<?xml version="1.0"?>`), which the parser
/// keeps as a processing instruction with target `xml` (SVGO
/// `removeXMLProcInst`). Other processing instructions, such as
/// `<?xml-stylesheet?>`, carry meaning and are kept.
pub struct RemoveProcInst;

impl Pass for RemoveProcInst {
    fn name(&self) -> &'static str {
        "removeProcInst"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        for id in ids {
            if matches!(
                doc.node(id).kind,
                NodeKind::ProcessingInstruction { ref target, .. } if target == "xml"
            ) {
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
    fn svgo_fixture_01_removes_xml_declaration() {
        let input = "<?xml version=\"1.0\" standalone=\"no\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\">test</svg>";
        let mut doc = parse(input).unwrap();
        assert!(serialize(&doc).starts_with("<?xml version=\"1.0\" standalone=\"no\"?>"));
        assert_eq!(RemoveProcInst.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<?xml"), "{output}");
        assert!(output.contains("<svg"), "{output}");
    }

    #[test]
    fn svgo_fixture_02_keeps_other_processing_instructions() {
        let input = "<?xml-stylesheet href=\"style.css\" type=\"text/css\"?><?foo bar?><svg xmlns=\"http://www.w3.org/2000/svg\"><?inner?>test</svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveProcInst.run(&mut doc), PassResult::Unchanged);
        assert_eq!(serialize(&doc), input);
    }

    #[test]
    fn no_change_without_declaration() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveProcInst.run(&mut doc), PassResult::Unchanged);
    }
}
