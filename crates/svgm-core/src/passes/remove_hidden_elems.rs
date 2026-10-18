use super::{
    CSS_GEOMETRY_PROPS, Computed, Pass, PassResult, animation_targets, computed_style_with,
    has_ancestor, has_scripts, has_stylesheet, parse_svg_number, style_sets_any,
    stylesheet_declares,
};
use crate::ast::{Document, Element, NodeId, NodeKind};

/// Attributes whose presence means the element may still be visually relevant
/// even if it appears to have zero geometry.
const EFFECT_ATTRS: &[&str] = &[
    "clip-path",
    "mask",
    "filter",
    "marker-start",
    "marker-mid",
    "marker-end",
];

pub struct RemoveHiddenElems;

impl Pass for RemoveHiddenElems {
    fn name(&self) -> &'static str {
        "removeHiddenElems"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let ids = doc.traverse();

        // Phase 1: Build animation target set.
        let animation_targets = animation_targets(doc);

        // A stylesheet or script may override `display` on any element
        // (`.a{display:block}` beats display="none"; a :hover rule or a
        // script may reveal it), so the computed value is unknown while
        // either exists. inlineStyles runs before this pass and removes
        // the sheet in the common case.
        let dynamic = has_stylesheet(doc) || has_scripts(doc);
        // A rule that sets geometry (`.a{width:30px}`) overrides a zero or
        // missing attribute; without selector matching, any such rule makes
        // every element's size unknown.
        let sheet_geometry = has_stylesheet(doc) && stylesheet_declares(doc, CSS_GEOMETRY_PROPS);

        // Phase 2: Check and remove hidden elements.
        let mut changed = false;

