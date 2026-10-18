use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

pub struct RemoveEmptyText;

impl Pass for RemoveEmptyText {
    fn name(&self) -> &'static str {
        "removeEmptyText"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        // Bottom-up so an emptied <tspan> lets its parent <text> go in the
        // same pass.
        let mut ids = doc.traverse();
        ids.reverse();

        for id in ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind
                && matches!(elem.name.as_str(), "text" | "tspan" | "textPath")
                && doc.children(id).next().is_none()
            {
                // SVGO parity: only an element with *no* children at all is
                // empty. Whitespace-only text or CDATA is character data that
                // may separate words (`read<tspan> </tspan>more`), so it is
                // never treated as "no content".
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

    #[test]
    fn keeps_text_with_cdata_content() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text><![CDATA[a < b]]></text></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(output.contains("<text>"), "{output}");
        assert!(
            output.contains("a &lt; b") || output.contains("<![CDATA[a < b]]>"),
            "{output}"
        );
    }

    #[test]
    fn keeps_tspan_with_cdata_content() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text><tspan><![CDATA[hi]]></tspan></text></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Unchanged);
        assert!(serialize(&doc).contains("<tspan>"));
    }

    #[test]
    fn keeps_whitespace_only_cdata_text() {
        // SVGO removeEmptyText only removes elements with no children.
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text><![CDATA[   ]]></text><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Unchanged);
        assert!(serialize(&doc).contains("<text"));
    }

    #[test]
    fn keeps_whitespace_only_tspan_between_words() {
        // The space inside the <tspan> separates "read" and "more".
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text>read<tspan> </tspan>more</text></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Unchanged);
        assert_eq!(
            serialize(&doc),
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text>read<tspan> </tspan>more</text></svg>"#
        );

        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text><tspan>read</tspan><tspan> </tspan><tspan>more</tspan></text></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Unchanged);
        assert!(serialize(&doc).contains("<tspan> </tspan>"));

        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text><![CDATA[ ]]><tspan> </tspan></text></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Unchanged);

        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text>   </text></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn removes_text_emptied_by_empty_tspan_in_one_pass() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text><tspan></tspan></text><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyText.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<text"), "{output}");
        assert!(!output.contains("<tspan"), "{output}");
    }
}
