use super::{Pass, PassResult, has_scripts, is_css_ident_char, unescape_css_ident};
use crate::ast::{Document, NodeId, NodeKind};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Removes unreferenced `id` attributes and shortens referenced ones to
/// minimal unique names (`a`, `b`, ... `z`, `aa`, ...), rewriting every
/// reference (`url(#id)` in any attribute, `href`/`xlink:href="#id"`, SMIL
/// `begin`/`end` timing, `#id` selectors and `url(#id)` inside `<style>`).
///
/// Names are assigned in first-reference order (as SVGO does), which keeps
/// the output stable across `sortDefsChildren` and `sortAttrs`.
///
/// Deoptimises (leaves the document alone) when, as in SVGO:
/// - a script may run (`<script>`, `javascript:` links, event handlers):
///   scripts look ids up at runtime;
/// - the root `<svg>` contains nothing but `<defs>` (a sprite sheet whose
///   ids are referenced from other documents).
///
/// SVGO additionally bails on any non-empty `<style>`. svgm instead scans
/// the stylesheet for references, but still bails when it uses `:target`,
/// since the URL fragment then selects elements by id.
pub struct CleanupIds;

impl Pass for CleanupIds {
    fn name(&self) -> &'static str {
        "cleanupIds"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        if has_scripts(doc) || stylesheet_uses_target(doc) {
            return PassResult::Unchanged;
        }

        // Defs-only <svg> subtrees are left untouched entirely; ids declared
        // in them are reserved so generated names cannot collide.
        let mut ids = Vec::new();
        let mut skipped = Vec::new();
        collect_nodes(doc, doc.root, &mut ids, &mut skipped);

