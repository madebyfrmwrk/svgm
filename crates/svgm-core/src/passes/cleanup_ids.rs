use super::{Pass, PassResult, inside_foreign_object, is_css_ident_char};
use crate::ast::{Document, NodeId, NodeKind};
use std::collections::{HashMap, HashSet};

pub struct CleanupIds;

impl Pass for CleanupIds {
    fn name(&self) -> &'static str {
        "cleanupIds"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let ids = doc.traverse();

        // Elements inside <foreignObject> are HTML: their ids are neither
        // removed nor renamed (SVGO skips the subtree).
        let foreign: HashSet<NodeId> = ids
            .iter()
            .copied()
            .filter(|&id| inside_foreign_object(doc, id))
            .collect();

        // Phase 1: Collect all ID declarations. Ids of foreign elements are
        // left alone but reserved, so that no SVG id is renamed onto one of
        // them (`url(#a)` would then resolve to the HTML element).
        let mut declarations: HashMap<String, NodeId> = HashMap::new();
        let mut reserved: HashSet<String> = HashSet::new();
        for &id in &ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind
                && let Some(id_val) = elem.attr("id")
            {
                if foreign.contains(&id) {
                    reserved.insert(id_val.to_string());
                } else {
                    declarations.insert(id_val.to_string(), id);
                }
            }
        }

        if declarations.is_empty() {
            return PassResult::Unchanged;
        }

        let declared_ids: HashSet<String> = declarations.keys().cloned().collect();

        // Phase 2: Collect all references to IDs
        let mut referenced: HashSet<String> = HashSet::new();

        for &id in &ids {
            let node = doc.node(id);
            match &node.kind {
                NodeKind::Element(elem) => {
                    for attr in &elem.attributes {
                        // url(#id) references in any attribute
                        for r in extract_url_refs(&attr.value) {
                            if declared_ids.contains(&r) {
                                referenced.insert(r);
                            }
                        }

                        // href="#id" or xlink:href="#id"
                        if attr.name == "href"
                            && let Some(r) = extract_href_ref(&attr.value)
                            && declared_ids.contains(r)
                        {
                            referenced.insert(r.to_string());
                        }

                        // SMIL timing: begin="id.click", end="id.end+2s"
                        if attr.name == "begin" || attr.name == "end" {
                            for r in extract_smil_refs(&attr.value, &declared_ids) {
                                referenced.insert(r);
                            }
                        }
                    }
                }
                NodeKind::Text(text) | NodeKind::CData(text) => {
                    // Check if inside <style> element
                    if let Some(parent_id) = node.parent
                        && let NodeKind::Element(ref parent_elem) = doc.node(parent_id).kind
                        && parent_elem.name == "style"
                    {
                        for r in extract_css_id_refs(text, &declared_ids) {
                            referenced.insert(r);
                        }
                    }
                }
                _ => {}
            }
        }

        // Phase 3: Build rename map for referenced IDs
        // Sort by traversal order for deterministic naming
        let mut referenced_ids: Vec<(&String, &NodeId)> = declarations
            .iter()
            .filter(|(name, _)| referenced.contains(name.as_str()))
            .collect();
        referenced_ids.sort_by_key(|(_, node_id)| node_id.0);

        let mut generator = IdGenerator::new();
        let mut rename_map: HashMap<String, String> = HashMap::new();

        for (old_name, _) in &referenced_ids {
            let mut new_name = generator.next_id();
            while reserved.contains(&new_name) {
                new_name = generator.next_id();
            }
            if new_name != **old_name {
                rename_map.insert((*old_name).clone(), new_name);
            }
        }

        // Phase 4: Check if there's anything to do
        let unreferenced_count = declarations.len() - referenced.len();
        if unreferenced_count == 0 && rename_map.is_empty() {
            return PassResult::Unchanged;
        }

        // Phase 5: Apply changes
        let mut changed = false;

        for &id in &ids {
            let is_foreign = foreign.contains(&id);
            let node = doc.node_mut(id);
            match &mut node.kind {
                NodeKind::Element(elem) => {
                    // Remove unreferenced IDs or rename referenced ones
                    if !is_foreign
                        && let Some(id_attr_pos) = elem
                            .attributes
                            .iter()
                            .position(|a| a.name == "id" && a.prefix.is_none())
                    {
                        let id_val = elem.attributes[id_attr_pos].value.clone();
                        if !referenced.contains(&id_val) {
                            // Unreferenced — remove
                            elem.attributes.remove(id_attr_pos);
                            changed = true;
                        } else if let Some(new_name) = rename_map.get(&id_val) {
                            // Referenced — shorten
                            elem.attributes[id_attr_pos].value = new_name.clone();
                            changed = true;
                        }
                    }

                    // Update references in attributes
                    for attr in &mut elem.attributes {
                        // url(#id) references, quoted or not
                        if attr.value.contains("url(") {
                            let new_val = replace_url_refs(&attr.value, &rename_map);
                            if new_val != attr.value {
                                attr.value = new_val;
                                changed = true;
                            }
                        }

                        // href="#id"
                        if attr.name == "href" && attr.value.starts_with('#') {
                            let old_ref = &attr.value[1..];
                            if let Some(new_name) = rename_map.get(old_ref) {
                                attr.value = format!("#{new_name}");
                                changed = true;
                            }
                        }

                        // SMIL timing
                        if attr.name == "begin" || attr.name == "end" {
                            let new_val = replace_smil_refs(&attr.value, &rename_map);
                            if new_val != attr.value {
                                attr.value = new_val;
                                changed = true;
                            }
                        }
                    }
                }
                NodeKind::Text(text) | NodeKind::CData(text) => {
                    // Update CSS #id references in <style> text nodes
                    if let Some(parent_id) = node.parent {
                        // Can't borrow doc again here, so check parent via ids list
                        // We stored style parents during collect. Use a simpler approach:
                        // just try replacement on all text nodes — replace_css_ids is
                        // a no-op if no matches.
                        let _ = parent_id; // used for context only
                        let new_text = replace_css_ids(text, &rename_map, &declared_ids);
                        if new_text != *text {
                            *text = new_text;
                            changed = true;
                        }
                    }
                }
                _ => {}
            }
        }

        if changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

