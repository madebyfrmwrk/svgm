use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

pub struct SortAttrs;

impl Pass for SortAttrs {
    fn name(&self) -> &'static str {
        "sortAttrs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                let attrs = &mut elem.attributes;
                if attrs.len() < 2 {
                    continue;
                }
                let already_sorted = attrs
                    .windows(2)
                    .all(|w| w[0].qualified_name() <= w[1].qualified_name());
                if !already_sorted {
                    attrs.sort_by_key(|a| a.qualified_name());
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

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = SortAttrs.run(&mut doc);
        (result, serialize(&doc))
    }

    #[test]
    fn sorts_unsorted_attributes() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"5\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // fill < height < width
        assert!(output.contains("fill=\"red\" height=\"5\" width=\"10\""));
    }

    #[test]
    fn already_sorted_unchanged() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect fill=\"red\" height=\"5\" width=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn handles_prefixed_attributes() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><use xlink:href=\"#a\" y=\"0\" x=\"0\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // x < xlink:href < y
        assert!(output.contains("x=\"0\" xlink:href=\"#a\" y=\"0\""));
    }

    #[test]
    fn single_attribute_unchanged() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_attributes_unchanged() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g></g></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }
}
