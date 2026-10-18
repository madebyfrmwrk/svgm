use super::{Pass, PassResult, attributes_in_stylesheets};
use crate::ast::{Document, NodeKind};

/// Removes deprecated SVG attributes (SVGO `removeDeprecatedAttrs`).
///
/// By default only removals that cannot change rendering are made:
/// `xml:lang` when a `lang` attribute is also present, and
/// `xml:space="default"` (its initial value). With `remove_unsafe` set,
/// SVGO's "unsafe" set (`removeUnsafe: true`) is removed as well; those
/// attributes still affect rendering in some user agents (`xml:space=
/// "preserve"` on text, `requiredFeatures` inside `<switch>`, ...).
///
/// `enable-background` is owned, filter-aware, by cleanupEnableBackground and
/// is never touched here; `version` on `<svg>` is removed by
/// removeUnknownsAndDefaults. Attributes referenced by an attribute selector
/// in a `<style>` sheet are always kept (SVGO #2144).
#[derive(Default)]
pub struct RemoveDeprecatedAttrs {
    pub remove_unsafe: bool,
}

/// SVGO `attrsGroupsDeprecated[*].unsafe` for the core, conditionalProcessing
/// and presentation groups, which nearly every SVG element carries.
const UNSAFE_ATTRS: &[&str] = &[
    "xml:base",
    "xml:lang",
    "xml:space",
    "requiredFeatures",
    "clip",
    "color-profile",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "kerning",
];

/// Elements carrying the animationAttributeTarget group (`attributeType`).
const ANIMATION_ELEMENTS: &[&str] = &["animate", "animateColor", "animateTransform", "set"];

/// SVGO `elems[name].deprecated.unsafe`.
const UNSAFE_ELEMENT_ATTRS: &[(&str, &[&str])] = &[
    ("color-profile", &["name"]),
    ("filter", &["filterRes"]),
    (
        "font",
        &[
            "horiz-origin-x",
            "horiz-origin-y",
            "vert-adv-y",
            "vert-origin-x",
            "vert-origin-y",
        ],
    ),
    (
        "font-face",
        &[
            "accent-height",
            "alphabetic",
            "ascent",
            "bbox",
            "cap-height",
            "descent",
            "hanging",
            "ideographic",
            "mathematical",
            "panose-1",
            "slope",
            "stemh",
            "stemv",
            "unicode-range",
            "units-per-em",
            "v-alphabetic",
            "v-hanging",
            "v-ideographic",
            "v-mathematical",
            "widths",
            "x-height",
        ],
    ),
    ("font-face-format", &["string"]),
    ("font-face-name", &["name"]),
    (
        "glyph",
        &[
            "arabic-form",
            "glyph-name",
            "horiz-adv-x",
            "orientation",
            "unicode",
            "vert-adv-y",
            "vert-origin-x",
            "vert-origin-y",
        ],
    ),
    (
        "glyphRef",
        &[
            "horiz-adv-x",
            "vert-adv-y",
            "vert-origin-x",
            "vert-origin-y",
        ],
    ),
    ("hkern", &["g1", "g2", "k", "u1", "u2"]),
    (
        "missing-glyph",
        &[
            "horiz-adv-x",
            "vert-adv-y",
            "vert-origin-x",
            "vert-origin-y",
        ],
    ),
    (
        "svg",
        &[
            "baseProfile",
            "contentScriptType",
            "contentStyleType",
            "zoomAndPan",
        ],
    ),
    ("view", &["viewTarget", "zoomAndPan"]),
    ("vkern", &["g1", "g2", "k", "u1", "u2"]),
];

impl RemoveDeprecatedAttrs {
    fn is_unsafe_for(&self, elem_name: &str, qname: &str) -> bool {
        if !self.remove_unsafe {
            return false;
        }
        if UNSAFE_ATTRS.contains(&qname) {
            return true;
        }
        if qname == "attributeType" && ANIMATION_ELEMENTS.contains(&elem_name) {
            return true;
        }
        UNSAFE_ELEMENT_ATTRS
            .iter()
            .any(|&(name, attrs)| name == elem_name && attrs.contains(&qname))
    }
}

impl Pass for RemoveDeprecatedAttrs {
    fn name(&self) -> &'static str {
        "removeDeprecatedAttrs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        // Attribute names used in `[attr]` selectors: removing those would
        // un-match the rule, so they are never removed.
        let referenced = attributes_in_stylesheets(doc);

        for id in ids {
            let node = doc.node(id);
            let elem = match &node.kind {
                NodeKind::Element(e) if e.prefix.is_none() => e,
                _ => continue,
            };

            // Removing deprecated xml:lang is safe when `lang` exists too.
            let has_lang = elem.attr("lang").is_some();
            let has_id = elem.attr("id").is_some();

            let should_remove: Vec<usize> = elem
                .attributes
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    let qname = a.qualified_name();
                    if referenced.contains(&qname) {
                        return false;
                    }
                    if qname == "xml:lang" && has_lang {
                        return true;
                    }
                    // Initial value (SVGO attrsGroupsDefaults.core); like
                    // other defaults, kept on elements with an id.
                    if qname == "xml:space" && a.value == "default" && !has_id {
                        return true;
                    }
                    self.is_unsafe_for(&elem.name, &qname)
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

    fn run(input: &str, remove_unsafe: bool) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = RemoveDeprecatedAttrs { remove_unsafe }.run(&mut doc);
        (result, serialize(&doc))
    }

