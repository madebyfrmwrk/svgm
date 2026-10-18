use super::{Pass, PassResult, parent_is, style_declaration, stylesheet_contains};
use crate::ast::{Document, Element, NodeKind};

/// Container elements that can be removed when empty.
const REMOVABLE_CONTAINERS: &[&str] = &[
    "g",
    "defs",
    "symbol",
    "clipPath",
    "mask",
    "pattern",
    "linearGradient",
    "radialGradient",
    "filter",
    "marker",
];

pub struct RemoveEmptyContainers;

impl Pass for RemoveEmptyContainers {
    fn name(&self) -> &'static str {
        "removeEmptyContainers"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        // A `<style>` rule may apply a filter to a class; this pass runs
        // before inlineStyles, so the class is still the only evidence.
        let css_filter = stylesheet_contains(doc, "filter");

        for id in ids {
            let node = doc.node(id);
            if let NodeKind::Element(ref elem) = node.kind
                && elem.prefix.is_none()
                && REMOVABLE_CONTAINERS.contains(&elem.name.as_str())
            {
                // Consider empty if no children, or only whitespace text children
                let has_meaningful_children =
                    doc.children(id).any(|child| match &doc.node(child).kind {
                        NodeKind::Text(t) => !t.trim().is_empty(),
                        _ => true,
                    });
                if !has_meaningful_children {
                    // Don't remove if it has an id — it might be referenced
                    if elem.attr("id").is_some() {
                        continue;
                    }
                    // Children of <switch> are alternatives: an empty branch
                    // with requiredFeatures/systemLanguage still selects which
                    // sibling renders (SVGO removeEmptyContainers.06).
                    if parent_is(doc, id, "switch") {
                        continue;
                    }
                    // An empty <g> with a filter may still produce pixels
                    // (feFlood/feTile/feImage) — SVGO removeEmptyContainers.04/.07.
                    if elem.name == "g" && has_filter(elem, css_filter) {
                        continue;
                    }
                    doc.remove(id);
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

/// True when a filter may apply to the element: a `filter` attribute, a
/// `style` declaration (even an empty one — unknown means unsafe), or a
/// `class` while some stylesheet mentions `filter`.
fn has_filter(elem: &Element, css_filter: bool) -> bool {
    elem.attr("filter").is_some()
        || style_declaration(elem, "filter").is_some()
        || (css_filter && elem.attr("class").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_empty_g() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g></g><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g>"));
    }

    #[test]
    fn keeps_g_with_children() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn keeps_empty_g_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g id="target"></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn removes_nested_empty_containers_across_passes() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><g></g></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // First pass removes inner <g>
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
        // Second pass removes now-empty outer <g>
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g>"));
    }

    // ── <switch> children (SVGO removeEmptyContainers.06) ──────────────

    #[test]
    fn keeps_empty_switch_branch() {
        // draw.io export: the empty requiredFeatures branch selects the
        // rendered sibling in capable viewers.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 462 352\"><switch><g requiredFeatures=\"http://www.w3.org/TR/SVG11/feature#Extensibility\"/><a transform=\"translate(0,-5)\" href=\"https://www.diagrams.net/doc/faq/svg-export-text-problems\" target=\"_blank\"><text text-anchor=\"middle\" font-size=\"10px\" x=\"50%\" y=\"100%\">Viewer does not support full SVG 1.1</text></a></switch></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(output.contains("<switch><g requiredFeatures"), "{output}");
    }

    #[test]
    fn keeps_attr_less_empty_g_inside_switch() {
        // Even without conditional attributes the first child of a <switch>
        // is the branch that renders; removing it changes the selection.
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><switch><g/><g><rect/></g></switch></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn removes_empty_g_nested_deeper_than_switch() {
        // Only direct children of <switch> are alternatives.
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><switch><g><g/><rect/></g></switch></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><switch><g><rect/></g></switch></svg>"#
        );
    }

    // ── Empty <g> with a filter (SVGO removeEmptyContainers.04/.07) ────

    #[test]
    fn keeps_empty_g_with_filter_attribute() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><filter id=\"feTileFilter\" filterUnits=\"userSpaceOnUse\" primitiveUnits=\"userSpaceOnUse\" x=\"115\" y=\"40\" width=\"250\" height=\"250\"><feFlood x=\"115\" y=\"40\" width=\"54\" height=\"19\" flood-color=\"lime\"/><feOffset x=\"115\" y=\"40\" width=\"50\" height=\"25\" dx=\"6\" dy=\"6\" result=\"offset\"/><feTile/></filter></defs><g filter=\"url(#feTileFilter)\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn keeps_empty_g_with_style_filter() {
        let input = "<svg viewBox=\"0 0 50 50\" xmlns=\"http://www.w3.org/2000/svg\"><filter id=\"a\" x=\"0\" y=\"0\" width=\"50\" height=\"50\" filterUnits=\"userSpaceOnUse\"><feFlood flood-color=\"#aaa\"/></filter><mask id=\"b\" x=\"0\" y=\"0\" width=\"50\" height=\"50\"><g style=\"filter: url(#a)\"/></mask><text x=\"16\" y=\"16\" style=\"mask: url(#b)\">hi</text></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
        // Empty declaration: unknown is unsafe, keep.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="filter:"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn keeps_empty_g_with_class_when_stylesheet_mentions_filter() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.f{filter:url(#a)}</style><filter id=\"a\"><feFlood flood-color=\"red\"/></filter><g class=\"f\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Unchanged);
        // A stylesheet that never mentions filter cannot apply one.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.f{fill:red}</style><g class=\"f\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
    }

    #[test]
    fn filter_only_protects_g() {
        // SVGO keeps only <g>; an empty <defs filter=...> is still removed.
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs filter="url(#a)"/></svg>"##;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEmptyContainers.run(&mut doc), PassResult::Changed);
    }

    #[test]
    fn full_optimize_keeps_filtered_empty_g_and_its_defs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 50 50\"><defs><filter id=\"feTileFilter\" filterUnits=\"userSpaceOnUse\" primitiveUnits=\"userSpaceOnUse\" x=\"115\" y=\"40\" width=\"250\" height=\"250\"><feFlood x=\"115\" y=\"40\" width=\"54\" height=\"19\" flood-color=\"lime\"/><feOffset x=\"115\" y=\"40\" width=\"50\" height=\"25\" dx=\"6\" dy=\"6\" result=\"offset\"/><feTile/></filter></defs><g filter=\"url(#feTileFilter)\"/></svg>";
        let out = crate::optimize(input).unwrap().data;
        assert!(out.contains("<filter"), "{out}");
        assert!(out.contains("<feTile/>"), "{out}");
        assert!(out.contains("<g filter=\"url(#"), "{out}");
    }
}
