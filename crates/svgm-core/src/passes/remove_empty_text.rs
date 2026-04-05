use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

pub struct RemoveEmptyText;

impl Pass for RemoveEmptyText {
    fn name(&self) -> &'static str {
        "removeEmptyText"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind
                && matches!(elem.name.as_str(), "text" | "tspan" | "textPath") {
                    // Remove text elements that have no text content children
                    let has_text = doc.children(id).any(|child| {
                        match &doc.node(child).kind {
                            NodeKind::Text(t) => !t.trim().is_empty(),
                            NodeKind::Element(_) => true, // nested tspan etc.
                            _ => false,
                        }
                    });
                    if !has_text {
                        doc.remove(id);
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
    fn removes_empty_text_element() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text></text><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<text"));
    }

    #[test]
    fn keeps_text_with_content() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text>Hello</text></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Unchanged);
    }
}