    #[test]
    fn keeps_xml_space_preserve_by_default() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xml:space="preserve"><text> a </text></svg>"#;
        let (result, output) = run(input, false);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("xml:space=\"preserve\""), "{output}");

        let (result, output) = run(input, true);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("xml:space"), "{output}");
    }

    #[test]
    fn removes_xml_space_default() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text xml:space="default">a</text></svg>"#;
        let (result, output) = run(input, false);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("xml:space"), "{output}");

        // Kept on elements with an id, like other defaults.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text id="t" xml:space="default">a</text></svg>"#;
        let (result, _) = run(input, false);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn never_touches_enable_background() {
        // Owned by cleanupEnableBackground, which knows about filters.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" enable-background="new 0 0 100 100"><rect/></svg>"#;
        for remove_unsafe in [false, true] {
            let (result, output) = run(input, remove_unsafe);
            assert_eq!(result, PassResult::Unchanged);
            assert!(output.contains("enable-background"), "{output}");
        }
    }

    #[test]
    fn svgo_fixture_05_removes_xml_lang_when_lang_exists() {
        let input = r#"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg"><text xml:lang="en-CA" lang="en-US">English text</text></svg>"#;
        let (result, output) = run(input, false);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("xml:lang"));
        assert!(output.contains("lang=\"en-US\""));
    }

    #[test]
    fn svgo_fixture_06_keeps_xml_lang_when_no_lang() {
        let input = r#"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg"><text xml:lang="en-US">English text</text></svg>"#;
        let (result, output) = run(input, false);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("xml:lang=\"en-US\""));
    }

    #[test]
    fn svgo_fixture_07_unsafe_removes_xml_lang_without_lang() {
        let input = r#"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg"><text xml:lang="en-US">English text</text></svg>"#;
        let (result, output) = run(input, true);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<text>English text</text>"), "{output}");
    }

    #[test]
    fn svgo_fixture_02_03_view_target_is_unsafe() {
        let input = r#"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg"><view id="one" viewBox="0 0 100 100" viewTarget=""/></svg>"#;
        let (result, output) = run(input, false);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("viewTarget=\"\""), "{output}");

        let (result, output) = run(input, true);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("viewTarget"), "{output}");
        assert!(output.contains("viewBox=\"0 0 100 100\"/>"), "{output}");
    }

    #[test]
    fn svgo_fixture_08_attribute_selector_keeps_deprecated_attrs() {
        let input = r#"<svg version="1.1" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg"><style><![CDATA[svg[version="1.1"]{fill:blue;}rect[clip]{fill:green;}]]></style><rect x="10" y="10" width="80" height="80" clip="1"/><rect clip="2" kerning="1"/></svg>"#;
        let (result, output) = run(input, true);
        // `clip` is referenced by a selector and stays on both rects;
        // `kerning` is not and goes.
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("clip=\"").count(), 2, "{output}");
        assert!(!output.contains("kerning"), "{output}");

        // Escaped prefixed names in selectors count too.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><style>text[xml\:lang]{fill:red}</style><text xml:lang="en" lang="en">a</text></svg>"#;
        let (result, output) = run(input, false);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("xml:lang=\"en\""), "{output}");
    }

    #[test]
    fn required_features_is_unsafe() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><switch><rect requiredFeatures="http://www.w3.org/TR/SVG11/feature#Shape"/><text>fallback</text></switch></svg>"#;
        let (result, output) = run(input, false);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("requiredFeatures"), "{output}");

        let (result, output) = run(input, true);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("requiredFeatures"), "{output}");
    }

    #[test]
    fn unsafe_group_and_element_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" baseProfile="full" zoomAndPan="magnify" xml:base="/a/"><filter id="f" filterRes="2"/><rect clip="rect(0 0 0 0)" color-profile="auto" glyph-orientation-vertical="auto"/><animate attributeType="XML" attributeName="x"/><rect attributeType="XML" zoomAndPan="magnify"/></svg>"#;
        let (result, output) = run(input, true);
        assert_eq!(result, PassResult::Changed);
        for gone in [
            "baseProfile",
            "zoomAndPan=\"magnify\" xml:base",
            "xml:base",
            "filterRes",
            "clip=",
            "color-profile",
            "glyph-orientation",
            "attributeType=\"XML\" attributeName",
        ] {
            assert!(!output.contains(gone), "{gone} should be removed: {output}");
        }
        // Element-specific entries do not leak onto other elements.
        assert!(
            output.contains("<rect attributeType=\"XML\" zoomAndPan=\"magnify\"/>"),
            "{output}"
        );
        assert!(output.contains("attributeName=\"x\""), "{output}");
    }

    #[test]
    fn does_not_remove_clip_path() {
        let input =
            r##"<svg xmlns="http://www.w3.org/2000/svg"><rect clip-path="url(#c)"/></svg>"##;
        for remove_unsafe in [false, true] {
            let (result, _) = run(input, remove_unsafe);
            assert_eq!(result, PassResult::Unchanged);
        }
    }

    #[test]
    fn unchanged_when_no_deprecated() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="red"/></svg>"#;
        for remove_unsafe in [false, true] {
            let (result, _) = run(input, remove_unsafe);
            assert_eq!(result, PassResult::Unchanged);
        }
    }
}
