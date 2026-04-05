use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

/// Opt-in pass: removes <desc> elements.
/// NOT included in default preset because <desc> carries accessibility semantics.
pub struct RemoveDesc;

impl Pass for RemoveDesc {
    fn name(&self) -> &'static str {
        "removeDesc"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        for id in ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind
                && matches!(elem.name.as_str(), "desc" | "title") {
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
    fn removes_desc_and_title() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><title>My SVG</title><desc>A description</desc><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDesc.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("desc"));
        assert!(!output.contains("title"));
        assert!(output.contains("<rect"));
    }
}
