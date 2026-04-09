use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Removes useless stroke and fill sub-properties when the primary property
/// makes them irrelevant (e.g., stroke-dasharray when stroke="none").
pub struct RemoveUselessStrokeAndFill;

const SHAPE_ELEMENTS: &[&str] = &[
    "circle", "ellipse", "line", "path", "polygon", "polyline", "rect",
];

const STROKE_SUB_PROPS: &[&str] = &[
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
];

const FILL_SUB_PROPS: &[&str] = &["fill-opacity", "fill-rule"];

impl Pass for RemoveUselessStrokeAndFill {
    fn name(&self) -> &'static str {
        "removeUselessStrokeAndFill"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let ids = doc.traverse();

        // Bail if <style> or <script> elements exist — CSS/JS may affect computed styles
        for &id in &ids {
            if let NodeKind::Element(ref e) = doc.node(id).kind
                && matches!(e.name.as_str(), "style" | "script")
            {
                return PassResult::Unchanged;
            }
        }

        let mut changed = false;

        for &id in &ids {
            let node = doc.node(id);
            let elem = match &node.kind {
                NodeKind::Element(e) => e,
                _ => continue,
            };

            // Only process shape elements
            if !SHAPE_ELEMENTS.contains(&elem.name.as_str()) {
                continue;
            }

            // Skip elements with id (may be referenced)
            if elem.attr("id").is_some() {
                continue;
            }

            let stroke_is_none = is_stroke_invisible(elem);
            let fill_is_none = is_fill_invisible(elem);

            let mut to_remove: Vec<usize> = Vec::new();

            if stroke_is_none {
                // Remove stroke sub-properties (but NOT stroke="none" itself — needed for inheritance)
                for (i, a) in elem.attributes.iter().enumerate() {
                    if a.prefix.is_none() && STROKE_SUB_PROPS.contains(&a.name.as_str()) {
                        // Don't remove the property that makes stroke invisible
                        if a.name == "stroke-opacity" && a.value == "0" {
                            continue;
                        }
                        if a.name == "stroke-width" && a.value == "0" {
                            continue;
                        }
                        to_remove.push(i);
                    }
                }
            }

            if fill_is_none {
                // Remove fill sub-properties (but NOT fill="none" itself)
                for (i, a) in elem.attributes.iter().enumerate() {
                    if a.prefix.is_none() && FILL_SUB_PROPS.contains(&a.name.as_str()) {
                        // Don't remove the property that makes fill invisible
                        if a.name == "fill-opacity" && a.value == "0" {
                            continue;
                        }
                        to_remove.push(i);
                    }
                }
            }

            if !to_remove.is_empty() {
                to_remove.sort_unstable();
                to_remove.dedup();
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

/// Check if stroke is provably invisible via attributes on this element or inherited.
fn is_stroke_invisible(elem: &crate::ast::Element) -> bool {
    if elem.attr("stroke").is_some_and(|v| v == "none") {
        return true;
    }
    // No stroke attribute = inherits default "none" (unless set via style)
    if elem.attr("stroke").is_none() {
        let has_style_stroke = elem
            .attr("style")
            .is_some_and(|s| s.contains("stroke:") || s.contains("stroke :"));
        if !has_style_stroke {
            return true;
        }
    }
    if elem.attr("stroke-opacity").is_some_and(|v| v == "0") {
        return true;
    }
    if elem.attr("stroke-width").is_some_and(|v| v == "0") {
        return true;
    }
    false
}

/// Check if fill is provably invisible via attributes on this element.
fn is_fill_invisible(elem: &crate::ast::Element) -> bool {
    if elem.attr("fill").is_some_and(|v| v == "none") {
        return true;
    }
    if elem.attr("fill-opacity").is_some_and(|v| v == "0") {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_stroke_sub_props_when_stroke_none() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" stroke="none" stroke-dasharray="5" stroke-linecap="round"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveUselessStrokeAndFill.run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(!output.contains("stroke-dasharray"));
        assert!(!output.contains("stroke-linecap"));
        assert!(output.contains("stroke=\"none\""));
    }

    #[test]
    fn removes_stroke_sub_props_when_stroke_width_zero() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10" stroke="red" stroke-width="0" stroke-dasharray="5"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveUselessStrokeAndFill.run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(!output.contains("stroke-dasharray"));
        // stroke-width="0" is kept (it's the trigger)
        assert!(output.contains("stroke-width=\"0\""));
    }

    #[test]
    fn removes_fill_sub_props_when_fill_none() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" fill="none" fill-rule="evenodd" fill-opacity="0.5"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveUselessStrokeAndFill.run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(!output.contains("fill-rule"));
        assert!(!output.contains("fill-opacity"));
        assert!(output.contains("fill=\"none\""));
    }

    #[test]
    fn skips_elements_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path id="p" d="M0 0" stroke="none" stroke-dasharray="5"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveUselessStrokeAndFill.run(&mut doc),
            PassResult::Unchanged
        );
    }

    #[test]
    fn bails_when_style_present() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><style>.a{stroke:red}</style><path d="M0 0" stroke="none" stroke-dasharray="5"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveUselessStrokeAndFill.run(&mut doc),
            PassResult::Unchanged
        );
    }

    #[test]
    fn unchanged_when_stroke_visible() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" stroke="red" stroke-dasharray="5"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveUselessStrokeAndFill.run(&mut doc),
            PassResult::Unchanged
        );
    }

    #[test]
    fn does_not_touch_non_shape_elements() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g stroke="none" stroke-dasharray="5"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            RemoveUselessStrokeAndFill.run(&mut doc),
            PassResult::Unchanged
        );
    }
}