// --- Reference extraction helpers ---

/// Split a `url(...)` argument into `(prefix, id, suffix)` where the prefix
/// is the optional whitespace and quote before `#`, and the suffix the
/// matching closing quote and whitespace. `None` when it is not a `#id`
/// reference (`url(data:...)`, `url(file.svg#x)`).
fn split_url_arg(arg: &str) -> Option<(&str, &str, &str)> {
    let trimmed_start = arg.trim_start();
    let lead = arg.len() - trimmed_start.len();
    let (quote, body) = match trimmed_start.as_bytes().first() {
        Some(&q @ (b'\'' | b'"')) => (Some(q), &trimmed_start[1..]),
        _ => (None, trimmed_start),
    };
    let body = body.strip_prefix('#')?;
    let id_end = match quote {
        Some(q) => body.find(q as char)?,
        None => body.trim_end().len(),
    };
    let id = &body[..id_end];
    if id.is_empty() {
        return None;
    }
    let prefix_len = lead + usize::from(quote.is_some()) + 1;
    Some((&arg[..prefix_len], id, &arg[prefix_len + id_end..]))
}

/// Extract all IDs from `url(#id)` / `url('#id')` / `url("#id")` patterns
/// in an attribute value.
fn extract_url_refs(value: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut search = value;
    while let Some(start) = search.find("url(") {
        let rest = &search[start + 4..];
        if let Some(end) = rest.find(')')
            && let Some((_, id, _)) = split_url_arg(&rest[..end])
        {
            refs.push(id.to_string());
        }
        search = rest;
    }
    refs
}

/// Extract an ID from an href="#id" value.
fn extract_href_ref(value: &str) -> Option<&str> {
    let id = value.strip_prefix('#')?;
    if id.is_empty() { None } else { Some(id) }
}

/// Extract IDs from SMIL timing values like "id.click", "id.end+2s".
fn extract_smil_refs(value: &str, known_ids: &HashSet<String>) -> Vec<String> {
    let mut refs = Vec::new();
    for part in value.split(';') {
        let trimmed = part.trim();
        if let Some(dot_pos) = trimmed.find('.') {
            let candidate = &trimmed[..dot_pos];
            if known_ids.contains(candidate) {
                refs.push(candidate.to_string());
            }
        }
    }
    refs
}

