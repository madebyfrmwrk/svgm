use super::{Pass, PassResult};
use crate::ast::{Document, NodeId, NodeKind};

pub struct MergePaths;

impl Pass for MergePaths {
    fn name(&self) -> &'static str {
        "mergePaths"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        // Collect parent IDs that have children (deduplicated, in traversal order)
        let mut parents_seen = std::collections::HashSet::new();
        let mut parents: Vec<NodeId> = Vec::new();
        for &id in &ids {
            if let Some(parent_id) = doc.node(id).parent
                && parents_seen.insert(parent_id)
            {
                parents.push(parent_id);
            }
        }

        for parent_id in parents {
            if merge_adjacent_paths(doc, parent_id) {
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

/// Attributes that block merging — they have semantics that change when paths
/// are concatenated into a compound path.
const BLOCKING_ATTRS: &[&str] = &[
    "id",
    "marker-start",
    "marker-mid",
    "marker-end",
    "fill-rule",  // evenodd + overlapping paths = different rendering
    "clip-rule",  // same issue as fill-rule
];

/// Try to merge adjacent <path> siblings under the given parent.
/// Returns true if any merge was performed.
fn merge_adjacent_paths(doc: &mut Document, parent_id: NodeId) -> bool {
    let mut changed = false;

    loop {
        let children: Vec<NodeId> = doc.children(parent_id).collect();
        let mut merged_any = false;

        let mut i = 0;
        while i < children.len() {
            // Find a run of mergeable paths starting at i
            if !is_mergeable_path(doc, children[i]) {
                i += 1;
                continue;
            }

            let run_start = i;
            let mut run_end = i + 1;

            while run_end < children.len() {
                // Skip whitespace-only text nodes between paths
                if is_whitespace_text(doc, children[run_end]) {
                    run_end += 1;
                    continue;
                }

                if is_mergeable_path(doc, children[run_end])
                    && attrs_match(doc, children[run_start], children[run_end])
                {
                    run_end += 1;
                } else {
                    break;
                }
            }

            // Collect the actual path IDs in this run (skip whitespace nodes)
            let path_ids: Vec<NodeId> = children[run_start..run_end]
                .iter()
                .copied()
                .filter(|&id| is_mergeable_path(doc, id))
                .collect();

            if path_ids.len() >= 2 {
                // Merge: concatenate all d values into the first path
                let mut combined_d = String::new();
                for (j, &path_id) in path_ids.iter().enumerate() {
                    if let NodeKind::Element(ref elem) = doc.node(path_id).kind
                        && let Some(d) = elem.attr("d")
                    {
                        if j > 0 {
                            // Separator between path data — not strictly needed if
                            // first path ends with z and second starts with M, but
                            // a space is safe and cheap
                            if !combined_d.is_empty()
                                && !combined_d.ends_with(' ')
                                && !combined_d.ends_with('z')
                                && !combined_d.ends_with('Z')
                            {
                                combined_d.push(' ');
                            }
                        }
                        combined_d.push_str(d);
                    }
                }

                // Update first path's d attribute
                let first_id = path_ids[0];
                if let NodeKind::Element(ref mut elem) = doc.node_mut(first_id).kind
                    && let Some(d_attr) = elem
                        .attributes
                        .iter_mut()
                        .find(|a| a.name == "d" && a.prefix.is_none())
                {
                    d_attr.value = combined_d;
                }

                // Remove merged paths (all except the first)
                for &path_id in &path_ids[1..] {
                    doc.remove(path_id);
                }

                // Also remove whitespace text nodes that were between the merged paths
                for &child_id in &children[run_start..run_end] {
                    if is_whitespace_text(doc, child_id) {
                        doc.remove(child_id);
                    }
                }

                merged_any = true;
                changed = true;
            }

            i = run_end;
        }

        if !merged_any {
            break;
        }
    }

    changed
}

/// Check if a node is a <path> element that can participate in merging.
fn is_mergeable_path(doc: &Document, id: NodeId) -> bool {
    let node = doc.node(id);
    if node.removed {
        return false;
    }

    let elem = match &node.kind {
        NodeKind::Element(e) if e.name == "path" => e,
        _ => return false,
    };

    // Must have a d attribute
    if elem.attr("d").is_none() {
        return false;
    }

    // Block if it has any attributes that make merging unsafe
    for attr in &elem.attributes {
        if attr.prefix.is_none() && BLOCKING_ATTRS.contains(&attr.name.as_str()) {
            return false;
        }
    }

    // Block if it has animation children
    for child_id in doc.children(id) {
        if let NodeKind::Element(ref child_elem) = doc.node(child_id).kind {
            match child_elem.name.as_str() {
                "animate" | "animateTransform" | "animateMotion" | "set" => return false,
                _ => {}
            }
        }
    }

    true
}

/// Check if all attributes except `d` are identical between two path elements.
fn attrs_match(doc: &Document, a: NodeId, b: NodeId) -> bool {
    let elem_a = match &doc.node(a).kind {
        NodeKind::Element(e) => e,
        _ => return false,
    };
    let elem_b = match &doc.node(b).kind {
        NodeKind::Element(e) => e,
        _ => return false,
    };

    // Collect non-d attributes from each
    let attrs_a: Vec<(&Option<String>, &str, &str)> = elem_a
        .attributes
        .iter()
        .filter(|a| !(a.name == "d" && a.prefix.is_none()))
        .map(|a| (&a.prefix, a.name.as_str(), a.value.as_str()))
        .collect();

    let attrs_b: Vec<(&Option<String>, &str, &str)> = elem_b
        .attributes
        .iter()
        .filter(|a| !(a.name == "d" && a.prefix.is_none()))
        .map(|a| (&a.prefix, a.name.as_str(), a.value.as_str()))
        .collect();

    if attrs_a.len() != attrs_b.len() {
        return false;
    }

    // Every attr in A must have an exact match in B (order-independent)
    for &(prefix_a, name_a, value_a) in &attrs_a {
        if !attrs_b
            .iter()
            .any(|&(prefix_b, name_b, value_b)| {
                prefix_a == prefix_b && name_a == name_b && value_a == value_b
            })
        {
            return false;
        }
    }

    true
}

/// Check if a node is a whitespace-only text node.
fn is_whitespace_text(doc: &Document, id: NodeId) -> bool {
    if doc.node(id).removed {
        return false;
    }
    matches!(&doc.node(id).kind, NodeKind::Text(t) if t.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::passes::PassResult;
    use crate::serializer::serialize;

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = MergePaths.run(&mut doc);
        (result, serialize(&doc))
    }

    // --- Basic merging ---

    #[test]
    fn merges_adjacent_paths_same_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // Should be one path with concatenated d
        assert_eq!(output.matches("<path").count(), 1);
        assert!(output.contains("M0 0L10 10") && output.contains("M20 20L30 30"), "expected merged d, got: {output}");
    }

    #[test]
    fn merges_three_adjacent_paths() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"red\"/><path d=\"M40 40L50 50\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("<path").count(), 1);
    }

    #[test]
    fn no_merge_different_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"blue\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_single_path() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Blocking attributes ---

    #[test]
    fn no_merge_with_id() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\" id=\"a\"/><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_fill_rule() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\" fill-rule=\"evenodd\"/><path d=\"M20 20L30 30\" fill=\"red\" fill-rule=\"evenodd\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_markers() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\" marker-end=\"url(#arrow)\"/><path d=\"M20 20L30 30\" fill=\"red\" marker-end=\"url(#arrow)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_animation_child() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"><animate attributeName=\"d\" to=\"M0 0L20 20\" dur=\"1s\"/></path><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Whitespace handling ---

    #[test]
    fn merges_paths_with_whitespace_between() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/>  \n  <path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("<path").count(), 1);
    }

    // --- Non-adjacent paths ---

    #[test]
    fn no_merge_non_adjacent() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><rect width=\"10\" height=\"10\"/><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Attribute order independence ---

    #[test]
    fn merges_with_different_attr_order() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\" stroke=\"black\"/><path d=\"M20 20L30 30\" stroke=\"black\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("<path").count(), 1);
    }

    // --- Integration ---

    #[test]
    fn full_optimizer_convergence() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let result1 = crate::optimize(input).unwrap();
        let result2 = crate::optimize(&result1.data).unwrap();
        assert_eq!(result1.data, result2.data, "should converge");
    }
}
