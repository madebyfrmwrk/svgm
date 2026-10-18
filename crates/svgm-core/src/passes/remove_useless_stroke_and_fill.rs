use super::{Computed, Pass, PassResult, computed_style_with, has_scripts};
use crate::ast::{Attribute, Document, NodeId, NodeKind};

/// Removes useless stroke and fill attributes (SVGO `removeUselessStrokeAndFill`).
///
/// - When the computed stroke is invisible (absent, `none`, `stroke-opacity:0`
///   or `stroke-width:0`) every `stroke*` attribute is dropped; `stroke="none"`
///   is re-added when an ancestor would otherwise paint a stroke. A
///   `marker-end` keeps the stroke attributes unless `stroke-width` is 0, since
///   the marker scales with the stroke width.
/// - When the computed fill is invisible (`none` or `fill-opacity:0`) every
///   `fill-*` attribute is dropped and `fill` is normalised to `none`.
///
/// The computed value includes the element's own `style=""` and inherited
/// values from ancestors. Elements with an `id`, and everything below them,
/// are skipped (they may be `<use>` targets styled by the referrer). A
/// `<style>` element or script deoptimises the whole pass.
pub struct RemoveUselessStrokeAndFill;

const SHAPE_ELEMENTS: &[&str] = &[
    "circle", "ellipse", "line", "path", "polygon", "polyline", "rect",
];

/// A computed value reduced to what this pass can reason about.
enum Resolved {
    /// Nothing sets the property.
    Unset,
    /// A statically known, non-empty value.
    Value(String),
    /// Dynamic, or an empty declaration like `style="stroke:"`.
    Unknown,
}

fn resolve(doc: &Document, id: NodeId, prop: &str, dynamic: bool) -> Resolved {
    match computed_style_with(doc, id, prop, dynamic) {
        Computed::Unset => Resolved::Unset,
        Computed::Static(v) if v.trim().is_empty() => Resolved::Unknown,
        Computed::Static(v) => Resolved::Value(v.trim().to_string()),
        Computed::Dynamic => Resolved::Unknown,
    }
}

impl Resolved {
    fn is(&self, value: &str) -> bool {
        matches!(self, Resolved::Value(v) if v == value)
    }
}

fn attrs_equal(a: &[Attribute], b: &[Attribute]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.prefix == y.prefix && x.name == y.name && x.value == y.value)
}

/// True when `id` or any ancestor element carries an `id` attribute
/// (SVGO's `visitSkip` on `id`).
fn in_id_subtree(doc: &Document, id: NodeId) -> bool {
    let mut current = Some(id);
    while let Some(nid) = current {
        if let NodeKind::Element(ref e) = doc.node(nid).kind
            && e.attr("id").is_some()
        {
            return true;
        }
        current = doc.node(nid).parent;
    }
    false
}

impl Pass for RemoveUselessStrokeAndFill {
    fn name(&self) -> &'static str {
        "removeUselessStrokeAndFill"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let ids = doc.traverse();

        // Bail if a <style> element or script exists — CSS/JS may affect
        // computed styles (SVGO: style and script elements deoptimize).
        let has_style = ids.iter().any(|&id| {
            matches!(&doc.node(id).kind, NodeKind::Element(e) if e.prefix.is_none() && e.name == "style")
        });
        if has_style || has_scripts(doc) {
            return PassResult::Unchanged;
        }

        let mut changed = false;

