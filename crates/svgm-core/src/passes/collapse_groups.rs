use super::{
    MOVEABLE_ELEMENTS, Pass, PassResult, has_animated_attr_deep, has_animation_child,
    has_group_only_prop, has_structural_css, parent_is, split_declarations, split_property,
    strip_important, style_declaration,
};
use crate::ast::{Document, Element, Namespace, NodeId, NodeKind};

pub struct CollapseGroups;

impl Pass for CollapseGroups {
    fn name(&self) -> &'static str {
        "collapseGroups"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        // A stylesheet that survived inlineStyles may select by structure
        // (`g rect`, `.a > rect`, `#g rect`, `:nth-child`); removing or
        // merging a group would silently change what those rules match.
        if has_structural_css(doc) {
            return PassResult::Unchanged;
        }

        let mut changed = false;

        // Process bottom-up so inner groups collapse first.
        let mut ids = doc.traverse();
        ids.reverse();

        for id in ids {
            if doc.node(id).removed {
                continue;
            }

            let NodeKind::Element(ref elem) = doc.node(id).kind else {
                continue;
            };
            if elem.name != "g" || elem.prefix.is_some() {
                continue;
            }
            let Some(parent_id) = doc.node(id).parent else {
                continue;
            };
            // A <g> that is the document root has nothing to collapse into.
            if matches!(doc.node(parent_id).kind, NodeKind::Root) {
                continue;
            }
            // Children of <switch> are alternatives: unwrapping one would
            // turn its contents into several branches (SVGO collapseGroups.14).
            if parent_is(doc, id, "switch") {
                continue;
            }

            let children: Vec<NodeId> = doc.children(id).collect();

            // Case 1: Empty group — removeEmptyContainers' job (it knows
            // which empty groups must stay, e.g. filtered ones).
            if children.is_empty() {
                continue;
            }

            // Animation children "add" attributes to the group; the group
            // must stay (SVGO collapseGroups.12).
            if has_animation_child(doc, id) {
                continue;
            }

            // Case 2: Group with no attributes and no namespace declarations
            // that matter — unwrap children to parent.
            if elem.attributes.is_empty() && effective_namespaces(doc, id).is_empty() {
                hoist_children(doc, id, parent_id);
                doc.node_mut(id).removed = true;
                changed = true;
                continue;
            }

            // Case 3: Group with single element child — merge attrs down if no conflicts
            if children.len() == 1 {
                let child_id = children[0];
                if let NodeKind::Element(ref child_elem) = doc.node(child_id).kind
                    && can_merge_into_child(doc, elem, child_id, child_elem)
                {
                    merge_group_into_child(doc, id, child_id);
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

/// Namespace declarations on `id` that change what a prefix resolves to,
/// i.e. are not already bound to the same URI by the nearest ancestor
/// declaration. Dropping any of these would leave descendants with an
/// undeclared (or re-bound) prefix.
fn effective_namespaces(doc: &Document, id: NodeId) -> Vec<Namespace> {
    let NodeKind::Element(ref elem) = doc.node(id).kind else {
        return Vec::new();
    };
    elem.namespaces
        .iter()
        .filter(|ns| {
            let mut current = doc.node(id).parent;
            while let Some(pid) = current {
                if let NodeKind::Element(ref e) = doc.node(pid).kind
                    && let Some(decl) = e.namespaces.iter().find(|d| d.prefix == ns.prefix)
                {
                    return decl.uri != ns.uri;
                }
                current = doc.node(pid).parent;
            }
            true
        })
        .cloned()
        .collect()
}

/// Move all children of `group_id` to be children of `parent_id`,
/// replacing the group's position in the parent's child list.
fn hoist_children(doc: &mut Document, group_id: NodeId, parent_id: NodeId) {
    let group_children: Vec<NodeId> = doc.node(group_id).children.clone();
    let parent = doc.node_mut(parent_id);
    let pos = parent.children.iter().position(|&c| c == group_id);

    if let Some(pos) = pos {
        // Replace the group with its children in the parent's child list
        parent
            .children
            .splice(pos..=pos, group_children.iter().copied());
        // Update parent pointers
        for &child in &group_children {
            doc.node_mut(child).parent = Some(parent_id);
        }
    }
}

fn has_attr(elem: &Element, name: &str) -> bool {
    elem.attr(name).is_some()
}

/// Check if the group's attributes can be safely merged into its single
/// element child (SVGO collapseGroups "move group attributes to the single
/// child element" guards). Transform is composed (group's prepended to the
/// child's); every other attribute must be absent on the child.
fn can_merge_into_child(
    doc: &Document,
    group: &Element,
    child_id: NodeId,
    child: &Element,
) -> bool {
    // Only elements that can carry the group's presentation attributes and
    // a transform. Non-rendered resources (clipPath, mask, symbol,
    // gradients, defs ...) have different semantics for them.
    if child.prefix.is_some() || !MOVEABLE_ELEMENTS.contains(&child.name.as_str()) {
        return false;
    }
    // A child with an id may be instanced by <use> (which would then inherit
    // the merged attributes) or be targeted by a script/animation.
    if has_attr(child, "id") {
        return false;
    }
    // clip-path/mask/filter on the group (attribute or style) apply to the
    // group's composited result in the group's coordinate system.
    if has_group_only_prop(group) {
        return false;
    }
    let group_tf = has_attr(group, "transform");
    let child_tf = has_attr(child, "transform");
    // A CSS transform (style="transform:...") overrides the presentation
    // attribute, so composing attribute transforms across it is wrong.
    if (group_tf || child_tf)
        && (style_declaration(group, "transform").is_some()
            || style_declaration(child, "transform").is_some())
    {
        return false;
    }
    for g_attr in &group.attributes {
        // Never copy an attribute the child (or its subtree) animates.
        if g_attr.prefix.is_none() && has_animated_attr_deep(doc, child_id, &g_attr.name) {
            return false;
        }
        // Transform can be composed (prepend group's to child's)
        if g_attr.name == "transform" && g_attr.prefix.is_none() {
            continue;
        }
        // If the child already has this attribute, don't merge
        if child
            .attributes
            .iter()
            .any(|a| a.name == g_attr.name && a.prefix == g_attr.prefix)
        {
            return false;
        }
    }
    // A `style=""` declaration and a presentation attribute for the same
    // property are not interchangeable: on one element the inline style
    // always wins. Moving `<g style="fill:red">` onto `<path fill="black">`
    // would turn the path red, and moving `<g transform>` onto a child whose
    // style declares `transform` would have the composed attribute ignored.
    // SVGO 4.0.1 merges these anyway (and renders wrong); svgm refuses.
    !style_conflicts_with_attrs(group, child) && !style_conflicts_with_attrs(child, group)
}

/// True when `styled`'s `style=""` declares a property that `other` carries
/// as a presentation attribute with a different value (`transform` conflicts
/// regardless of value, since it would be composed rather than copied).
fn style_conflicts_with_attrs(styled: &Element, other: &Element) -> bool {
    let Some(style) = styled.attr("style") else {
        return false;
    };
    split_declarations(style)
        .into_iter()
        .filter_map(split_property)
        .any(|(name, value)| {
            // CSS property names are case-insensitive; attribute names are not.
            let prop = name.to_ascii_lowercase();
            other.attr(&prop).is_some_and(|attr_value| {
                prop == "transform" || strip_important(value) != attr_value.trim()
            })
        })
}

/// Merge the group's attributes (and namespace declarations) into its single
/// child, then hoist the child.
fn merge_group_into_child(doc: &mut Document, group_id: NodeId, child_id: NodeId) {
    // Clone group attrs before mutating
    let group_attrs = if let NodeKind::Element(ref elem) = doc.node(group_id).kind {
        elem.attributes.clone()
    } else {
        return;
    };
    let group_ns = effective_namespaces(doc, group_id);

    // Add group attrs to child, composing transforms
    if let NodeKind::Element(ref mut child_elem) = doc.node_mut(child_id).kind {
        for attr in group_attrs {
            if attr.name == "transform" && attr.prefix.is_none() {
                // Compose: prepend group transform to child transform
                if let Some(child_tf) = child_elem
                    .attributes
                    .iter_mut()
                    .find(|a| a.name == "transform" && a.prefix.is_none())
                {
                    child_tf.value = format!("{} {}", attr.value, child_tf.value);
                } else {
                    child_elem.attributes.push(attr);
                }
            } else {
                child_elem.attributes.push(attr);
            }
        }
        // Keep the group's namespace declarations in scope for the child
        // subtree; a prefix the child redeclares already wins there.
        for ns in group_ns {
            if !child_elem.namespaces.iter().any(|d| d.prefix == ns.prefix) {
                child_elem.namespaces.push(ns);
            }
        }
    }

    // Hoist child to group's parent
    if let Some(parent_id) = doc.node(group_id).parent {
        hoist_children(doc, group_id, parent_id);
        doc.node_mut(group_id).removed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn structural_css_blocks_collapsing() {
        for css in [
            "g rect{fill:red}",
            ".a rect{fill:red}",
            "#g rect{fill:red}",
            "@media print{g{opacity:1}}",
        ] {
            let input = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>{css}</style><g class=\"a\" id=\"g\"><rect/></g><g><rect/></g></svg>"
            );
            let mut doc = parse(&input).unwrap();
            assert_eq!(CollapseGroups.run(&mut doc), PassResult::Unchanged, "{css}");
            assert_eq!(serialize(&doc), input, "{css}");
        }
    }

    #[test]
    fn collapses_attr_less_group() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect/><circle r="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g>"), "group should be removed: {output}");
        assert!(output.contains("<rect/>"));
        assert!(output.contains("<circle"));
    }

    #[test]
    fn merges_single_child_attrs() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><rect width="10"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g"), "group should be removed: {output}");
        assert!(output.contains("fill=\"red\""));
        assert!(output.contains("width=\"10\""));
    }