/// Byte length of the CSS identifier starting at `from` (zero when none).
/// Identifiers may contain any non-ASCII character (`#ǎx`).
fn css_ident_len(text: &str, from: usize) -> usize {
    text[from..]
        .char_indices()
        .find(|&(_, c)| !is_css_ident_char(c))
        .map_or(text.len() - from, |(i, _)| i)
}

/// Extract IDs referenced as CSS selectors (#id) in <style> text.
/// Conservative: checks against known IDs to avoid false positives with hex colors.
fn extract_css_id_refs(text: &str, known_ids: &HashSet<String>) -> Vec<String> {
    let mut refs = Vec::new();
    let mut rest = text;
    while let Some(hash) = rest.find('#') {
        let start = hash + 1;
        let end = start + css_ident_len(rest, start);
        if end > start && known_ids.contains(&rest[start..end]) {
            refs.push(rest[start..end].to_string());
        }
        rest = &rest[end.max(start)..];
    }
    refs
}

// --- Reference replacement helpers ---

/// Replace `url(#old_id)` (also quoted: `url('#old_id')`) with the renamed
/// id in an attribute value, keeping the quoting as written.
fn replace_url_refs(value: &str, rename_map: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(value.len());
    let mut search = value;
    while let Some(start) = search.find("url(") {
        result.push_str(&search[..start + 4]);
        let rest = &search[start + 4..];
        let Some(end) = rest.find(')') else {
            result.push_str(rest);
            return result;
        };
        match split_url_arg(&rest[..end]) {
            Some((prefix, id, suffix)) => {
                result.push_str(prefix);
                result.push_str(rename_map.get(id).map_or(id, String::as_str));
                result.push_str(suffix);
            }
            None => result.push_str(&rest[..end]),
        }
        search = &rest[end..];
    }
    result.push_str(search);
    result
}

/// Replace SMIL timing references: "old_id.click" -> "new_id.click".
fn replace_smil_refs(value: &str, rename_map: &HashMap<String, String>) -> String {
    let parts: Vec<&str> = value.split(';').collect();
    let mut new_parts = Vec::new();
    let mut any_changed = false;

    for part in parts {
        let trimmed = part.trim();
        if let Some(dot_pos) = trimmed.find('.') {
            let candidate = &trimmed[..dot_pos];
            if let Some(new_name) = rename_map.get(candidate) {
                new_parts.push(format!("{new_name}{}", &trimmed[dot_pos..]));
                any_changed = true;
                continue;
            }
        }
        new_parts.push(trimmed.to_string());
    }

    if any_changed {
        new_parts.join(";")
    } else {
        value.to_string()
    }
}

/// Replace #old_id with #new_id in CSS text, only for known declared IDs.
fn replace_css_ids(
    text: &str,
    rename_map: &HashMap<String, String>,
    declared_ids: &HashSet<String>,
) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(hash) = rest.find('#') {
        result.push_str(&rest[..=hash]);
        let start = hash + 1;
        let end = start + css_ident_len(rest, start);
        let candidate = &rest[start..end];
        match rename_map.get(candidate) {
            Some(new_name) => result.push_str(new_name),
            // A known but unrenamed id, or not an id at all (hex colour):
            // keep the text as-is.
            None => {
                let _ = declared_ids;
                result.push_str(candidate);
            }
        }
        rest = &rest[end..];
    }
    result.push_str(rest);

    result
}

// --- ID generator ---

struct IdGenerator {
    counter: usize,
}

impl IdGenerator {
    fn new() -> Self {
        Self { counter: 0 }
    }

    fn next_id(&mut self) -> String {
        let name = encode(self.counter);
        self.counter += 1;
        name
    }
}