        for &id in &ids {
            let elem = match &doc.node(id).kind {
                NodeKind::Element(e) if e.prefix.is_none() => e,
                _ => continue,
            };

            // Only process shape elements
            if !SHAPE_ELEMENTS.contains(&elem.name.as_str()) {
                continue;
            }

            // id attribute deoptimizes the whole subtree
            if in_id_subtree(doc, id) {
                continue;
            }

            let parent = doc.node(id).parent;
            let stroke = resolve(doc, id, "stroke", false);
            let stroke_opacity = resolve(doc, id, "stroke-opacity", false);
            let stroke_width = resolve(doc, id, "stroke-width", false);
            let marker_end = resolve(doc, id, "marker-end", false);
            let fill = resolve(doc, id, "fill", false);
            let fill_opacity = resolve(doc, id, "fill-opacity", false);
            let parent_stroke = match parent {
                Some(p) if matches!(doc.node(p).kind, NodeKind::Element(_)) => {
                    resolve(doc, p, "stroke", false)
                }
                _ => Resolved::Unset,
            };

            let mut attrs = elem.attributes.clone();
            let before = attrs.clone();

            // remove stroke* (an unknown stroke value, e.g. `style="stroke:"`,
            // cannot be proven invisible nor safely replaced by stroke="none")
            let stroke_invisible = !matches!(stroke, Resolved::Unknown)
                && (matches!(stroke, Resolved::Unset)
                    || stroke.is("none")
                    || stroke_opacity.is("0")
                    || stroke_width.is("0"));
            // stroke-width may affect the size of marker-end; the marker is
            // not visible when stroke-width is 0.
            if stroke_invisible && (stroke_width.is("0") || matches!(marker_end, Resolved::Unset)) {
                attrs.retain(|a| a.prefix.is_some() || !a.name.starts_with("stroke"));
                // set explicit none to not inherit from parent
                if let Resolved::Value(v) = &parent_stroke
                    && v != "none"
                {
                    attrs.push(Attribute {
                        prefix: None,
                        name: "stroke".to_string(),
                        value: "none".to_string(),
                    });
                }
            }

            // remove fill* (same caveat for an unknown fill value)
            if !matches!(fill, Resolved::Unknown) && (fill.is("none") || fill_opacity.is("0")) {
                attrs.retain(|a| a.prefix.is_some() || !a.name.starts_with("fill-"));
                let needs_none = match &fill {
                    Resolved::Unset => true,
                    Resolved::Value(v) => v != "none",
                    Resolved::Unknown => false,
                };
                if needs_none {
                    if let Some(a) = attrs
                        .iter_mut()
                        .find(|a| a.prefix.is_none() && a.name == "fill")
                    {
                        a.value = "none".to_string();
                    } else {
                        attrs.push(Attribute {
                            prefix: None,
                            name: "fill".to_string(),
                            value: "none".to_string(),
                        });
                    }
                }
            }

            if !attrs_equal(&attrs, &before)
                && let NodeKind::Element(ref mut elem) = doc.node_mut(id).kind
            {
                elem.attributes = attrs;
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

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = RemoveUselessStrokeAndFill.run(&mut doc);
        (result, serialize(&doc))
    }

    #[test]
    fn removes_all_stroke_attrs_when_stroke_none() {
        // SVGO drops stroke="none" too: nothing above paints a stroke.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" stroke="none" stroke-dasharray="5" stroke-linecap="round"/></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>"#
        );
    }