    #[test]
    fn keeps_group_with_conflicting_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><rect fill="blue"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // Can't merge because both have `fill`
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn collapses_group_with_transform_to_child() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,10)"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(
            !output.contains("<g"),
            "group should be collapsed: {output}"
        );
        assert!(
            output.contains("transform=\"translate(10,10)\""),
            "transform should be on rect: {output}"
        );
    }

    #[test]
    fn composes_transforms_during_collapse() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,10)"><rect transform="scale(2)"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(
            !output.contains("<g"),
            "group should be collapsed: {output}"
        );
        assert!(
            output.contains("translate(10,10) scale(2)"),
            "transforms should be composed: {output}"
        );
    }

    #[test]
    fn collapses_nested_groups() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><g><rect/></g></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // First pass collapses inner, second pass collapses outer
        CollapseGroups.run(&mut doc);
        CollapseGroups.run(&mut doc);
        let output = serialize(&doc);
        assert!(
            !output.contains("<g"),
            "all groups should be removed: {output}"
        );
        assert!(output.contains("<rect/>"));
    }

    #[test]
    fn keeps_group_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g id="layer1"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // Has id attr — can't collapse (might be referenced)
        // But single child with no conflict — actually this should merge
        // The id goes to the child. Let's check: can_merge_attrs allows it since child has no id.
        let result = CollapseGroups.run(&mut doc);
        assert_eq!(result, PassResult::Changed);
    }

    #[test]
    fn keeps_group_whose_style_overrides_child_attr() {
        // Inline style beats a presentation attribute on the same element:
        // merging would turn the black path red / give the stroke-less path
        // a 3px red stroke.
        for input in [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="fill:red"><path fill="black" d="M0 0h5v5z"/></g></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="stroke:red;stroke-width:3"><path stroke="none" stroke-width="1" d="M0 5h5v5z"/></g></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="fill:red !important"><path fill="black" d="M0 0h5v5z"/></g></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="FILL:red"><path fill="black" d="M0 0h5v5z"/></g></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="transform:rotate(45deg)"><path transform="translate(1 1)" d="M0 0h5v5z"/></g></svg>"#,
        ] {
            let mut doc = parse(input).unwrap();
            assert_eq!(
                CollapseGroups.run(&mut doc),
                PassResult::Unchanged,
                "{input}"
            );
            assert_eq!(serialize(&doc), input);
        }
    }

    #[test]
    fn keeps_group_whose_attr_is_overridden_by_child_style() {
        // The symmetric case: the child's style would override the moved
        // attribute (and a composed transform attribute would be ignored).
        for input in [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><path style="fill:black" d="M0 0h5v5z"/></g></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(1 1)"><path style="transform:rotate(45deg)" d="M0 0h5v5z"/></g></svg>"#,
        ] {
            let mut doc = parse(input).unwrap();
            assert_eq!(
                CollapseGroups.run(&mut doc),
                PassResult::Unchanged,
                "{input}"
            );
        }
    }

    #[test]
    fn merges_group_style_when_child_attr_agrees() {
        // Same property, same value: moving the style changes nothing.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="fill:red"><path fill="red" d="M0 0h5v5z"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("<g"), "{output}");
        assert!(output.contains(r#"style="fill:red""#), "{output}");
    }

    // ── Reference safety tests ─────────────────────────────────────────

    #[test]
    fn clip_path_blocks_merge() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g clip-path="url(#clip1)"><rect/></g></svg>"##;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(
            output.contains("<g"),
            "group with clip-path must be preserved: {output}"
        );
    }

    #[test]
    fn mask_blocks_merge() {
        let input =
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g mask="url(#mask1)"><rect/></g></svg>"##;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(
            output.contains("<g"),
            "group with mask must be preserved: {output}"
        );
    }

    #[test]
    fn filter_blocks_merge() {
        let input =
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g filter="url(#blur)"><rect/></g></svg>"##;
        let mut doc = parse(input).unwrap();
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(
            output.contains("<g"),
            "group with filter must be preserved: {output}"
        );
    }

    #[test]
    fn opacity_single_child_merges() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g opacity="0.5"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // Single child — opacity on group is equivalent to opacity on child
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(
            !output.contains("<g"),
            "single-child opacity group should collapse: {output}"
        );
        assert!(
            output.contains("opacity=\"0.5\""),
            "opacity should be on rect: {output}"
        );
    }

    #[test]
    fn opacity_multi_child_preserved() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g opacity="0.5"><rect fill="red"/><rect fill="blue"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        // Multi-child — group opacity composites differently than per-element
        assert_eq!(CollapseGroups.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(
            output.contains("<g"),
            "multi-child opacity group must be preserved: {output}"
        );
    }

    #[test]
    fn inherited_attrs_cascade_correctly() {
        // Inner group merges stroke into rect; outer group stays (has fill attr + 2 children)
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><g stroke="blue"><rect/></g><circle r="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        CollapseGroups.run(&mut doc);
        let output = serialize(&doc);
        // Inner group collapsed: stroke moved to rect
        assert!(
            output.contains("stroke=\"blue\""),
            "stroke should be on rect: {output}"
        );
        // Outer group preserved: fill="red" inherited by both children
        assert!(
            output.contains("fill=\"red\""),
            "fill should stay on outer group: {output}"
        );
        assert!(
            output.contains("<g"),
            "outer group must be preserved: {output}"
        );
    }

    fn run(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = CollapseGroups.run(&mut doc);
        (result, serialize(&doc))
    }

    // ── <switch> children (SVGO collapseGroups.14) ─────────────────────

    #[test]
    fn switch_children_are_never_collapsed() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><switch><g id="a"><g class="i"/></g><g id="b" class="n"><g class="i"/></g><g><g/></g></switch></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn switch_branches_keep_their_children() {
        // draw.io / Illustrator pattern: each branch is an alternative.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><switch><g requiredFeatures=\"http://www.w3.org/TR/SVG11/feature#Extensibility\"><foreignObject/></g><g><rect/><circle r=\"1\"/></g></switch></svg>";
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn groups_below_switch_branches_still_collapse() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><switch><g><g><rect/></g></g></switch></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><switch><g><rect/></g></switch></svg>"#
        );
    }

    #[test]
    fn root_level_group_is_left_alone() {
        let (result, _) = run(r#"<g xmlns="http://www.w3.org/2000/svg"><rect/></g>"#);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn empty_groups_are_left_to_remove_empty_containers() {
        // SVGO: "non-empty groups" only. <g><g/></g> unwraps the outer group
        // (no attributes) but never touches the empty inner one.
        let (result, output) = run(r#"<svg xmlns="http://www.w3.org/2000/svg"><g><g/></g></svg>"#);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g></g></svg>"#
        );
        let (result, _) =
            run(r##"<svg xmlns="http://www.w3.org/2000/svg"><g filter="url(#f)"/></svg>"##);
        assert_eq!(result, PassResult::Unchanged);
    }

    // ── ids and animation (SVGO collapseGroups.12) ─────────────────────

    #[test]
    fn keeps_groups_with_id_child_or_animation_elements() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g stroke=\"#000\"><g id=\"star\"><path id=\"bar\" d=\"...\"/></g></g><g><animate id=\"frame0\" attributeName=\"visibility\" values=\"visible\" dur=\"33ms\" begin=\"0s;frame27.end\"/><path d=\"...\" fill=\"#272727\"/><path d=\"...\" fill=\"#404040\"/><path d=\"...\" fill=\"#2d2d2d\"/></g><g transform=\"rotate(-90 25 0)\"><circle stroke-dasharray=\"110\" r=\"20\" stroke=\"#10cfbd\" fill=\"none\" stroke-width=\"3\" stroke-linecap=\"round\"><animate attributeName=\"stroke-dashoffset\" values=\"360;140\" dur=\"2.2s\" keyTimes=\"0;1\" calcMode=\"spline\" fill=\"freeze\" keySplines=\"0.41,0.314,0.8,0.54\" repeatCount=\"indefinite\" begin=\"0\"/><animateTransform attributeName=\"transform\" type=\"rotate\" values=\"0;274;360\" keyTimes=\"0;0.74;1\" calcMode=\"linear\" dur=\"2.2s\" repeatCount=\"indefinite\" begin=\"0\"/></circle></g></svg>";
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn child_with_id_blocks_merge() {
        // The <use> instance would otherwise inherit the merged fill.
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><path id="x" d="M0 0"/></g><use href="#x" fill="blue"/></svg>"##;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn animated_attribute_in_child_subtree_blocks_merge() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g opacity=".5"><rect><set attributeName="opacity" to="1"/></rect></g></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
        // Deeper than a direct child, too: the attribute-less inner group
        // unwraps, the opacity group stays.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g opacity=".5"><g><rect><set attributeName="opacity" to="1"/></rect></g></g></svg>"#;
        let (_, output) = run(input);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g opacity=".5"><rect><set attributeName="opacity" to="1"/></rect></g></svg>"#
        );
        // An unrelated animated attribute does not block.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g opacity=".5"><rect><set attributeName="x" to="1"/></rect></g></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed, "{output}");
    }

    #[test]
    fn animation_child_blocks_unwrap() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g><set attributeName="opacity" to="0"/><rect/></g></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    // ── non-rendered resources (WP2-25) ─────────────────────────────────

    #[test]
    fn transform_is_not_merged_into_resource_children() {
        for child in [
            r#"<clipPath id="c"><rect width="10" height="10"/></clipPath>"#,
            r#"<mask id="c"><rect width="10" height="10"/></mask>"#,
            r#"<symbol id="c"><rect width="10" height="10"/></symbol>"#,
            r#"<linearGradient><stop offset="0"/></linearGradient>"#,
            r#"<defs><rect/></defs>"#,
            r#"<foreignObject><div/></foreignObject>"#,
        ] {
            let input = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="scale(2)">{child}</g></svg>"#
            );
            let (result, output) = run(&input);
            assert_eq!(result, PassResult::Unchanged, "{output}");
        }
    }

    #[test]
    fn merges_into_anchor_and_switch_children() {
        let (result, output) = run(
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red"><a href="#x"><rect/></a></g></svg>"##,
        );
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains(r##"<a href="#x" fill="red">"##), "{output}");
    }

    // ── style-form group-only props (SVGO collapseGroups.17/18) ────────

    #[test]
    fn style_filter_blocks_merge() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 88 88\"><filter id=\"a\"><feGaussianBlur stdDeviation=\"1\"/></filter><g transform=\"matrix(0.6875,0,0,0.6875,20.34375,66.34375)\" style=\"filter:url(#a)\"><path d=\"M 33.346591,-83.471591 L -10.744318,-36.471591 L -10.49989,-32.5\" style=\"fill-opacity:1\"/></g></svg>";
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn style_filter_on_outer_group_blocks_merge_into_inner_group() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><clipPath id=\"a\"><circle cx=\"25\" cy=\"15\" r=\"10\"/></clipPath><filter id=\"b\"><feColorMatrix type=\"saturate\"/></filter><g filter=\"url(#b)\"><g clip-path=\"url(#a)\"><circle cx=\"30\" cy=\"10\" r=\"10\" fill=\"yellow\" id=\"c1\"/></g></g><g style=\"filter:url(#b)\"><g clip-path=\"url(#a)\"><circle cx=\"20\" cy=\"10\" r=\"10\" fill=\"blue\" id=\"c2\"/></g></g><circle cx=\"25\" cy=\"15\" r=\"10\" stroke=\"black\" stroke-width=\".1\" fill=\"none\"/></svg>";
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn style_clip_path_and_mask_block_merge() {
        for style in [
            "clip-path:url(#c);filter:url(#a)",
            "mask:url(#m)",
            "clip-path: url(#c)",
            "filter:",
        ] {
            let input = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="{style}"><rect/></g></svg>"#
            );
            let (result, output) = run(&input);
            assert_eq!(result, PassResult::Unchanged, "{output}");
        }
        // Other style declarations still move to a single child.
        let (result, output) = run(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="opacity:.5"><rect/></g></svg>"#,
        );
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains(r#"<rect style="opacity:.5"/>"#), "{output}");
    }

    #[test]
    fn css_transform_blocks_transform_composition() {
        let (result, output) = run(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="scale(2)"><rect style="transform:rotate(45deg)"/></g></svg>"#,
        );
        assert_eq!(result, PassResult::Unchanged, "{output}");
        let (result, output) = run(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="transform:rotate(45deg)"><rect transform="scale(2)"/></g></svg>"#,
        );
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    // ── namespace declarations (WP2-27) ────────────────────────────────

    #[test]
    fn namespace_declaration_moves_to_single_child() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="#a"/></g><rect id="a" width="1" height="1"/></svg>"##;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert!(
            output
                .contains(r##"<use xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="#a"/>"##),
            "{output}"
        );
        assert!(!output.contains("<g"), "{output}");
        // Same with a presentation attribute on the group (merge path).
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g fill="red" xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="#a"/></g></svg>"##;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains(
                r##"<use xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="#a" fill="red"/>"##
            ),
            "{output}"
        );
    }

    #[test]
    fn namespace_declaration_keeps_multi_child_group() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><g xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="#a"/><use xlink:href="#b"/></g></svg>"##;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn child_redeclaration_keeps_its_own_uri() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g xmlns:foo="urn:a"><rect xmlns:foo="urn:b" foo:bar="1"/></g></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains(r#"<rect xmlns:foo="urn:b" foo:bar="1"/>"#),
            "{output}"
        );
        assert!(!output.contains("urn:a"), "{output}");
    }

    #[test]
    fn redundant_namespace_redeclaration_does_not_block_unwrap() {
        // The prefix already resolves to the same URI on an ancestor.
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><g xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="#a"/><use xlink:href="#b"/></g></svg>"##;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<g"), "{output}");
        // Redundant default namespace too.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g xmlns="http://www.w3.org/2000/svg"><rect/><circle r="1"/></g></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/><circle r="1"/></svg>"#
        );
        // A re-binding to a different URI is not redundant.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:foo="urn:a"><g xmlns:foo="urn:b"><rect foo:x="1"/><circle r="1"/></g></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn group_declaring_used_namespace_is_kept() {
        // The declaration scopes the foo:* descendants; neither unwrapping
        // (no attributes) nor merging into a single child may drop it.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g xmlns:foo="urn:foo"><foo:bar/><rect foo:attr="1"/></g><g xmlns:foo="urn:foo" fill="red"><rect foo:attr="1"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        CollapseGroups.run(&mut doc);
        let output = serialize(&doc);
        // Multi-child group: kept as the declaration's scope.
        assert!(
            output.contains(r#"<g xmlns:foo="urn:foo"><foo:bar/><rect foo:attr="1"/></g>"#),
            "{output}"
        );
        // Single-child group: may be merged, but the declaration must travel
        // with the child so every used prefix stays declared.
        assert_eq!(
            output.matches("xmlns:foo=\"urn:foo\"").count(),
            2,
            "{output}"
        );
        assert_eq!(output.matches("foo:attr=\"1\"").count(), 2, "{output}");
        assert!(output.contains("fill=\"red\""), "{output}");
    }

    #[test]
    fn group_with_unused_namespace_is_collapsed_after_cleanup() {
        // removeUnusedNamespaces strips the unused declaration first, then
        // the group collapses as usual.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g xmlns:foo="urn:foo"><rect width="1" height="1"/></g></svg>"#;
        let out = crate::optimize(input).unwrap().data;
        assert!(!out.contains("<g"), "{out}");
        assert!(!out.contains("xmlns:foo"), "{out}");
    }
}
