use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

pub struct RemoveUnknownsAndDefaults;

/// SVG attributes that can be safely removed when they match their default values.
/// Conservative list — only includes values that are unambiguously default per SVG spec.
const DEFAULT_ATTRS: &[(&str, &str)] = &[
    // Presentation defaults
    ("fill", "black"),
    ("fill", "#000"),
    ("fill", "#000000"),
    ("fill-opacity", "1"),
    ("fill-rule", "nonzero"),
    ("stroke", "none"),
    ("stroke-opacity", "1"),
    ("stroke-width", "1"),
    ("stroke-linecap", "butt"),
    ("stroke-linejoin", "miter"),
    ("stroke-miterlimit", "4"),
    ("stroke-dasharray", "none"),
    ("stroke-dashoffset", "0"),
    ("opacity", "1"),
    ("visibility", "visible"),
    ("display", "inline"),
    ("overflow", "visible"),
    ("clip-rule", "nonzero"),
    ("color-interpolation", "sRGB"),
    ("color-interpolation-filters", "linearRGB"),
    ("direction", "ltr"),
    ("font-style", "normal"),
    ("font-variant", "normal"),
    ("font-weight", "normal"),
    ("font-stretch", "normal"),
    ("text-anchor", "start"),
    ("text-decoration", "none"),
    ("dominant-baseline", "auto"),
    ("alignment-baseline", "auto"),
    ("baseline-shift", "0"),
    ("writing-mode", "lr-tb"),
    ("letter-spacing", "normal"),
    ("word-spacing", "normal"),
    ("filter", "none"),
    ("flood-opacity", "1"),
    ("lighting-color", "white"),
    ("lighting-color", "#fff"),
    ("lighting-color", "#ffffff"),
    ("pointer-events", "visiblePainted"),
    ("image-rendering", "auto"),
    ("shape-rendering", "auto"),
    ("text-rendering", "auto"),
    ("color-profile", "auto"),
    ("cursor", "auto"),
    ("enable-background", "accumulate"),
    ("stop-color", "black"),
    ("stop-color", "#000"),
    ("stop-color", "#000000"),
    ("stop-opacity", "1"),
    // SVG filter defaults
    ("mode", "normal"),
    ("color-interpolation-filters", "linearRGB"),
    ("flood-color", "black"),
    ("flood-color", "#000"),
    ("flood-color", "#000000"),
    // Gradient defaults
    ("spreadMethod", "pad"),
    ("gradientUnits", "objectBoundingBox"),
    // Misc defaults
    ("clip-path", "none"),
    ("mask", "none"),
    ("unicode-bidi", "normal"),
    ("baseline-shift", "baseline"),
    ("white-space", "normal"),
    ("text-overflow", "clip"),
];

/// Elements where `fill="black"` is NOT the default and should not be removed.
/// For these, the default fill behavior differs or fill is inherited.
const SKIP_FILL_REMOVAL: &[&str] = &[
    // On the root <svg>, fill is inherited to all children, so removing it changes behavior.
    "svg",
];

impl Pass for RemoveUnknownsAndDefaults {
    fn name(&self) -> &'static str {
        "removeUnknownsAndDefaults"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                let elem_name = elem.name.clone();
                let before = elem.attributes.len();

                elem.attributes.retain(|attr| {
                    // Only check unprefixed attributes
                    if attr.prefix.is_some() {
                        return true;
                    }

                    // Remove obsolete version attribute from <svg> (SVG2)
                    if attr.name == "version" && elem_name == "svg" {
                        return false;
                    }

                    // Remove x="0" and y="0" from <svg> (spec defaults)
                    if (attr.name == "x" || attr.name == "y")
                        && attr.value == "0"
                        && elem_name == "svg"
                    {
                        return false;
                    }

                    // Check fill removal exceptions
                    if attr.name == "fill" && SKIP_FILL_REMOVAL.contains(&elem_name.as_str()) {
                        return true;
                    }

                    // Check if this attr=value pair matches a known default
                    let is_default = DEFAULT_ATTRS
                        .iter()
                        .any(|&(name, val)| attr.name == name && attr.value == val);

                    !is_default
                });

                if elem.attributes.len() != before {
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

    #[test]
    fn removes_default_fill_black() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="black" width="10"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUnknownsAndDefaults.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(
            !output.contains("fill"),
            "default fill=black should be removed: {output}"
        );
    }

    #[test]
    fn removes_default_opacity() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect opacity="1" fill-opacity="1" stroke-opacity="1"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUnknownsAndDefaults.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(
            !output.contains("opacity"),
            "default opacities should be removed: {output}"
        );
    }

    #[test]
    fn removes_default_stroke_none() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect stroke="none"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUnknownsAndDefaults.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(
            !output.contains("stroke"),
            "default stroke=none should be removed: {output}"
        );
    }

    #[test]
    fn keeps_non_default_fill() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="red"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveUnknownsAndDefaults.run(&mut doc),
            PassResult::Unchanged
        );
    }

    #[test]
    fn keeps_fill_on_svg_element() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" fill="black"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        // fill="black" on <svg> should be kept — it's inherited by children
        assert_eq!(
            RemoveUnknownsAndDefaults.run(&mut doc),
            PassResult::Unchanged
        );
    }

    #[test]
    fn removes_version_from_svg() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUnknownsAndDefaults.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("version"));
    }

    #[test]
    fn removes_hex_default_fill() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path fill=\"#000000\" d=\"M0 0\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveUnknownsAndDefaults.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(
            !output.contains("fill"),
            "fill=#000000 should be removed: {output}"
        );
    }
}
