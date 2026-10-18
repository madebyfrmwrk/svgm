use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Removes comments, except those starting with `!` (`<!--! ... -->`), the
/// convention for license and copyright notices (SVGO `removeComments`
/// default `preservePatterns: [/^!/]`).
pub struct RemoveComments;

impl Pass for RemoveComments {
    fn name(&self) -> &'static str {
        "removeComments"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        for id in ids {
            if matches!(doc.node(id).kind, NodeKind::Comment(ref text) if !text.starts_with('!')) {
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
    fn svgo_fixture_02_keeps_license_comments() {
        let input = "<!--!Icon Font v1 by @iconfont - Copyright 2023 Icon Font CIC.--><svg xmlns=\"http://www.w3.org/2000/svg\"><!-- plain --><!--!inner license--><rect/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveComments.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert_eq!(
            output,
            "<!--!Icon Font v1 by @iconfont - Copyright 2023 Icon Font CIC.--><svg xmlns=\"http://www.w3.org/2000/svg\"><!--!inner license--><rect/></svg>"
        );

        // Only a leading `!` counts; ` !` or `!` later in the text does not.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><!-- !x --><!--a!--><rect/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveComments.run(&mut doc), PassResult::Changed);
        assert!(!serialize(&doc).contains("<!--"));
    }

    #[test]
    fn no_change_without_comments() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveComments.run(&mut doc), PassResult::Unchanged);
    }
}
