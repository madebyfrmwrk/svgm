use super::{Pass, PassResult};
use crate::ast::{Document, Element, NodeId, NodeKind};

/// Removes elements inside `<defs>` that have no `id` and are not `<style>`.
///
/// Port of SVGO's `collectUsefulNodes` (removeUselessDefs.js, svg/svgo#1923):
/// the search is recursive, so id-less wrappers such as `<g>` or a nested
/// `<defs>` are unwrapped and the useful elements inside them are hoisted
/// into the enclosing container. Going beyond SVGO for safety, a wrapper that
/// carries attributes which could affect its descendants (presentation
/// attributes, `transform`, `style`, `class`, ...) is kept as a unit instead
/// of being unwrapped; only its useless descendants are pruned. If a `<defs>`
/// ends up with no useful content it is removed entirely.
pub struct RemoveUselessDefs;

impl Pass for RemoveUselessDefs {
    fn name(&self) -> &'static str {
        "removeUselessDefs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;

        for id in doc.traverse() {
            if !is_elem(doc, id, "defs") {
                continue;
            }
            let (pruned, useful) = prune_container(doc, id);
            changed |= pruned;
            if useful == 0 {
                doc.remove(id);
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

fn element(doc: &Document, id: NodeId) -> Option<&Element> {
    match &doc.node(id).kind {
        NodeKind::Element(e) => Some(e),
        _ => None,
    }
}

fn is_elem(doc: &Document, id: NodeId, name: &str) -> bool {
    element(doc, id).is_some_and(|e| e.prefix.is_none() && e.name == name)
}

/// SVGO's notion of a useful node inside `<defs>`: it can be referenced (has
/// an `id`) or it is a stylesheet.
fn is_useful(e: &Element) -> bool {
    (e.name == "style" && e.prefix.is_none()) || e.attr("id").is_some()
}

/// True when an id-less wrapper can be unwrapped without changing what its
/// descendants inherit or how they are positioned. Any unprefixed attribute
/// (except `data-*`) or an `xml:` attribute (`xml:space`, `xml:lang`) is
/// treated as significant; editor namespaces (`inkscape:label`, ...) are not.
fn is_unwrappable(e: &Element) -> bool {
    e.attributes.iter().all(|a| match &a.prefix {
        None => a.name.starts_with("data-"),
        Some(p) => p != "xml",
    })
}

/// Rewrite `container`'s children to the useful elements among its
/// descendants, in document order. Returns `(changed, useful_count)`.
///
/// For each element child: a useful one is kept as is (its subtree is not
/// inspected, as in SVGO); an unwrappable wrapper is replaced by its useful
/// descendants (found recursively); any other wrapper is pruned recursively
/// and kept only if something useful remains inside it. Non-element children
/// of `container` are left alone; those inside an unwrapped wrapper go away
/// with it.
fn prune_container(doc: &mut Document, container: NodeId) -> (bool, usize) {
    let mut changed = false;
    let mut useful = 0usize;
    let mut i = 0;
    while i < doc.node(container).children.len() {
        let child = doc.node(container).children[i];
        if doc.node(child).removed {
            i += 1;
            continue;
        }
        let Some(e) = element(doc, child) else {
            i += 1;
            continue;
        };
        if is_useful(e) {
            useful += 1;
            i += 1;
            continue;
        }
        if is_unwrappable(e) {
            // Flatten the wrapper first so its direct children are exactly
            // its useful descendants, then hoist them in front of it.
            let (_, inner) = prune_container(doc, child);
            let kids: Vec<NodeId> = doc.children(child).collect();
            let mut insert_at = i;
            for kid in kids {
                if element(doc, kid).is_some() {
                    doc.reparent(kid, container, insert_at);
                    insert_at += 1;
                } else {
                    doc.remove(kid);
                }
            }
            doc.remove_shallow(child);
            useful += inner;
            changed = true;
            i = insert_at + 1;
            continue;
        }
        let (pruned, inner) = prune_container(doc, child);
        if inner == 0 {
            doc.remove(child);
            changed = true;
        } else {
            useful += 1;
            changed |= pruned;
        }
        i += 1;
    }
    (changed, useful)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = RemoveUselessDefs.run(&mut doc);
        (result, serialize(&doc))
    }

    #[test]
    fn removes_defs_child_without_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient><stop offset="0"/></linearGradient></defs><rect/></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("defs"));
        assert!(!output.contains("linearGradient"));
    }

