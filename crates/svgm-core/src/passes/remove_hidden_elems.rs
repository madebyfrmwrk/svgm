use super::{Pass, PassResult};
use crate::ast::{Document, NodeId, NodeKind};
use std::collections::{HashMap, HashSet};

/// Animation element names that can toggle rendering properties.
const ANIMATION_ELEMS: &[&str] = &["animate", "animateTransform", "animateMotion", "set"];

/// Shape elements where fill="none" + stroke="none" means invisible (if leaf).
const SHAPE_ELEMS: &[&str] = &[
    "rect", "circle", "ellipse", "path", "line", "polyline", "polygon",
];

/// Attributes whose presence means the element may still be visually relevant
/// even if it appears to have zero geometry or no fill/stroke.
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
        // Build id->NodeId map for resolving href targets.
        let mut id_to_node: HashMap<&str, NodeId> = HashMap::new();
        for &nid in &ids {
            if let NodeKind::Element(ref elem) = doc.node(nid).kind
                && let Some(id_val) = elem.attr("id")
            {
                id_to_node.insert(id_val, nid);
            }
        }

        let mut animation_targets: HashSet<NodeId> = HashSet::new();
        for &nid in &ids {
            if let NodeKind::Element(ref elem) = doc.node(nid).kind
                && ANIMATION_ELEMS.contains(&elem.name.as_str())
            {
                // Target via href or xlink:href
                let href_target = elem
                    .attr("href")
                    .or_else(|| {
                        elem.attributes
                            .iter()
                            .find(|a| a.name == "href" && a.prefix.as_deref() == Some("xlink"))
                            .map(|a| a.value.as_str())
                    })
                    .and_then(|v| v.strip_prefix('#'))
                    .and_then(|id| id_to_node.get(id).copied());

                if let Some(target) = href_target {
                    animation_targets.insert(target);
                } else if let Some(parent) = doc.node(nid).parent {
                    // Default: animation targets its parent element
                    animation_targets.insert(parent);
                }
            }
        }

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

            if should_remove(doc, nid, elem, name) {
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
fn should_remove(doc: &Document, nid: NodeId, elem: &crate::ast::Element, name: &str) -> bool {
    // display="none" — universally invisible
    if elem.attr("display") == Some("none") {
        return true;
    }

    // Zero-size geometry checks
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
            // Missing rx or ry = no rendering; explicit ="0" = no rendering
            let rx = elem.attr("rx");
            let ry = elem.attr("ry");
            if rx.is_none() || ry.is_none() || is_zero(rx.unwrap()) || is_zero(ry.unwrap()) {
                return true;
            }
        }
        "path" => {
            match elem.attr("d") {
                None => return true,
                Some(d) if d.trim().is_empty() => return true,
                _ => {}
            }
        }
        "line" => {
            let x1 = parse_coord(elem.attr("x1"));
            let y1 = parse_coord(elem.attr("y1"));
            let x2 = parse_coord(elem.attr("x2"));
            let y2 = parse_coord(elem.attr("y2"));
            if x1 == x2 && y1 == y2 {
                return true;
            }
        }
        "image" => {
            if is_explicit_zero(elem.attr("width")) || is_explicit_zero(elem.attr("height")) {
                return true;
            }
        }
        _ => {}
    }

    // fill="none" + stroke="none" on leaf shape elements
    if SHAPE_ELEMS.contains(&name)
        && elem.attr("fill") == Some("none")
        && elem.attr("stroke") == Some("none")
        && !doc.children(nid).any(|_| true)
    {
        return true;
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
    v.trim().trim_end_matches("px").parse::<f64>() == Ok(0.0)
}

/// Parse a coordinate attribute, defaulting to 0.0 per SVG spec.
fn parse_coord(value: Option<&str>) -> f64 {
    value
        .and_then(|v| v.trim().trim_end_matches("px").parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// Walk up the parent chain to check if this node is inside a <defs> or <symbol>.
fn is_inside_defs_or_symbol(doc: &Document, nid: NodeId) -> bool {
    let mut current = doc.node(nid).parent;
    while let Some(pid) = current {
        if let NodeKind::Element(ref elem) = doc.node(pid).kind {
            match elem.name.as_str() {
                "defs" | "symbol" => return true,
                _ => {}
            }
        }
        current = doc.node(pid).parent;
    }
    false
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

    // --- display="none" ---

    #[test]
    fn removes_display_none() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect display=\"none\" width=\"10\" height=\"10\"/><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<rect"));
        assert!(output.contains("<circle"));
    }

    // --- Zero-size geometry ---

    #[test]
    fn removes_zero_width_rect() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"0\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<rect"));
    }

    #[test]
    fn removes_zero_height_rect() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"0\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<rect"));
    }

    #[test]
    fn removes_zero_radius_circle() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"5\" cy=\"5\" r=\"0\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<circle"));
    }

    #[test]
    fn removes_circle_missing_r() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"5\" cy=\"5\"/></svg>";
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
    fn removes_ellipse_missing_rx() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><ellipse cx=\"5\" cy=\"5\" ry=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<ellipse"));
    }

    #[test]
    fn removes_path_empty_d() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"\"/></svg>";
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
    fn removes_zero_length_line() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><line x1=\"5\" y1=\"5\" x2=\"5\" y2=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<line"));
    }

    #[test]
    fn removes_line_default_coords() {
        // All coordinates default to 0, so x1==x2 && y1==y2
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><line/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<line"));
    }

    #[test]
    fn removes_zero_width_image() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><image width=\"0\" height=\"10\" href=\"img.png\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<image"));
    }

    // --- fill="none" + stroke="none" ---

    #[test]
    fn removes_leaf_shape_no_fill_no_stroke() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect fill=\"none\" stroke=\"none\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<rect"));
    }

    #[test]
    fn preserves_shape_no_fill_no_stroke_with_children() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text fill=\"none\" stroke=\"none\"><tspan>hi</tspan></text></svg>";
        let (result, _) = run_pass(input);
        // text is not in SHAPE_ELEMS, so this wouldn't be caught anyway.
        // But test the children logic with a real case:
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
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>";
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