/// Bijective base-26 encoding: 0=a, 1=b, ..., 25=z, 26=aa, 27=ab, ..., 51=az, 52=ba, ...
fn encode(mut n: usize) -> String {
    let mut result = Vec::new();
    loop {
        result.push(b'a' + (n % 26) as u8);
        n /= 26;
        if n == 0 {
            break;
        }
        n -= 1;
    }
    result.reverse();
    String::from_utf8(result).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = CleanupIds.run(&mut doc);
        (result, serialize(&doc))
    }

    // --- ID generator ---

    #[test]
    fn encode_sequence() {
        assert_eq!(encode(0), "a");
        assert_eq!(encode(1), "b");
        assert_eq!(encode(25), "z");
        assert_eq!(encode(26), "aa");
        assert_eq!(encode(27), "ab");
        assert_eq!(encode(51), "az");
        assert_eq!(encode(52), "ba");
        assert_eq!(encode(701), "zz");
        assert_eq!(encode(702), "aaa");
    }

    // --- Unreferenced ID removal ---

    #[test]
    fn removes_unreferenced_id() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"unused\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("id="));
        assert!(!output.contains("unused"));
    }

    #[test]
    fn no_change_without_ids() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- url(#id) references ---

    #[test]
    fn shortens_url_referenced_id() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><clipPath id=\"clip0_7441_19649\"><rect width=\"10\" height=\"10\"/></clipPath></defs><rect clip-path=\"url(#clip0_7441_19649)\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"a\""));
        assert!(output.contains("url(#a)"));
        assert!(!output.contains("clip0_7441_19649"));
    }

    #[test]
    fn preserves_and_shortens_multiple_ids() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><clipPath id=\"clip1\"><rect width=\"10\" height=\"10\"/></clipPath><clipPath id=\"clip2\"><rect width=\"20\" height=\"20\"/></clipPath><clipPath id=\"unused\"><rect width=\"5\" height=\"5\"/></clipPath></defs><rect clip-path=\"url(#clip1)\"/><rect clip-path=\"url(#clip2)\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"a\""));
        assert!(output.contains("id=\"b\""));
        assert!(output.contains("url(#a)"));
        assert!(output.contains("url(#b)"));
        // "unused" ID should be removed
        assert!(!output.contains("unused"));
    }

    // --- href references ---

    #[test]
    fn shortens_href_reference() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><defs><symbol id=\"icon\" viewBox=\"0 0 24 24\"><path d=\"M0 0\"/></symbol></defs><use xlink:href=\"#icon\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"a\""));
        assert!(output.contains("href=\"#a\""));
        assert!(!output.contains("icon"));
    }

    #[test]
    fn shortens_plain_href() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><symbol id=\"longname\" viewBox=\"0 0 24 24\"><path d=\"M0 0\"/></symbol></defs><use href=\"#longname\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"a\""));
        assert!(output.contains("href=\"#a\""));
    }

    // --- SMIL timing references ---

    #[test]
    fn shortens_smil_timing_reference() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"target\" width=\"10\" height=\"10\"/><animate begin=\"target.click\" attributeName=\"opacity\" to=\"0\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"a\""));
        assert!(output.contains("begin=\"a.click\""));
    }

    // --- CSS style references ---

    #[test]
    fn shortens_css_id_reference() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>#myid{opacity:.5}</style><rect id=\"myid\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"a\""));
        assert!(output.contains("#a{"));
    }

    // --- Edge cases ---

    #[test]
    fn already_short_id_no_change() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><clipPath id=\"a\"><rect width=\"10\" height=\"10\"/></clipPath></defs><rect clip-path=\"url(#a)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn quoted_url_references_are_renamed() {
        // SVGO cleanupIds.01: fill="url('#gradient001')"
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"gradient001\"/><linearGradient id=\"gradient002\"/><linearGradient id=\"gradient003\"/></defs><rect fill=\"url('#gradient001')\"/><rect fill=\"url(&quot;#gradient002&quot;)\"/><rect fill=\"url( #gradient003 )\"/><rect fill=\"url(data:image/png;base64,#notanid)\"/></svg>";
        let (_, output) = run_pass(input);
        assert!(output.contains("fill=\"url('#a')\""), "{output}");
        assert!(output.contains("fill=\"url(&quot;#b&quot;)\""), "{output}");
        assert!(output.contains("fill=\"url( #c )\""), "{output}");
        assert!(
            output.contains("url(data:image/png;base64,#notanid)"),
            "{output}"
        );
        assert!(!output.contains("gradient00"), "{output}");
    }

    #[test]
    fn split_url_arg_shapes() {
        assert_eq!(split_url_arg("#x"), Some(("#", "x", "")));
        assert_eq!(split_url_arg("'#x'"), Some(("'#", "x", "'")));
        assert_eq!(split_url_arg(" \"#x\" "), Some((" \"#", "x", "\" ")));
        assert_eq!(split_url_arg("data:x"), None);
        assert_eq!(split_url_arg("'#x"), None);
        assert_eq!(split_url_arg("#"), None);
    }

    #[test]
    fn multiple_url_refs_in_single_attr() {
        // This is rare but possible in CSS shorthand-like values
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"grad1\"><stop offset=\"0\" stop-color=\"red\"/></linearGradient><linearGradient id=\"grad2\"><stop offset=\"0\" stop-color=\"blue\"/></linearGradient></defs><rect fill=\"url(#grad1)\" stroke=\"url(#grad2)\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("url(#a)"));
        assert!(output.contains("url(#b)"));
    }

    #[test]
    fn shortens_non_ascii_css_id_reference() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>#ǎx{opacity:.5}</style><rect id=\"ǎx\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"a\""), "{output}");
        assert!(output.contains("#a{opacity:.5}"), "{output}");
        assert!(!output.contains("ǎx"), "{output}");
    }

    #[test]
    fn css_id_prefix_of_longer_ident_is_not_a_reference() {
        // `#ǎxy` is a different identifier from `#ǎx`.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>#ǎxy{opacity:.5}</style><rect id=\"ǎx\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("id="), "{output}");
        assert!(output.contains("#ǎxy{opacity:.5}"), "{output}");
    }

    #[test]
    fn ids_inside_foreign_object_are_left_alone() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><clipPath id=\"clip_long\"><rect/></clipPath></defs><rect clip-path=\"url(#clip_long)\"/><foreignObject><div xmlns=\"http://www.w3.org/1999/xhtml\" id=\"html-unused\"><p id=\"para\">x</p></div></foreignObject></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"a\""), "{output}");
        assert!(output.contains("url(#a)"), "{output}");
        assert!(output.contains("id=\"html-unused\""), "{output}");
        assert!(output.contains("id=\"para\""), "{output}");
    }

    #[test]
    fn generated_ids_skip_names_used_inside_foreign_object() {
        // The HTML <div id="a"> keeps its id, so the clipPath must not be
        // renamed to `a`: `url(#a)` would resolve to the <div> (first in
        // tree order) and the clip would be lost.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><foreignObject><div xmlns=\"http://www.w3.org/1999/xhtml\" id=\"a\">hi</div></foreignObject><defs><clipPath id=\"clip_long\"><rect width=\"5\" height=\"5\"/></clipPath></defs><rect clip-path=\"url(#clip_long)\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains("<div xmlns=\"http://www.w3.org/1999/xhtml\" id=\"a\">"),
            "{output}"
        );
        assert!(output.contains("<clipPath id=\"b\">"), "{output}");
        assert!(output.contains("clip-path=\"url(#b)\""), "{output}");
        assert!(!output.contains("clip_long"), "{output}");
    }

    #[test]
    fn generated_ids_skip_several_reserved_names() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"grad_one\"/><linearGradient id=\"grad_two\"/></defs><rect fill=\"url(#grad_one)\" stroke=\"url(#grad_two)\"/><foreignObject><div xmlns=\"http://www.w3.org/1999/xhtml\" id=\"a\"><span id=\"b\">x</span></div></foreignObject></svg>";
        let (_, output) = run_pass(input);
        assert!(
            output.contains("<linearGradient id=\"c\"></linearGradient><linearGradient id=\"d\">"),
            "{output}"
        );
        assert!(
            output.contains("fill=\"url(#c)\" stroke=\"url(#d)\""),
            "{output}"
        );
        assert!(output.contains("id=\"a\""), "{output}");
        assert!(output.contains("id=\"b\""), "{output}");
    }

    #[test]
    fn full_optimizer_convergence() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><clipPath id=\"clip0_long_name\"><rect width=\"10\" height=\"10\"/></clipPath></defs><rect clip-path=\"url(#clip0_long_name)\"/></svg>";
        let result1 = crate::optimize(input).unwrap();
        let result2 = crate::optimize(&result1.data).unwrap();
        assert_eq!(result1.data, result2.data, "should converge");
    }
}