    #[test]
    fn keeps_defs_child_with_id() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="g1"><stop offset="0"/></linearGradient></defs><rect/></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("linearGradient"));
    }

    #[test]
    fn keeps_style_in_defs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><style>.a{fill:red}</style></defs><rect/></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("style"));
    }

    #[test]
    fn removes_empty_defs_after_cleanup() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><clipPath/></defs><rect/></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("defs"));
    }

    #[test]
    fn unchanged_when_no_defs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#;
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // SVGO removeUselessDefs.01: id-less <g> wrapper is unwrapped and the
    // useful path hoisted into <defs>.
    #[test]
    fn svgo_01_unwraps_idless_group() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><path d="..."/><g><path d="..." id="a"/></g></defs></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><path d="..." id="a"/></defs></svg>"#
        );
    }

    // SVGO removeUselessDefs.04/.05: a non-rendering element or group outside
    // <defs> is not this pass's business, even without an id.
    #[test]
    fn svgo_04_05_ignores_wrappers_outside_defs() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="url(#a)" width="64" height="64"/><symbol><linearGradient id="a"><stop offset="5%" stop-color="gold"/></linearGradient></symbol><g><linearGradient id="b"><stop/></linearGradient></g></svg>"##;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }

    #[test]
    fn nested_defs_are_flattened() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><defs><linearGradient id="g"><stop/></linearGradient></defs></defs><rect fill="url(#g)"/></svg>"##;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="g"><stop/></linearGradient></defs><rect fill="url(#g)"/></svg>"##
        );
    }

    #[test]
    fn referenced_path_inside_group_survives() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><g><path id="p" d="M0 0h1"/></g></defs><use href="#p"/></svg>"##;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><path id="p" d="M0 0h1"/></defs><use href="#p"/></svg>"##
        );
    }

    #[test]
    fn style_and_wrapped_clip_path_kept_in_order() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><style>.a{fill:red}</style><g><clipPath id="c"><rect/></clipPath></g></defs><rect clip-path="url(#c)"/></svg>"##;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><style>.a{fill:red}</style><clipPath id="c"><rect/></clipPath></defs><rect clip-path="url(#c)"/></svg>"##
        );
    }

    #[test]
    fn hoists_several_levels_and_keeps_document_order() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><g><path id="a"/><g><path/><path id="b"/></g><path/></g><path id="c"/></defs></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><path id="a"/><path id="b"/><path id="c"/></defs></svg>"#
        );
    }

    #[test]
    fn whitespace_inside_unwrapped_group_is_dropped_but_defs_text_kept() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>\n  <g>\n    <path id=\"a\"/>\n  </g>\n</defs></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>\n  <path id=\"a\"/>\n</defs></svg>"
        );
    }

    #[test]
    fn wrapper_with_presentation_attrs_is_kept_not_hoisted() {
        // clip-rule on the group is inherited by the clipPath's children, so
        // unwrapping would change the clip. Only the useless sibling goes.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><g clip-rule="evenodd"><clipPath id="c"><path/></clipPath><path/></g></defs></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><g clip-rule="evenodd"><clipPath id="c"><path/></clipPath></g></defs></svg>"#
        );
    }

    #[test]
    fn wrapper_with_transform_is_kept() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><g transform="scale(2)"><path id="a"/></g></defs></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }

    #[test]
    fn wrapper_with_attrs_but_nothing_useful_is_removed() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><g fill="red"><path/></g><linearGradient id="g"/></defs></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="g"></linearGradient></defs></svg>"#
        );
    }

    #[test]
    fn editor_attrs_do_not_block_unwrapping() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"><defs><g inkscape:label="Layer" data-name="x"><path id="a"/></g></defs></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains(r#"<defs><path id="a"/></defs>"#),
            "{output}"
        );
    }

    #[test]
    fn useful_nodes_are_not_pruned_inside() {
        // SVGO does not look inside useful nodes: the id-less path inside
        // <g id="x"> stays.
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><g id="x"><path/></g></defs></svg>"#;
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }

    #[test]
    fn full_optimizer_keeps_wrapped_gradient() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs><g><linearGradient id="grad"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></g></defs><rect width="10" height="10" fill="url(#grad)"/></svg>"##;
        let out = crate::optimize(input).unwrap().data;
        assert!(out.contains("<linearGradient id=\"a\">"), "{out}");
        assert!(out.contains("fill=\"url(#a)\""), "{out}");
        let again = crate::optimize(&out).unwrap().data;
        assert_eq!(again, out);
    }
}
