use std::collections::HashMap;

use super::{INHERITABLE_PROPS, Pass, PassResult};
use crate::ast::{Document, NodeId, NodeKind};

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

/// Compute the inherited value for each inheritable attribute by walking the
/// ancestor chain. Returns a map of attr_name → nearest ancestor's value.
/// Matches SVGO's computeStyle ancestor-walking logic.
fn compute_inherited_values(doc: &Document, id: NodeId) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut current = doc.node(id).parent;
    while let Some(parent_id) = current {
        if let NodeKind::Element(ref elem) = doc.node(parent_id).kind {
            for attr in &elem.attributes {
                if attr.prefix.is_none()
                    && INHERITABLE_PROPS.contains(&attr.name.as_str())
                    && !values.contains_key(&attr.name)
                {
                    // Nearest ancestor wins — don't overwrite
                    values.insert(attr.name.clone(), attr.value.clone());
                }
            }
        }
        current = doc.node(parent_id).parent;
    }
    values
}

impl Pass for RemoveUnknownsAndDefaults {
    fn name(&self) -> &'static str {
        "removeUnknownsAndDefaults"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            // Phase 1: compute inherited attribute values (immutable borrow)
            let inherited = compute_inherited_values(doc, id);

            // Phase 2: mutate attributes (mutable borrow)
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                let elem_name = elem.name.clone();
                let has_id = elem
                    .attributes
                    .iter()
                    .any(|a| a.prefix.is_none() && a.name == "id");
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

                    // SVGO parity: skip default removal and useless override removal
                    // for elements with id (CSS/JS may target them)
                    if has_id {
                        return true;
                    }

                    let is_default = DEFAULT_ATTRS
                        .iter()
                        .any(|&(name, val)| attr.name == name && attr.value == val);
                    let is_inheritable = INHERITABLE_PROPS.contains(&attr.name.as_str());

                    // Check A (SVGO defaultAttrs): remove known defaults
                    if is_default {
                        if is_inheritable {
                            // Only remove if NO ancestor has this property at all.
                            // If any ancestor sets it, keep — removing would change
                            // the inherited value. (Falls through to Check B.)
                            if !inherited.contains_key(&attr.name) {
                                return false;
                            }
                        } else {
                            // Non-inheritable default → always safe to remove
                            return false;
                        }
                    }

                    // Check B (SVGO uselessOverrides): remove attrs that duplicate
                    // the inherited value (redundant override)
                    if is_inheritable && inherited.get(&attr.name).is_some_and(|v| *v == attr.value)
                    {
                        return false;
                    }

                    true
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

    #[test]
    fn keeps_fill_black_when_parent_has_fill_none() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"none\"><path fill=\"black\" d=\"M0 0\"/></svg>";
        let mut doc = parse(input).unwrap();
        RemoveUnknownsAndDefaults.run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("fill=\"black\""),
            "fill=black must be kept when parent has fill=none: {output}"
        );
    }

    #[test]
    fn keeps_fill_black_with_grandparent_override() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"red\"><g><path fill=\"black\" d=\"M0 0\"/></g></svg>";
        let mut doc = parse(input).unwrap();
        RemoveUnknownsAndDefaults.run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("fill=\"black\""),
            "fill=black must be kept when grandparent has fill=red: {output}"
        );
    }

    #[test]
    fn keeps_stroke_none_when_parent_has_stroke() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" stroke=\"red\"><path stroke=\"none\" d=\"M0 0\"/></svg>";
        let mut doc = parse(input).unwrap();
        RemoveUnknownsAndDefaults.run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("stroke=\"none\""),
            "stroke=none must be kept when parent has stroke=red: {output}"
        );
    }

    #[test]
    fn removes_useless_override_same_value() {
        // Parent has fill="red", child has fill="red" → useless override, remove
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"red\"><path fill=\"red\" d=\"M0 0\"/></svg>";
        let mut doc = parse(input).unwrap();
        RemoveUnknownsAndDefaults.run(&mut doc);
        let output = serialize(&doc);
        // Child's fill="red" should be removed (matches inherited value)
        assert_eq!(
            output.matches("fill=\"red\"").count(),
            1,
            "child fill=red should be removed as useless override: {output}"
        );
    }

    #[test]
    fn keeps_default_when_parent_has_same_attr_different_value() {
        // Parent has fill="black" → computedParentStyle['fill'] is non-null →
        // Check A skips. Check B: "black" != "#000" → keeps. Matches SVGO.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"black\"><path fill=\"#000\" d=\"M0 0\"/></svg>";
        let mut doc = parse(input).unwrap();
        RemoveUnknownsAndDefaults.run(&mut doc);
        let output = serialize(&doc);
        // SVGO keeps this because the string values differ, even though semantically both are black.
        // (convert_colors normalizes these before this pass runs in practice.)
        assert!(
            output.contains("fill=\"#000\""),
            "fill=#000 should be kept when parent has fill=black (different string): {output}"
        );
    }

    #[test]
    fn removes_useless_override_exact_match() {
        // Parent has fill="black", child has fill="black" → exact match → remove
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" fill=\"black\"><path fill=\"black\" d=\"M0 0\"/></svg>";
        let mut doc = parse(input).unwrap();
        RemoveUnknownsAndDefaults.run(&mut doc);
        let output = serialize(&doc);
        // svg keeps fill="black" (SKIP_FILL_REMOVAL), path's is removed (useless override)
        assert_eq!(
            output.matches("fill=\"black\"").count(),
            1,
            "child fill=black should be removed as useless override: {output}"
        );
    }

    #[test]
    fn keeps_defaults_on_element_with_id() {
        // SVGO skips default removal for elements with id
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path id=\"a\" fill=\"black\" d=\"M0 0\"/></svg>";
        let mut doc = parse(input).unwrap();
        RemoveUnknownsAndDefaults.run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("fill=\"black\""),
            "fill=black should be kept on element with id: {output}"
        );
    }
}