    #[test]
    fn removes_stroke_attrs_when_stroke_width_zero() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10" stroke="red" stroke-width="0" stroke-dasharray="5"/></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>"#
        );
    }

    #[test]
    fn removes_fill_sub_props_when_fill_none() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" fill="none" fill-rule="evenodd" fill-opacity="0.5"/></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" fill="none"/></svg>"#
        );
    }

    #[test]
    fn skips_elements_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path id="p" d="M0 0" stroke="none" stroke-dasharray="5"/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn skips_subtree_below_element_with_id() {
        // <use stroke="red" href="#g"> paints the referenced subtree with a
        // stroke, so its stroke-width is not useless.
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><g id="g"><path d="M0 0h5" stroke-width="4" stroke-dasharray="1"/></g></defs><use href="#g" stroke="red"/></svg>"##;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn bails_when_style_present() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><style>.a{stroke:red}</style><path d="M0 0" stroke="none" stroke-dasharray="5"/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn bails_when_script_present() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" stroke-dasharray="5" onclick="f()"/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn unchanged_when_stroke_visible() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" stroke="red" stroke-dasharray="5"/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn does_not_touch_non_shape_elements() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g stroke="none" stroke-dasharray="5"><rect/></g></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- inherited stroke / fill ---

    #[test]
    fn keeps_stroke_props_when_group_sets_stroke() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g stroke="#000"><path d="M0 0h5" stroke-width="4" stroke-dasharray="2 1" stroke-linecap="round"/></g></svg>"##;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn keeps_stroke_props_when_group_style_sets_stroke() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g style="stroke:#000"><path d="M0 0h5" stroke-width="4" stroke-dasharray="2 1"/></g></svg>"##;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn keeps_stroke_props_when_root_svg_sets_stroke() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" stroke="currentColor"><path d="M0 0h5" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn strips_stroke_props_when_group_stroke_is_none() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g stroke="none"><path d="M0 0h5" stroke-width="4"/></g></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g stroke="none"><path d="M0 0h5"/></g></svg>"#
        );
    }

    #[test]
    fn adds_stroke_none_when_parent_paints_stroke() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g stroke="#000"><path d="M0 0h5" stroke="red" stroke-width="0"/></g></svg>"##;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g stroke="#000"><path d="M0 0h5" stroke="none"/></g></svg>"##
        );
    }

    // --- own style="" overrides ---

    #[test]
    fn style_attr_stroke_beats_stroke_none_attr() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0h5" stroke="none" style="stroke:red" stroke-width="3"/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn style_attr_fill_beats_fill_none_attr() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0h5" fill="none" style="fill:red" fill-rule="evenodd"/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn style_attr_stroke_none_strips_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0h5" style="stroke:none" stroke="red" stroke-width="3"/></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0h5" style="stroke:none"/></svg>"#
        );
    }

    #[test]
    fn empty_style_declaration_is_unknown() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0h5" style="stroke:" stroke-width="3"/><path d="M0 0h5" style="fill:" fill-opacity="0"/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- marker-end ---

    #[test]
    fn marker_end_keeps_stroke_width() {
        // The marker scales with stroke-width even when the stroke is none.
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><line x1="0" y1="0" x2="5" y2="0" stroke="none" stroke-width="2" marker-end="url(#m)"/></svg>"##;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn inherited_marker_end_keeps_stroke_width() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g marker-end="url(#m)"><line x1="0" y1="0" x2="5" y2="0" stroke-width="2"/></g></svg>"##;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- SVGO fixtures ---

    #[test]
    fn svgo_fixture_01_stroke() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><g id="test"><rect stroke-dashoffset="5" width="100" height="100"/></g></defs><circle fill="red" stroke-width="6" stroke-dashoffset="5" cx="60" cy="60" r="50"/><circle fill="red" stroke="#000" stroke-width="6" stroke-dashoffset="5" stroke-opacity="0" cx="60" cy="60" r="50"/><circle fill="red" stroke="#000" stroke-width="0" stroke-dashoffset="5" cx="60" cy="60" r="50"/><circle fill="red" stroke="#000" stroke-width="6" stroke-dashoffset="5" cx="60" cy="60" r="50"/><g stroke="#000" stroke-width="6"><circle fill="red" stroke="red" stroke-width="0" stroke-dashoffset="5" cx="60" cy="60" r="50"/><circle fill="red" stroke-dashoffset="5" cx="60" cy="60" r="50"/></g><g stroke="#000"><circle fill="red" stroke-width="0" stroke-dashoffset="5" cx="60" cy="60" r="50"/><circle fill="red" stroke="none" stroke-dashoffset="5" cx="60" cy="60" r="50"/></g></svg>"##;
        let expected = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><g id="test"><rect stroke-dashoffset="5" width="100" height="100"/></g></defs><circle fill="red" cx="60" cy="60" r="50"/><circle fill="red" cx="60" cy="60" r="50"/><circle fill="red" cx="60" cy="60" r="50"/><circle fill="red" stroke="#000" stroke-width="6" stroke-dashoffset="5" cx="60" cy="60" r="50"/><g stroke="#000" stroke-width="6"><circle fill="red" cx="60" cy="60" r="50" stroke="none"/><circle fill="red" stroke-dashoffset="5" cx="60" cy="60" r="50"/></g><g stroke="#000"><circle fill="red" cx="60" cy="60" r="50" stroke="none"/><circle fill="red" cx="60" cy="60" r="50" stroke="none"/></g></svg>"##;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, expected);
    }

    #[test]
    fn svgo_fixture_02_fill() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><g id="test"><rect fill-opacity=".5" width="100" height="100"/></g></defs><circle fill="none" fill-rule="evenodd" cx="60" cy="60" r="50"/><circle fill="red" fill-opacity="0" cx="90" cy="90" r="50"/><circle fill-opacity="0" fill-rule="evenodd" cx="90" cy="60" r="50"/><circle fill="red" fill-opacity=".5" cx="60" cy="60" r="50"/><g fill="none"><circle fill-opacity=".5" cx="60" cy="60" r="50"/></g></svg>"#;
        let expected = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><g id="test"><rect fill-opacity=".5" width="100" height="100"/></g></defs><circle fill="none" cx="60" cy="60" r="50"/><circle fill="none" cx="90" cy="90" r="50"/><circle cx="90" cy="60" r="50" fill="none"/><circle fill="red" fill-opacity=".5" cx="60" cy="60" r="50"/><g fill="none"><circle cx="60" cy="60" r="50"/></g></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, expected);
    }

    #[test]
    fn svgo_fixture_04_marker_end() {
        let input = r##"<svg width="480" height="360" xmlns="http://www.w3.org/2000/svg"><defs><marker id="testMarker"><rect width="100" height="100" fill="blue"/></marker></defs><line x1="150" y1="150" x2="165" y2="150" stroke="red" stroke-width="25" marker-end="url(#testMarker)"/><line x1="250" y1="250" x2="265" y2="250" stroke="red" stroke-width="0" marker-end="url(#testMarker)"/></svg>"##;
        let expected = r##"<svg xmlns="http://www.w3.org/2000/svg" width="480" height="360"><defs><marker id="testMarker"><rect width="100" height="100" fill="blue"/></marker></defs><line x1="150" y1="150" x2="165" y2="150" stroke="red" stroke-width="25" marker-end="url(#testMarker)"/><line x1="250" y1="250" x2="265" y2="250" marker-end="url(#testMarker)"/></svg>"##;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, expected);
    }

    #[test]
    fn idempotent_on_own_output() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g stroke="#000"><circle fill="red" stroke-width="0" cx="60" cy="60" r="50"/></g><circle fill-opacity="0" fill-rule="evenodd" r="5"/></svg>"##;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        let (again, output2) = run_pass(&output);
        assert_eq!(again, PassResult::Unchanged, "{output} -> {output2}");
    }
}