        for &nid in &ids {
            if doc.node(nid).removed {
                continue;
            }
            let node = doc.node(nid);
            let elem = match &node.kind {
                NodeKind::Element(elem) => elem,
                _ => continue,
            };
            let name = elem.name.as_str();

            // Skip: svg and symbol elements are never removed
            if name == "svg" || name == "symbol" {
                continue;
            }

            // Skip: elements inside <defs> or <symbol>
            if is_inside_defs_or_symbol(doc, nid) {
                continue;
            }

            // Skip: elements with id (might be referenced externally)
            if elem.attr("id").is_some() {
                continue;
            }

            // Skip: animation targets
            if animation_targets.contains(&nid) {
                continue;
            }

            // Skip: elements with effect-bearing attributes
            if EFFECT_ATTRS.iter().any(|&a| elem.attr(a).is_some()) {
                continue;
            }

            if should_remove(doc, nid, elem, name, dynamic, sheet_geometry) {
                doc.remove(nid);
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

/// Determine if an element should be removed based on its properties.
fn should_remove(
    doc: &Document,
    nid: NodeId,
    elem: &Element,
    name: &str,
    dynamic: bool,
    sheet_geometry: bool,
) -> bool {
    // display="none" / style="display:none" — universally invisible. The
    // computed value accounts for a `style=""` override (`display="none"
    // style="display:inline"` renders) and is unknown under a stylesheet.
    // Markers with display:none are still rendered where referenced.
    if name != "marker"
        && matches!(computed_style_with(doc, nid, "display", dynamic),
            Computed::Static(v) if v.trim() == "none")
    {
        return true;
    }

    // Geometry set in CSS (`style="width:30px"`, `r:15px`, `d:path(...)`)
    // overrides a zero or missing attribute: the size is unknown.
    if sheet_geometry || style_sets_any(elem, CSS_GEOMETRY_PROPS) {
        return false;
    }

    // Zero-size geometry checks. Only an explicitly numeric zero counts:
    // a unit or percentage value ("0%", "1em") is left alone.
    match name {
        "rect" => {
            if is_explicit_zero(elem.attr("width")) || is_explicit_zero(elem.attr("height")) {
                return true;
            }
        }
        "circle" => {
            // Missing r = no rendering per SVG spec; explicit r="0" = no rendering
            match elem.attr("r") {
                None => return true,
                Some(v) if is_zero(v) => return true,
                _ => {}
            }
        }
        "ellipse" => {
            // SVG2: a missing (or "auto") radius takes the other radius'
            // value, so <ellipse ry="10"> renders as a circle. Only an
            // explicit zero, or both radii missing, disables rendering.
            let rx = elem.attr("rx").filter(|v| v.trim() != "auto");
            let ry = elem.attr("ry").filter(|v| v.trim() != "auto");
            if (rx.is_none() && ry.is_none()) || is_explicit_zero(rx) || is_explicit_zero(ry) {
                return true;
            }
        }
        "path" => match elem.attr("d") {
            None => return true,
            Some(d) if d.trim().is_empty() => return true,
            _ => {}
        },
        "image"
            if is_explicit_zero(elem.attr("width")) || is_explicit_zero(elem.attr("height")) =>
        {
            return true;
        }
        _ => {}
    }

    false
}

/// Check if a value is explicitly "0" (parses to 0.0).
fn is_explicit_zero(value: Option<&str>) -> bool {
    match value {
        Some(v) => is_zero(v),
        None => false,
    }
}

/// Check if a string value represents zero.
fn is_zero(v: &str) -> bool {
    parse_svg_number(v.trim().trim_end_matches("px")) == Some(0.0)
}

/// Walk up the parent chain to check if this node is inside a <defs> or <symbol>.
fn is_inside_defs_or_symbol(doc: &Document, nid: NodeId) -> bool {
    has_ancestor(doc, nid, &["defs", "symbol"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = RemoveHiddenElems.run(&mut doc);
        (result, serialize(&doc))
    }

    // --- geometry set in CSS (#48) ---

    #[test]
    fn css_geometry_in_style_keeps_zero_or_missing_attrs() {
        for shape in [
            "<rect width=\"0\" height=\"10\" style=\"width:30px\"/>",
            "<circle r=\"0\" style=\"r:15px\"/>",
            "<circle style=\"r:15px\"/>",
            "<ellipse rx=\"0\" ry=\"5\" style=\"rx:3px\"/>",
            "<image width=\"0\" height=\"1\" style=\"width:5px\"/>",
            "<path style=\"d:path('M0 0h5')\"/>",
        ] {
            let input = format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{shape}</svg>");
            let (result, output) = run_pass(&input);
            assert_eq!(result, PassResult::Unchanged, "{shape}");
            assert_eq!(output, input);
        }
    }

    #[test]
    fn non_geometry_style_still_allows_zero_size_removal() {
        let (result, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"0\" height=\"10\" style=\"fill:red\"/></svg>",
        );
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<rect"), "{output}");
    }

    #[test]
    fn stylesheet_geometry_keeps_zero_size_elements() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{width:30px}.a:hover{fill:red}</style><rect class=\"a\" width=\"0\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }

    // --- display="none" ---

    #[test]
    fn removes_display_none() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect display=\"none\" width=\"10\" height=\"10\"/><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<rect"));
        assert!(output.contains("<circle"));
    }

    #[test]
    fn removes_style_display_none() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect style=\"display:none\" width=\"10\" height=\"10\"/><g style=\"fill:red; display : none ;\"><circle r=\"5\"/></g><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg>"
        );
    }

    #[test]
    fn keeps_display_none_overridden_by_style_attr() {
        // style="" beats the presentation attribute: the rect renders.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect display=\"none\" style=\"display:inline\" width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn keeps_display_none_when_stylesheet_may_override() {
        // .a{display:block} wins over display="none".
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{display:block}</style><rect class=\"a\" display=\"none\" width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn keeps_display_none_when_stylesheet_has_dynamic_rules() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>@media (hover:hover){.a:hover{display:inline}}</style><rect class=\"a\" display=\"none\" width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn keeps_display_none_when_script_present() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>show()</script><rect display=\"none\" width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn keeps_marker_with_display_none() {
        // Markers are rendered where referenced regardless of display.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><marker style=\"display:none\"><path d=\"M0 0h1\"/></marker><marker display=\"none\"><path d=\"M0 0h1\"/></marker></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn display_is_not_inherited_from_group() {
        // display is not an inherited property; the child itself renders
        // nothing only because its parent does, which is the parent's
        // decision (and the parent gets removed as a whole).
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g display=\"none\"><rect width=\"10\" height=\"10\"/></g></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>");
    }

    // --- Zero-size geometry ---

    #[test]
    fn removes_zero_width_rect() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"0\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<rect"));
    }

    #[test]
    fn removes_zero_height_rect() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"0\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<rect"));
    }

    #[test]
    fn keeps_rect_with_percent_or_unit_size() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"0%\" height=\"10\"/><rect width=\"10\" height=\"1em\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn removes_zero_radius_circle() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"5\" cy=\"5\" r=\"0\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<circle"));
    }

    #[test]
    fn removes_circle_missing_r() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"5\" cy=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<circle"));
    }

    #[test]
    fn removes_zero_rx_ellipse() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><ellipse cx=\"5\" cy=\"5\" rx=\"0\" ry=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<ellipse"));
    }

    #[test]
    fn removes_zero_ry_ellipse() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><ellipse cx=\"5\" cy=\"5\" rx=\"10\" ry=\"0\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<ellipse"));
    }

    #[test]
    fn preserves_ellipse_missing_rx_svg2_auto() {
        // SVG2: rx defaults to auto = ry, so this renders as a circle.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><ellipse cx=\"5\" cy=\"5\" ry=\"10\"/><ellipse cx=\"5\" cy=\"5\" rx=\"10\"/><ellipse rx=\"auto\" ry=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output.matches("<ellipse").count(), 3);
    }

    #[test]
    fn removes_ellipse_both_radii_missing() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><ellipse cx=\"5\" cy=\"5\"/><ellipse rx=\"auto\" ry=\"auto\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<ellipse"));
    }

    #[test]
    fn removes_path_empty_d() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<path"));
    }

    #[test]
    fn removes_path_missing_d() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<path"));
    }

    #[test]
    fn keeps_zero_length_line() {
        // SVGO has no zero-length line rule: with a round/square linecap
        // or markers the line renders a dot, and unit coordinates
        // (x2="100%") are not comparable to numbers anyway.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><line x1=\"5\" y1=\"5\" x2=\"5\" y2=\"5\" stroke=\"red\" stroke-linecap=\"round\"/><line/><line x1=\"50%\" x2=\"50%\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output.matches("<line").count(), 3);
    }

    #[test]
    fn removes_zero_width_image() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><image width=\"0\" height=\"10\" href=\"img.png\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<image"));
    }

    // --- fill="none" + stroke="none" is NOT a removal rule ---

    #[test]
    fn keeps_leaf_shape_no_fill_no_stroke() {
        // SVGO only removes these with the opt-in removeNone param: the
        // shape may be a pointer-events hit area, a clipPath/mask/marker
        // child, or restyled by CSS.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect fill=\"none\" stroke=\"none\" width=\"10\" height=\"10\"/><rect fill=\"none\" stroke=\"none\" pointer-events=\"all\" width=\"10\" height=\"10\"/><clipPath><rect fill=\"none\" stroke=\"none\" width=\"10\" height=\"10\"/></clipPath><mask><path fill=\"none\" stroke=\"none\" d=\"M0 0h1\"/></mask><marker><path fill=\"none\" stroke=\"none\" d=\"M0 0h1\"/></marker><rect fill=\"none\" stroke=\"none\" style=\"fill:blue\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output.matches("fill=\"none\"").count(), 6);
    }

    #[test]
    fn keeps_leaf_shape_no_fill_no_stroke_under_stylesheet() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{fill:red}</style><rect class=\"a\" fill=\"none\" stroke=\"none\" width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Safety: preserved elements ---

    #[test]
    fn preserves_inside_defs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><rect width=\"0\" height=\"0\"/></defs></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<rect"));
    }

    #[test]
    fn preserves_inside_symbol() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><symbol id=\"s\"><rect width=\"0\" height=\"0\"/></symbol></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<rect"));
    }

    #[test]
    fn preserves_element_with_id() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"keep\" width=\"0\" height=\"0\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn preserves_animation_target_parent() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect display=\"none\" width=\"10\" height=\"10\"><animate attributeName=\"display\" to=\"inline\" dur=\"1s\"/></rect></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<rect"));
    }

    #[test]
    fn preserves_animation_target_via_href() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"r\" display=\"none\" width=\"10\" height=\"10\"/><animate href=\"#r\" attributeName=\"display\" to=\"inline\" dur=\"1s\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn preserves_svg_element() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" display=\"none\"><rect width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<svg"));
    }

    #[test]
    fn preserves_element_with_clip_path() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"0\" height=\"0\" clip-path=\"url(#c)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn preserves_element_with_filter() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"0\" height=\"0\" filter=\"url(#f)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn preserves_element_with_mask() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"0\" height=\"0\" mask=\"url(#m)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Preservation: visible elements ---

    #[test]
    fn preserves_visible_rect() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn unchanged_when_nothing_to_remove() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"5\" cy=\"5\" r=\"5\"/><rect width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Deferred: not removed in v1 ---

    #[test]
    fn does_not_remove_visibility_hidden() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect visibility=\"hidden\" width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn does_not_remove_opacity_zero() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect opacity=\"0\" width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }
}
