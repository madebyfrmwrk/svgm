use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Removes deprecated SVG attributes per the SVG spec.
pub struct RemoveDeprecatedAttrs;

/// Deprecated attributes that are always safe to remove.
const DEPRECATED_ATTRS: &[&str] = &[
    "xml:base",
    "xml:space",
    "requiredFeatures",
    "attributeType",
    "clip",
    "color-profile",
    "enable-background",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
];

impl Pass for RemoveDeprecatedAttrs {
    fn name(&self) -> &'static str {
        "removeDeprecatedAttrs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            let elem = match &node.kind {
                NodeKind::Element(e) => e,
                _ => continue,
            };

            // Check if xml:lang should be removed (only if lang attr also exists)
            let has_lang = elem.attr("lang").is_some();
            let has_xml_lang = elem
                .attributes
                .iter()
                .any(|a| a.prefix.as_deref() == Some("xml") && a.name == "lang");
            let remove_xml_lang = has_lang && has_xml_lang;

            let should_remove: Vec<usize> = elem
                .attributes
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    let qname = a.qualified_name();
                    if DEPRECATED_ATTRS.contains(&qname.as_str()) {
                        // Don't remove "clip" if it's actually "clip-path" or has a prefix
                        if a.name == "clip" && a.prefix.is_some() {
                            return false;
                        }
                        return true;
                    }
                    if remove_xml_lang && a.prefix.as_deref() == Some("xml") && a.name == "lang" {
                        return true;
                    }
                    false
                })
                .map(|(i, _)| i)
                .collect();

            if !should_remove.is_empty() {
                let node = doc.node_mut(id);
                if let NodeKind::Element(ref mut elem) = node.kind {
                    for &i in should_remove.iter().rev() {
                        elem.attributes.remove(i);
                    }
                }
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
    fn removes_xml_space() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xml:space="preserve"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDeprecatedAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("xml:space"));
    }

    #[test]
    fn removes_enable_background() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" enable-background="new 0 0 100 100"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDeprecatedAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("enable-background"));
    }

    #[test]
    fn removes_xml_lang_when_lang_exists() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg" xml:lang="en" lang="en"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDeprecatedAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("xml:lang"));
        assert!(output.contains("lang=\"en\""));
    }

    #[test]
    fn keeps_xml_lang_when_no_lang() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xml:lang="en"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDeprecatedAttrs.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn does_not_remove_clip_path() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect clip-path="url(#c)"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDeprecatedAttrs.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn removes_required_features() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect requiredFeatures="foo"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDeprecatedAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("requiredFeatures"));
    }

    #[test]
    fn unchanged_when_no_deprecated() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="red"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveDeprecatedAttrs.run(&mut doc), PassResult::Unchanged);
    }
}
