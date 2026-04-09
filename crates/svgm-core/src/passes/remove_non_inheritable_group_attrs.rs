use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Removes non-inheritable presentation attributes from `<g>` elements.
/// These attributes have no effect on groups since they don't cascade to children.
pub struct RemoveNonInheritableGroupAttrs;

/// Presentation attributes that are inheritable (valid and meaningful on `<g>`).
const INHERITABLE_ATTRS: &[&str] = &[
    "clip-rule",
    "color",
    "color-interpolation",
    "color-interpolation-filters",
    "cursor",
    "direction",
    "dominant-baseline",
    "fill",
    "fill-opacity",
    "fill-rule",
    "font",
    "font-family",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-variant",
    "font-weight",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "image-rendering",
    "letter-spacing",
    "marker",
    "marker-start",
    "marker-mid",
    "marker-end",
    "paint-order",
    "pointer-events",
    "shape-rendering",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "text-anchor",
    "text-rendering",
    "visibility",
    "word-spacing",
    "writing-mode",
];

/// Non-inheritable presentation attributes that are still valid on `<g>`.
const GROUP_VALID_NON_INHERITABLE: &[&str] = &[
    "clip-path",
    "display",
    "filter",
    "mask",
    "opacity",
    "overflow",
    "text-decoration",
    "transform",
    "unicode-bidi",
];

/// All presentation attributes (superset). Anything in this list that is NOT
/// in INHERITABLE_ATTRS or GROUP_VALID_NON_INHERITABLE should be removed from `<g>`.
const PRESENTATION_ATTRS: &[&str] = &[
    "alignment-baseline",
    "baseline-shift",
    "clip",
    "clip-path",
    "clip-rule",
    "color",
    "color-interpolation",
    "color-interpolation-filters",
    "cursor",
    "direction",
    "display",
    "dominant-baseline",
    "enable-background",
    "fill",
    "fill-opacity",
    "fill-rule",
    "filter",
    "flood-color",
    "flood-opacity",
    "font",
    "font-family",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-variant",
    "font-weight",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "image-rendering",
    "letter-spacing",
    "lighting-color",
    "marker",
    "marker-start",
    "marker-mid",
    "marker-end",
    "mask",
    "opacity",
    "overflow",
    "paint-order",
    "pointer-events",
    "shape-rendering",
    "stop-color",
    "stop-opacity",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "text-anchor",
    "text-decoration",
    "text-rendering",
    "transform",
    "unicode-bidi",
    "visibility",
    "word-spacing",
    "writing-mode",
];

impl Pass for RemoveNonInheritableGroupAttrs {
    fn name(&self) -> &'static str {
        "removeNonInheritableGroupAttrs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            let elem = match &node.kind {
                NodeKind::Element(e) if e.name == "g" && e.prefix.is_none() => e,
                _ => continue,
            };

            let to_remove: Vec<usize> = elem
                .attributes
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    // Only consider unprefixed presentation attributes
                    if a.prefix.is_some() {
                        return false;
                    }
                    let name = a.name.as_str();
                    // Is it a presentation attr?
                    if !PRESENTATION_ATTRS.contains(&name) {
                        return false;
                    }
                    // Is it inheritable or group-valid? Then keep it.
                    if INHERITABLE_ATTRS.contains(&name) {
                        return false;
                    }
                    if GROUP_VALID_NON_INHERITABLE.contains(&name) {
                        return false;
                    }
                    // It's a non-inheritable presentation attr on a group — remove it
                    true
                })
                .map(|(i, _)| i)
                .collect();

            if !to_remove.is_empty() {
                let node = doc.node_mut(id);
                if let NodeKind::Element(ref mut elem) = node.kind {
                    for &i in to_remove.iter().rev() {
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
    fn removes_alignment_baseline_from_group() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g alignment-baseline="middle"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveNonInheritableGroupAttrs.run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(!output.contains("alignment-baseline"));
    }

    #[test]
    fn removes_stop_color_from_group() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g stop-color="red"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveNonInheritableGroupAttrs.run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(!output.contains("stop-color"));
    }

    #[test]
    fn keeps_fill_on_group() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveNonInheritableGroupAttrs.run(&mut doc),
            PassResult::Unchanged
        );
        let output = serialize(&doc);
        assert!(output.contains("fill=\"red\""));
    }

    #[test]
    fn keeps_transform_on_group() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(1,2)"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveNonInheritableGroupAttrs.run(&mut doc),
            PassResult::Unchanged
        );
    }

    #[test]
    fn keeps_clip_path_on_group() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g clip-path="url(#c)"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveNonInheritableGroupAttrs.run(&mut doc),
            PassResult::Unchanged
        );
    }

    #[test]
    fn does_not_touch_non_group_elements() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect alignment-baseline="middle"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveNonInheritableGroupAttrs.run(&mut doc),
            PassResult::Unchanged
        );
    }
}