        // Phase 1: collect all id declarations.
        let mut declared: HashSet<String> = HashSet::new();
        for &id in &ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind
                && let Some(id_val) = elem.attr("id")
            {
                declared.insert(id_val.to_string());
            }
        }
        if declared.is_empty() {
            return PassResult::Unchanged;
        }

        // Phase 2: collect all references, in first-encounter order.
        let mut refs_in_order: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut style_text_nodes: HashSet<NodeId> = HashSet::new();
        {
            let mut note = |r: String| {
                if !seen.contains(&r) {
                    seen.insert(r.clone());
                    refs_in_order.push(r);
                }
            };
            for &id in &ids {
                let node = doc.node(id);
                match &node.kind {
                    NodeKind::Element(elem) => {
                        // Visit attributes in sortAttrs order so a second run
                        // over sorted attributes assigns the same names.
                        let mut attrs: Vec<usize> = (0..elem.attributes.len())
                            .filter(|&i| {
                                let a = &elem.attributes[i];
                                a.value.contains('(')
                                    || a.name == "href"
                                    || a.name == "begin"
                                    || a.name == "end"
                            })
                            .collect();
                        if attrs.len() > 1 {
                            attrs.sort_by_cached_key(|&i| elem.attributes[i].qualified_name());
                        }
                        for i in attrs {
                            let attr = &elem.attributes[i];
                            if attr.value.contains('(') {
                                for r in extract_url_refs(&attr.value) {
                                    note(r);
                                }
                            }
                            if attr.name == "href"
                                && let Some(r) = extract_href_ref(&attr.value)
                            {
                                note(r);
                            }
                            if attr.name == "begin" || attr.name == "end" {
                                for r in extract_smil_refs(&attr.value) {
                                    note(r);
                                }
                            }
                        }
                    }
                    NodeKind::Text(text) | NodeKind::CData(text)
                        if node
                            .parent
                            .is_some_and(|p| is_unprefixed_elem(doc, p, "style")) =>
                    {
                        style_text_nodes.insert(id);
                        for (_, r) in css_id_refs(text) {
                            note(r);
                        }
                    }
                    _ => {}
                }
            }
        }

        let referenced: HashSet<&str> = refs_in_order
            .iter()
            .map(String::as_str)
            .filter(|r| declared.contains(*r))
            .collect();

        // Phase 3: build the rename map. Generated names must not collide
        // with dangling references (svg/svgo#1815) or with ids declared in
        // skipped subtrees.
        let mut reserved: HashSet<String> = refs_in_order
            .iter()
            .filter(|r| !declared.contains(*r))
            .cloned()
            .collect();
        for &root in &skipped {
            collect_declared_ids(doc, root, &mut reserved);
        }

        let mut generator = IdGenerator::new();
        let mut rename_map: HashMap<String, String> = HashMap::new();
        for old_name in refs_in_order.iter().filter(|r| declared.contains(*r)) {
            let new_name = generator.next_id(&reserved);
            if new_name != *old_name {
                rename_map.insert(old_name.clone(), new_name);
            }
        }

        // Phase 4: anything to do?
        if declared.len() == referenced.len() && rename_map.is_empty() {
            return PassResult::Unchanged;
        }

        // Phase 5: apply changes.
        let mut changed = false;

        for &id in &ids {
            let node = doc.node_mut(id);
            match &mut node.kind {
                NodeKind::Element(elem) => {
                    if let Some(id_attr_pos) = elem
                        .attributes
                        .iter()
                        .position(|a| a.name == "id" && a.prefix.is_none())
                    {
                        let id_val = elem.attributes[id_attr_pos].value.clone();
                        if !referenced.contains(id_val.as_str()) {
                            elem.attributes.remove(id_attr_pos);
                            changed = true;
                        } else if let Some(new_name) = rename_map.get(&id_val) {
                            elem.attributes[id_attr_pos].value = new_name.clone();
                            changed = true;
                        }
                    }

                    for attr in &mut elem.attributes {
                        if attr.value.contains('(') {
                            let new_val = replace_url_refs(&attr.value, &rename_map);
                            if new_val != attr.value {
                                attr.value = new_val;
                                changed = true;
                            }
                        }

                        if attr.name == "href"
                            && let Some(old_ref) = extract_href_ref(&attr.value)
                            && let Some(new_name) = rename_map.get(&old_ref)
                        {
                            attr.value = format!("#{new_name}");
                            changed = true;
                        }

                        if attr.name == "begin" || attr.name == "end" {
                            let new_val = replace_smil_refs(&attr.value, &rename_map);
                            if new_val != attr.value {
                                attr.value = new_val;
                                changed = true;
                            }
                        }
                    }
                }
                // Only stylesheet text is rewritten; visible text, <title>
                // and <desc> may legitimately contain "#word".
                NodeKind::Text(text) | NodeKind::CData(text) if style_text_nodes.contains(&id) => {
                    let new_text = replace_css_ids(text, &rename_map);
                    if new_text != *text {
                        *text = new_text;
                        changed = true;
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

// --- Document queries ---

fn is_unprefixed_elem(doc: &Document, id: NodeId, name: &str) -> bool {
    matches!(&doc.node(id).kind, NodeKind::Element(e) if e.prefix.is_none() && e.name == name)
}

/// True when every element child of `svg` is a `<defs>` (SVGO's
/// `hasDefsOnly`); non-element children are ignored.
fn is_defs_only(doc: &Document, svg: NodeId) -> bool {
    doc.children(svg).all(|c| match &doc.node(c).kind {
        NodeKind::Element(e) => e.prefix.is_none() && e.name == "defs",
        _ => true,
    })
}

/// Depth-first node ids, skipping (and recording) defs-only `<svg>` subtrees.
fn collect_nodes(doc: &Document, id: NodeId, out: &mut Vec<NodeId>, skipped: &mut Vec<NodeId>) {
    if is_unprefixed_elem(doc, id, "svg") && is_defs_only(doc, id) {
        skipped.push(id);
        return;
    }
    out.push(id);
    for child in doc.children(id) {
        collect_nodes(doc, child, out, skipped);
    }
}

fn collect_declared_ids(doc: &Document, id: NodeId, out: &mut HashSet<String>) {
    if let NodeKind::Element(e) = &doc.node(id).kind
        && let Some(v) = e.attr("id")
    {
        out.insert(v.to_string());
    }
    for child in doc.children(id) {
        collect_declared_ids(doc, child, out);
    }
}

/// True when any `<style>` text mentions `:target`.
fn stylesheet_uses_target(doc: &Document) -> bool {
    doc.traverse().into_iter().any(|id| {
        is_unprefixed_elem(doc, id, "style")
            && doc.children(id).any(|c| match &doc.node(c).kind {
                NodeKind::Text(t) | NodeKind::CData(t) => {
                    t.to_ascii_lowercase().contains(":target")
                }
                _ => false,
            })
    })
}

// --- Reference scanning ---

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

/// Decode `%XX` escapes (SVGO applies `decodeURI` to every reference, see
/// svg/svgo#1696). Falls back to the raw text when the result is not UTF-8.
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// Parse the body of a `url(` reference starting at byte `j` (just after the
/// opening parenthesis): optional whitespace, optional quote, `#`, the id,
/// the matching quote or `)`. Returns the byte range of the raw id.
fn url_ref_at(b: &[u8], mut j: usize) -> Option<Range<usize>> {
    while j < b.len() && b[j].is_ascii_whitespace() {
        j += 1;
    }
    let quote = match b.get(j) {
        Some(&q @ (b'"' | b'\'')) => {
            j += 1;
            Some(q)
        }
        _ => None,
    };
    if b.get(j) != Some(&b'#') {
        return None;
    }
    let start = j + 1;
    let closer = quote.unwrap_or(b')');
    let mut end = start;
    while end < b.len() && b[end] != closer {
        end += 1;
    }
    if end >= b.len() {
        return None; // unterminated
    }
    let mut id_end = end;
    if quote.is_none() {
        while id_end > start && b[id_end - 1].is_ascii_whitespace() {
            id_end -= 1;
        }
    }
    (id_end > start).then_some(start..id_end)
}

/// Call `f` with the raw id range of every `url(#id)` / `url('#id')` /
/// `url("#id")` reference in `text`.
fn scan_url_refs(text: &str, mut f: impl FnMut(Range<usize>)) {
    let b = text.as_bytes();
    let mut i = 0;
    while i + 4 <= b.len() {
        if b[i..i + 4].eq_ignore_ascii_case(b"url(") && (i == 0 || !is_ident_byte(b[i - 1])) {
            if let Some(r) = url_ref_at(b, i + 4) {
                i = r.end;
                f(r);
            } else {
                i += 4;
            }
            continue;
        }
        i += 1;
    }
}

/// Extract all (percent-decoded) ids from `url(#id)` references in an
/// attribute value.
fn extract_url_refs(value: &str) -> Vec<String> {
    let mut refs = Vec::new();
    scan_url_refs(value, |r| refs.push(percent_decode(&value[r])));
    refs
}

/// Extract the (percent-decoded) id from an `href="#id"` value.
fn extract_href_ref(value: &str) -> Option<String> {
    let id = value.strip_prefix('#')?;
    if id.is_empty() {
        None
    } else {
        Some(percent_decode(id))
    }
}

/// Extract id candidates from SMIL timing values like `id.click`,
/// `0;id.end+2s`.
fn extract_smil_refs(value: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for part in value.split(';') {
        let trimmed = part.trim();
        if let Some(dot_pos) = trimmed.find('.') {
            let candidate = &trimmed[..dot_pos];
            if !candidate.is_empty() {
                refs.push(candidate.to_string());
            }
        }
    }
    refs
}

/// Byte offset just past the CSS identifier starting at `start` (handles
/// backslash escapes and non-ASCII code points).
fn ident_end(text: &str, start: usize) -> usize {
    let rest = &text[start..];
    let mut pos = 0;
    while let Some(c) = rest[pos..].chars().next() {
        if c == '\\' {
            let mut hex_end = pos + 1;
            while hex_end - (pos + 1) < 6
                && rest[hex_end..]
                    .chars()
                    .next()
                    .is_some_and(|h| h.is_ascii_hexdigit())
            {
                hex_end += 1;
            }
            if hex_end > pos + 1 {
                pos = hex_end;
                if let Some(w) = rest[pos..].chars().next()
                    && w.is_whitespace()
                {
                    pos += w.len_utf8();
                }
            } else if let Some(e) = rest[pos + 1..].chars().next() {
                pos += 1 + e.len_utf8();
            } else {
                pos += 1;
            }
        } else if is_css_ident_char(c) {
            pos += c.len_utf8();
        } else {
            break;
        }
    }
    start + pos
}

/// Id references in stylesheet text: `#id` tokens in selector preludes
/// (text before a `{` that does not start with `@`) and `url(#id)` anywhere.
/// Comments and strings are skipped, so `fill:#fff` inside a declaration
/// block, `/* #x */` and `content:"#y"` are not references. Returns the byte
/// range of each raw token together with its decoded id, in document order.
fn css_id_refs(text: &str) -> Vec<(Range<usize>, String)> {
    let b = text.as_bytes();
    let n = b.len();
    let mut out: Vec<(Range<usize>, String)> = Vec::new();
    // `#id` tokens of the current segment; they only count if the segment
    // turns out to be a selector prelude.
    let mut pending: Vec<(Range<usize>, String)> = Vec::new();
    let mut seg_first: Option<u8> = None;
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i = text[i + 2..].find("*/").map_or(n, |p| i + 2 + p + 2);
            continue;
        }
        if c == b'"' || c == b'\'' {
            seg_first.get_or_insert(c);
            i += 1;
            while i < n && b[i] != c {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        match c {
            b'{' => {
                if seg_first != Some(b'@') {
                    out.append(&mut pending);
                } else {
                    pending.clear();
                }
                seg_first = None;
                i += 1;
            }
            b'}' | b';' => {
                pending.clear();
                seg_first = None;
                i += 1;
            }
            b'#' => {
                seg_first.get_or_insert(c);
                let start = i + 1;
                let end = ident_end(text, start);
                if end > start {
                    pending.push((start..end, unescape_css_ident(&text[start..end])));
                    i = end;
                } else {
                    i += 1;
                }
            }
            _ if c.is_ascii_whitespace() => i += 1,
            _ => {
                seg_first.get_or_insert(c);
                if (c == b'u' || c == b'U')
                    && i + 4 <= n
                    && b[i..i + 4].eq_ignore_ascii_case(b"url(")
                    && (i == 0 || !is_ident_byte(b[i - 1]))
                {
                    if let Some(r) = url_ref_at(b, i + 4) {
                        i = r.end;
                        out.push((r.clone(), percent_decode(&text[r])));
                    } else {
                        i += 4;
                    }
                    continue;
                }
                i += 1;
            }
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out
}

// --- Reference replacement ---

/// Replace the given byte ranges of `text` (ascending, non-overlapping) whose
/// decoded id is in `rename_map`.
fn rewrite_ranges<'a>(
    text: &str,
    refs: impl Iterator<Item = (Range<usize>, &'a str)>,
    rename_map: &HashMap<String, String>,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (r, id) in refs {
        if let Some(new_name) = rename_map.get(id) {
            out.push_str(&text[last..r.start]);
            out.push_str(new_name);
            last = r.end;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// Rewrite renamed ids inside `url(#id)` references, preserving quoting.
fn replace_url_refs(value: &str, rename_map: &HashMap<String, String>) -> String {
    let mut refs = Vec::new();
    scan_url_refs(value, |r| refs.push(r));
    let decoded: Vec<(Range<usize>, String)> = refs
        .into_iter()
        .map(|r| {
            let id = percent_decode(&value[r.clone()]);
            (r, id)
        })
        .collect();
    rewrite_ranges(
        value,
        decoded.iter().map(|(r, id)| (r.clone(), id.as_str())),
        rename_map,
    )
}

/// Replace SMIL timing references: `old_id.click` -> `new_id.click`.
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

/// Rewrite renamed ids in stylesheet text (selectors and `url()` values).
fn replace_css_ids(text: &str, rename_map: &HashMap<String, String>) -> String {
    let refs = css_id_refs(text);
    rewrite_ranges(
        text,
        refs.iter().map(|(r, id)| (r.clone(), id.as_str())),
        rename_map,
    )
}

// --- ID generator ---

struct IdGenerator {
    counter: usize,
}

impl IdGenerator {
    fn new() -> Self {
        Self { counter: 0 }
    }

    /// Next minimal name that is not in `reserved`.
    fn next_id(&mut self, reserved: &HashSet<String>) -> String {
        loop {
            let name = encode(self.counter);
            self.counter += 1;
            if !reserved.contains(&name) {
                return name;
            }
        }
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

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
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

    #[test]
    fn generator_skips_reserved_names() {
        let reserved: HashSet<String> = ["a", "c"].iter().map(|s| s.to_string()).collect();
        let mut g = IdGenerator::new();
        assert_eq!(g.next_id(&reserved), "b");
        assert_eq!(g.next_id(&reserved), "d");
    }

    // --- Scanners ---

    #[test]
    fn url_scanner_accepts_quotes_and_whitespace() {
        assert_eq!(extract_url_refs("url(#a)"), vec!["a"]);
        assert_eq!(extract_url_refs("url('#a')"), vec!["a"]);
        assert_eq!(extract_url_refs("url(\"#a\")"), vec!["a"]);
        assert_eq!(extract_url_refs("url( '#a' )"), vec!["a"]);
        assert_eq!(extract_url_refs("url( #a )"), vec!["a"]);
        assert_eq!(extract_url_refs("URL(#a)"), vec!["a"]);
        assert_eq!(
            extract_url_refs("fill:url(#a);stroke:url('#b')"),
            vec!["a", "b"]
        );
        assert_eq!(extract_url_refs("url(#%E4%BA%BA%E5%8F%A3)"), vec!["人口"]);
        assert_eq!(extract_url_refs("url(#渐变_1)"), vec!["渐变_1"]);
        assert!(extract_url_refs("url(#)").is_empty());
        assert!(extract_url_refs("url(#a").is_empty());
        assert!(extract_url_refs("url(data:x)").is_empty());
        assert!(extract_url_refs("url('#a)").is_empty());
    }

    #[test]
    fn url_rewrite_preserves_quoting() {
        let m = map(&[("g", "a")]);
        assert_eq!(replace_url_refs("url(#g)", &m), "url(#a)");
        assert_eq!(replace_url_refs("url('#g')", &m), "url('#a')");
        assert_eq!(replace_url_refs("url( \"#g\" )", &m), "url( \"#a\" )");
        assert_eq!(replace_url_refs("url(#g) url(#h)", &m), "url(#a) url(#h)");
        assert_eq!(replace_url_refs("url(#%67)", &m), "url(#a)");
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("my%20id"), "my id");
        assert_eq!(percent_decode("%E4%BA%BA%E5%8F%A3"), "人口");
        // Malformed sequences are kept verbatim.
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("%E4%BA"), "%E4%BA");
    }

    #[test]
    fn css_scanner_finds_selectors_and_urls_only() {
        let ids =
            |css: &str| -> Vec<String> { css_id_refs(css).into_iter().map(|(_, s)| s).collect() };
        assert_eq!(ids("#a{fill:red}"), vec!["a"]);
        assert_eq!(ids(".k{fill:#fff}"), Vec::<String>::new());
        assert_eq!(ids(".k{fill:url(#g)}"), vec!["g"]);
        assert_eq!(ids("#a,#b .c>#d:not(#e){}"), vec!["a", "b", "d", "e"]);
        assert_eq!(
            ids("@media (min-width:1px){#a{fill:url(#g)}}"),
            vec!["a", "g"]
        );
        assert_eq!(ids("@import url(#x);#a{}"), vec!["x", "a"]);
        assert_eq!(ids("/* #c */ #a{} a{content:'#s'}"), vec!["a"]);
        assert_eq!(ids("#ǎ{}#bǎ{}"), vec!["ǎ", "bǎ"]);
        assert_eq!(ids(r"#a\:b{}"), vec!["a:b"]);
        assert_eq!(ids(r"#a\3A b{}"), vec!["a:b"]);
        // A trailing selector without a block is not a rule.
        assert_eq!(ids("#a{} #b"), vec!["a"]);
        assert_eq!(ids("#a{fill:#fff;stroke:url( '#g' )}"), vec!["a", "g"]);
    }

    #[test]
    fn css_rewrite_leaves_colours_alone() {
        let m = map(&[("fff", "a"), ("g", "b"), ("a:b", "c")]);
        assert_eq!(
            replace_css_ids(".k{fill:#fff}#fff{stroke:url(#g)}", &m),
            ".k{fill:#fff}#a{stroke:url(#b)}"
        );
        assert_eq!(replace_css_ids(r"#a\:b{}", &m), "#c{}");
        assert_eq!(replace_css_ids("#other{}", &m), "#other{}");
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

    #[test]
    fn quoted_url_refs_are_renamed_not_deleted() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"g1\"/><linearGradient id=\"g2\"/><linearGradient id=\"g3\"/></defs><rect fill=\"url('#g1')\"/><rect stroke='url(\"#g2\")'/><rect style=\"fill:url( '#g3' )\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"a\"></linearGradient><linearGradient id=\"b\"></linearGradient><linearGradient id=\"c\"></linearGradient></defs><rect fill=\"url('#a')\"/><rect stroke=\"url(&quot;#b&quot;)\"/><rect style=\"fill:url( '#c' )\"/></svg>"
        );
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

    // SVGO cleanupIds.25
    #[test]
    fn svgo_25_percent_encoded_href_resolves_non_ascii_id() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 9 9\"><defs><path id=\"人口\" d=\"M1 1l2 2\" stroke=\"black\"/></defs><use href=\"#%E4%BA%BA%E5%8F%A3\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 9 9\"><defs><path id=\"a\" d=\"M1 1l2 2\" stroke=\"black\"/></defs><use href=\"#a\"/></svg>"
        );
    }

    // SVGO cleanupIds.26
    #[test]
    fn svgo_26_non_ascii_url_reference() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"渐变_1\"><stop stop-color=\"#5a2100\"/></linearGradient></defs><rect fill=\"url(#渐变_1)\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"a\"><stop stop-color=\"#5a2100\"/></linearGradient></defs><rect fill=\"url(#a)\"/></svg>"
        );
    }

    #[test]
    fn percent_encoded_space_in_href() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><path id=\"my id\" d=\"M0 0\"/></defs><use href=\"#my%20id\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains("id=\"a\"") && output.contains("href=\"#a\""),
            "{output}"
        );
    }

    // SVGO cleanupIds.23 (svg/svgo#1815)
    #[test]
    fn svgo_23_dangling_reference_reserves_generated_name() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><path id=\"uwu\" d=\"M0 0\"/></defs><use href=\"#a\" x=\"378\" y=\"464\"/><use href=\"#uwu\" x=\"385\" y=\"464\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><path id=\"b\" d=\"M0 0\"/></defs><use href=\"#a\" x=\"378\" y=\"464\"/><use href=\"#b\" x=\"385\" y=\"464\"/></svg>"
        );
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

    // SVGO cleanupIds.24 (svg/svgo#1775): names follow first-reference order.
    #[test]
    fn svgo_24_smil_begin_lists() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><circle cx=\"12\" cy=\"12\"><animate id=\"thing1\" fill=\"freeze\" attributeName=\"r\" begin=\"0;thing2.end\" dur=\"1.2s\" values=\"0;11\"/></circle><circle cx=\"12\" cy=\"12\"><animate id=\"thing2\" fill=\"freeze\" attributeName=\"r\" begin=\"thing1.begin+0.2s\" dur=\"1.2s\" values=\"0;11\"/></circle></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><circle cx=\"12\" cy=\"12\"><animate id=\"b\" fill=\"freeze\" attributeName=\"r\" begin=\"0;a.end\" dur=\"1.2s\" values=\"0;11\"/></circle><circle cx=\"12\" cy=\"12\"><animate id=\"a\" fill=\"freeze\" attributeName=\"r\" begin=\"b.begin+0.2s\" dur=\"1.2s\" values=\"0;11\"/></circle></svg>"
        );
    }

    // SVGO cleanupIds.01 (minus the duplicated id): mixed reference kinds,
    // quoting preserved, naming in first-reference order. The only
    // difference from SVGO's expected output: within one element svgm reads
    // attributes in sortAttrs order (`begin` before `xlink:href`), so
    // `block`/`crochet` receive `c`/`d` instead of `d`/`c`.
    #[test]
    fn svgo_01_mixed_references() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><defs><linearGradient id=\"gradient001\"><stop offset=\"5%\" stop-color=\"#F60\"/></linearGradient><text id=\"referencedText\">referenced text</text><path id=\"crochet\" d=\"...\"/><path id=\"block\" d=\"...\"/><path id=\"two\" d=\"...\"/></defs><g id=\"g001\"><circle id=\"circle001\" fill=\"url(#gradient001)\" cx=\"60\" cy=\"60\" r=\"50\"/><rect fill=\"url('#gradient001')\" x=\"0\" y=\"0\" width=\"500\" height=\"100\"/><tref xlink:href=\"#referencedText\"/></g><g><tref xlink:href=\"#referencedText\"/></g><animateMotion xlink:href=\"#crochet\" dur=\"0.5s\" begin=\"block.mouseover\" fill=\"freeze\" path=\"m 0,0 0,-21\"/><use xlink:href=\"#two\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><defs><linearGradient id=\"a\"><stop offset=\"5%\" stop-color=\"#F60\"/></linearGradient><text id=\"b\">referenced text</text><path id=\"d\" d=\"...\"/><path id=\"c\" d=\"...\"/><path id=\"e\" d=\"...\"/></defs><g><circle fill=\"url(#a)\" cx=\"60\" cy=\"60\" r=\"50\"/><rect fill=\"url('#a')\" x=\"0\" y=\"0\" width=\"500\" height=\"100\"/><tref xlink:href=\"#b\"/></g><g><tref xlink:href=\"#b\"/></g><animateMotion xlink:href=\"#d\" dur=\"0.5s\" begin=\"c.mouseover\" fill=\"freeze\" path=\"m 0,0 0,-21\"/><use xlink:href=\"#e\"/></svg>"
        );
    }

    // SVGO cleanupIds.05: forward references and an existing short id.
    #[test]
    fn svgo_05_forward_references() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:x=\"http://www.w3.org/1999/xlink\"><defs><g id=\"mid-line\"/><g id=\"line-plus\"><use x:href=\"#mid-line\"/><use x:href=\"#plus\"/></g><g id=\"plus\"/><g id=\"line-circle\"><use x:href=\"#mid-line\"/></g></defs><path d=\"M0 0\" id=\"a\"/><use x:href=\"#a\" x=\"50\" y=\"50\"/><use x:href=\"#line-plus\"/><use x:href=\"#line-circle\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:x=\"http://www.w3.org/1999/xlink\"><defs><g id=\"a\"></g><g id=\"d\"><use x:href=\"#a\"/><use x:href=\"#b\"/></g><g id=\"b\"></g><g id=\"e\"><use x:href=\"#a\"/></g></defs><path d=\"M0 0\" id=\"c\"/><use x:href=\"#c\" x=\"50\" y=\"50\"/><use x:href=\"#d\"/><use x:href=\"#e\"/></svg>"
        );
    }

    // SVGO cleanupIds.22: two ids referenced from one style attribute.
    #[test]
    fn svgo_22_two_ids_in_one_style_attribute() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><linearGradient id=\"a\"><stop stop-color=\"#f00\" offset=\"0\"/></linearGradient><linearGradient id=\"linearGradient3520\" href=\"#a\" x1=\"0\"/><filter id=\"c\" style=\"color-interpolation-filters:sRGB\"><feGaussianBlur stdDeviation=\"3\"/></filter><path d=\"M0 0\" style=\"opacity:1;fill:url(#linearGradient3520);fill-opacity:1;filter:url(#c)\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><linearGradient id=\"a\"><stop stop-color=\"#f00\" offset=\"0\"/></linearGradient><linearGradient id=\"b\" href=\"#a\" x1=\"0\"></linearGradient><filter id=\"c\" style=\"color-interpolation-filters:sRGB\"><feGaussianBlur stdDeviation=\"3\"/></filter><path d=\"M0 0\" style=\"opacity:1;fill:url(#b);fill-opacity:1;filter:url(#c)\"/></svg>"
        );
    }

    // --- Deoptimisation ---

    // SVGO cleanupIds.03
    #[test]
    fn svgo_03_script_element_deoptimizes() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>document.getElementById('circle001')</script><circle id=\"circle001\" fill=\"red\" cx=\"60\" cy=\"60\" r=\"50\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
        // An external script can run too.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><script href=\"app.js\"/><circle id=\"circle001\"/></svg>";
        assert_eq!(run_pass(input).0, PassResult::Unchanged);
    }

    #[test]
    fn event_handler_and_javascript_link_deoptimize() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle id=\"circle001\" onclick=\"go()\"/><rect id=\"r\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);

        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><a href=\"javascript:toggle('r')\"><rect id=\"r\"/></a></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }

    // SVGO cleanupIds.09 / .10: `:target` selects elements by id from the
    // URL fragment, so ids must neither be removed nor renamed.
    #[test]
    fn svgo_09_target_selector_deoptimizes() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"0 0 120 120\"><style>svg .hidden { display: none; } svg .hidden:target { display: inline; }</style><defs><circle id=\"circle\" fill=\"red\" cx=\"60\" cy=\"60\" r=\"50\"/><rect id=\"rect\" fill=\"blue\" x=\"10\" y=\"10\" width=\"100\" height=\"100\"/></defs><g id=\"figure\" class=\"hidden\"><use xlink:href=\"#circle\"/><use href=\"#rect\"/></g></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }

    // SVGO cleanupIds.11
    #[test]
    fn svgo_11_defs_only_document_is_untouched() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\">\n    <defs>\n        <circle cx=\"100\" cy=\"100\" r=\"50\" id=\"circle\"/>\n        <ellipse cx=\"50\" cy=\"50\" rx=\"50\" ry=\"10\" id=\"ellipse\"/>\n        <rect x=\"100\" y=\"50\" width=\"50\" height=\"10\" id=\"rect\"/>\n    </defs>\n</svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
        // Two <defs> and a comment still count as defs-only.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><!-- sprites --><defs><path id=\"a1\"/></defs><defs><path id=\"b1\"/></defs></svg>";
        assert_eq!(run_pass(input).0, PassResult::Unchanged);
    }

    #[test]
    fn nested_defs_only_svg_is_skipped_and_its_ids_reserved() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><svg><defs><path id=\"a\" d=\"M0 0\"/></defs></svg><path id=\"long\" d=\"M1 1\"/><use href=\"#long\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><svg><defs><path id=\"a\" d=\"M0 0\"/></defs></svg><path id=\"b\" d=\"M1 1\"/><use href=\"#b\"/></svg>"
        );
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

    // SVGO cleanupIds.19: an empty <style> does not deoptimise.
    #[test]
    fn svgo_19_empty_style_does_not_deoptimize() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><style></style><linearGradient id=\"file-name_svg__original-id\" x1=\"12\"><stop offset=\"0\" stop-color=\"#6b5aed\"/></linearGradient></defs><path d=\"M46 24\" fill=\"url(#file-name_svg__original-id)\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains("id=\"a\"") && output.contains("url(#a)"),
            "{output}"
        );
    }

    #[test]
    fn hex_colour_in_declaration_is_not_a_reference() {
        // `fill:#fff` is a colour, `#fff{...}` is a selector for id="fff".
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.k{fill:#fff}#fff{stroke:red}</style><rect id=\"fff\" class=\"k\"/><rect id=\"abc\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.k{fill:#fff}#a{stroke:red}</style><rect id=\"a\" class=\"k\"/><rect/></svg>"
        );
        // Colour only, no selector: the id is unreferenced and removed, the
        // colour untouched.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.k{fill:#fff}</style><rect id=\"fff\" class=\"k\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.k{fill:#fff}</style><rect class=\"k\"/></svg>"
        );
    }

    #[test]
    fn css_url_references_inside_at_rules_are_found() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>@media (min-width:1px){#big{fill:url(#grad)}}</style><defs><linearGradient id=\"grad\"/></defs><rect id=\"big\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>@media (min-width:1px){#a{fill:url(#b)}}</style><defs><linearGradient id=\"b\"></linearGradient></defs><rect id=\"a\"/></svg>"
        );
    }

    #[test]
    fn non_ascii_css_ids_match_whole_identifiers() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>#ǎ{fill:red}#bǎ{fill:blue}</style><rect id=\"ǎ\"/><rect id=\"bǎ\"/><rect id=\"b\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>#a{fill:red}#b{fill:blue}</style><rect id=\"a\"/><rect id=\"b\"/><rect/></svg>"
        );
    }

    #[test]
    fn escaped_css_id_selector() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>#a\\:b{fill:red}</style><rect id=\"a:b\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>#a{fill:red}</style><rect id=\"a\"/></svg>"
        );
    }

    #[test]
    fn css_comments_and_strings_are_not_references() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>/* #x */ .a{content:\"#y\"}</style><rect id=\"x\"/><rect id=\"y\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>/* #x */ .a{content:\"#y\"}</style><rect/><rect/></svg>"
        );
    }

    #[test]
    fn only_style_text_is_rewritten() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>#grad</title><desc>uses #grad</desc><defs><linearGradient id=\"grad\"/></defs><text>#grad</text><rect fill=\"url(#grad)\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><title>#grad</title><desc>uses #grad</desc><defs><linearGradient id=\"a\"></linearGradient></defs><text>#grad</text><rect fill=\"url(#a)\"/></svg>"
        );
    }

    // --- Edge cases ---

    #[test]
    fn already_short_id_no_change() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><clipPath id=\"a\"><rect width=\"10\" height=\"10\"/></clipPath></defs><rect clip-path=\"url(#a)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
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
    fn naming_is_stable_under_attribute_order() {
        // sortAttrs runs later and reorders attributes; names must not
        // depend on the original attribute order or the output would change
        // on a second run.
        let a = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"g1\"/><linearGradient id=\"g2\"/></defs><rect stroke=\"url(#g1)\" fill=\"url(#g2)\"/></svg>";
        let b = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"g1\"/><linearGradient id=\"g2\"/></defs><rect fill=\"url(#g2)\" stroke=\"url(#g1)\"/></svg>";
        let (_, out_a) = run_pass(a);
        let (_, out_b) = run_pass(b);
        assert!(out_a.contains("fill=\"url(#a)\""), "{out_a}");
        assert!(out_b.contains("fill=\"url(#a)\""), "{out_b}");
    }

    #[test]
    fn full_optimizer_convergence() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><clipPath id=\"clip0_long_name\"><rect width=\"10\" height=\"10\"/></clipPath></defs><rect clip-path=\"url(#clip0_long_name)\"/></svg>";
        let result1 = crate::optimize(input).unwrap();
        let result2 = crate::optimize(&result1.data).unwrap();
        assert_eq!(result1.data, result2.data, "should converge");
    }

    #[test]
    fn full_optimizer_convergence_with_sorted_defs() {
        // sortDefsChildren reorders <defs> after cleanupIds; names must come
        // from reference order so the second run agrees with the first.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs><linearGradient id=\"paint\" x1=\"0\" x2=\"1\"><stop offset=\"0\" stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/></linearGradient><clipPath id=\"clip\"><rect width=\"10\" height=\"10\"/></clipPath><clipPath id=\"clip2\"><rect width=\"5\" height=\"5\"/></clipPath></defs><g clip-path=\"url(#clip)\"><rect width=\"10\" height=\"10\" fill=\"url(#paint)\"/><circle r=\"3\" clip-path=\"url(#clip2)\"/></g></svg>";
        let result1 = crate::optimize(input).unwrap();
        let result2 = crate::optimize(&result1.data).unwrap();
        assert_eq!(result1.data, result2.data, "should converge");
    }
}
