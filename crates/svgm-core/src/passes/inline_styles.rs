//! inlineStyles: merge `<style>` sheets, inline simple rules onto the elements
//! they match (honouring specificity and source order), drop rules that match
//! nothing and strip class names that are no longer referenced.
//!
//! Only simple selectors (`rect`, `.a`, `#x`, `rect.a.b#x`, comma lists of
//! those) are understood. Everything else is "unsupported": such rules are
//! preserved verbatim, never inlined and never removed, and they constrain
//! what the supported rules may do (see [`decide_target`]). Their exact
//! specificity is computed per CSS Selectors 4 ([`selector_specificity`]);
//! a selector that cannot be understood blocks every rule it competes with.

use super::{
    Pass, PassResult, attributes_in_stylesheets, has_scripts, inside_foreign_object,
    is_css_ident_char, split_declarations, split_property, split_selector_list, strip_css_comments,
};
use crate::ast::{Attribute, Document, NodeId, NodeKind};
use std::collections::{HashMap, HashSet};

/// SVG presentation attributes — CSS properties that can be set as XML attributes.
const PRESENTATION_ATTRS: &[&str] = &[
    "alignment-baseline",
    "baseline-shift",
    "clip-path",
    "clip-rule",
    "color",
    "color-interpolation",
    "color-interpolation-filters",
    "cursor",
    "direction",
    "display",
    "dominant-baseline",
    "fill",
    "fill-opacity",
    "fill-rule",
    "filter",
    "flood-color",
    "flood-opacity",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "image-rendering",
    "letter-spacing",
    "lighting-color",
    "marker-end",
    "marker-mid",
    "marker-start",
    "mask",
    "opacity",
    "overflow",
    "pointer-events",
    "shape-rendering",
    "stop-color",
    "stop-opacity",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "text-anchor",
    "text-decoration",
    "text-rendering",
    "unicode-bidi",
    "visibility",
    "word-spacing",
    "writing-mode",
];

pub struct InlineStyles;

/// `(ids, classes, types)` as in the CSS cascade.
type Specificity = (u32, u32, u32);

/// A parsed simple (compound) selector: `rect.a.b#x`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SimpleSelector {
    elem: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
}

impl SimpleSelector {
    fn specificity(&self) -> Specificity {
        (
            u32::from(self.id.is_some()),
            self.classes.len() as u32,
            u32::from(self.elem.is_some()),
        )
    }
}

/// One comma-separated part of a rule's selector list.
struct SelectorPart {
    /// Original (trimmed) text, re-emitted verbatim when the part survives.
    text: String,
    /// `Some` for a supported simple selector; `None` for anything else.
    parsed: Option<SimpleSelector>,
    matched: Vec<NodeId>,
    /// Dropped from the stylesheet: unused, or fully inlined.
    removed: bool,
    /// Fully inlined onto every matched element (subset of `removed`).
    consumed: bool,
}

struct CssRule {
    parts: Vec<SelectorPart>,
    declarations: Vec<(String, String)>,
}

enum CssItem {
    Rule(CssRule),
    /// An `@`-rule kept as opaque text.
    AtRule(String),
}

/// Where an inlined declaration goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// Presentation attribute: ranks below every stylesheet rule.
    Attr,
    /// `style` attribute: ranks above every non-`!important` rule.
    Style,
    /// Cannot be inlined safely; the declaration stays in the stylesheet.
    Blocked,
}

/// A selector part that stays in the stylesheet and may match any element:
/// an unsupported part, or (when scripts are present) a supported part that
/// matches nothing yet. Carries the exact specificity of the part (`None`
/// when the selector is not understood) and its rule's declarations with
/// their `!important` flags.
struct KeptPart {
    rule_idx: usize,
    spec: Option<Specificity>,
    decls: Vec<(String, bool)>,
}

impl Pass for InlineStyles {
    fn name(&self) -> &'static str {
        "inlineStyles"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let all_ids = doc.traverse();

        // Elements inside <foreignObject> are HTML: their <style> sheets are
        // not ours to read and they never receive SVG presentation attributes.
        let foreign: HashSet<NodeId> = all_ids
            .iter()
            .copied()
            .filter(|&id| inside_foreign_object(doc, id))
            .collect();

        // Step 1: collect <style> elements and their CSS text. A `media`
        // attribute scopes the sheet, so its text is wrapped in `@media`.
        let mut style_nodes: Vec<NodeId> = Vec::new();
        let mut css_text = String::new();
        let mut media_attr_seen = false;

        for &nid in &all_ids {
            if foreign.contains(&nid) {
                continue;
            }
            let NodeKind::Element(ref elem) = doc.node(nid).kind else {
                continue;
            };
            if elem.name != "style" || elem.prefix.is_some() {
                continue;
            }
            if let Some(t) = elem.attr("type")
                && !t.is_empty()
                && t != "text/css"
            {
                continue;
            }
            style_nodes.push(nid);

            let mut text = String::new();
            for child_id in doc.children(nid) {
                if let NodeKind::Text(t) | NodeKind::CData(t) = &doc.node(child_id).kind {
                    text.push_str(t);
                }
            }

            let media = elem.attr("media").map(str::trim).unwrap_or("");
            if elem.attr("media").is_some() {
                media_attr_seen = true;
            }
            if text.trim().is_empty() {
                continue;
            }
            if media.is_empty() || media == "all" {
                css_text.push_str(&text);
            } else {
                css_text.push_str("@media ");
                css_text.push_str(media);
                css_text.push('{');
                css_text.push_str(&text);
                css_text.push('}');
            }
        }

        if style_nodes.is_empty() {
            return PassResult::Unchanged;
        }

        // Step 2: parse the CSS; on failure preserve everything.
        let Some(mut items) = parse_css(&css_text) else {
            return PassResult::Unchanged;
        };
        let has_at_rules = items.iter().any(|i| matches!(i, CssItem::AtRule(_)));

        // Step 3: DOM lookup tables (all elements, so that rules matching
        // HTML inside <foreignObject> are detected and left alone).
        let mut class_map: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut id_map: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut style_attr_props: HashMap<NodeId, HashSet<String>> = HashMap::new();

        for &nid in &all_ids {
            let NodeKind::Element(ref elem) = doc.node(nid).kind else {
                continue;
            };
            if let Some(class_val) = elem.attr("class") {
                for class_name in class_val.split_whitespace() {
                    class_map
                        .entry(class_name.to_string())
                        .or_default()
                        .push(nid);
                }
            }
            if let Some(id_val) = elem.attr("id") {
                id_map.entry(id_val.to_string()).or_default().push(nid);
            }
            if let Some(style_val) = elem.attr("style") {
                let props: HashSet<String> = split_declarations(style_val)
                    .into_iter()
                    .filter_map(|decl| split_property(decl).map(|(name, _)| name.to_string()))
                    .collect();
                if !props.is_empty() {
                    style_attr_props.insert(nid, props);
                }
            }
        }

        // Step 4: match supported selectors. A part that reaches into a
        // <foreignObject> is demoted to unsupported: kept, never touched.
        for item in &mut items {
            let CssItem::Rule(rule) = item else { continue };
            for part in &mut rule.parts {
                let Some(sel) = &part.parsed else { continue };
                part.matched = match_simple_selector(sel, &class_map, &id_map, &all_ids, doc);
                if part.matched.iter().any(|nid| foreign.contains(nid)) {
                    part.parsed = None;
                    part.matched.clear();
                }
            }
        }

        // Step 5: document-wide safety flags.
        let scripts = has_scripts(doc);
        // Attribute names used in attribute selectors: adding or removing
        // such an attribute would change what those selectors match.
        let attr_selected = attributes_in_stylesheets(doc);

        let mut rules_changed = false;
        let mut attrs_changed = false;

        // Step 6: drop supported parts that match nothing (unless a script
        // could add matching elements or classes later).
        if !scripts {
            for item in &mut items {
                let CssItem::Rule(rule) = item else { continue };
                for part in &mut rule.parts {
                    if part.parsed.is_some() && part.matched.is_empty() {
                        part.removed = true;
                        rules_changed = true;
                    }
                }
            }
        }

        // Step 7: inline. Skipped entirely when @-rules are present: their
        // contents (media-scoped rules, keyframes, imports) cannot be reasoned
        // about, so every element keeps its classes and every rule stays.
        if !has_at_rules {
            let rules: Vec<&CssRule> = items
                .iter()
                .filter_map(|i| match i {
                    CssItem::Rule(r) => Some(r),
                    CssItem::AtRule(_) => None,
                })
                .collect();

            // Every part that survives in the sheet, one entry per part so
            // that a list like `svg path, g :hover` ranks each alternative on
            // its own (which one matches a given element is unknown).
            let mut kept: Vec<KeptPart> = Vec::new();
            for (idx, rule) in rules.iter().enumerate() {
                let decls: Vec<(String, bool)> = rule
                    .declarations
                    .iter()
                    .map(|(p, v)| (p.clone(), is_important(v)))
                    .collect();
                for part in &rule.parts {
                    let spec = match &part.parsed {
                        None => selector_specificity(&part.text),
                        // Unmatched but kept (scripts may add matching
                        // elements or classes later).
                        Some(sel) if !part.removed && part.matched.is_empty() => {
                            Some(sel.specificity())
                        }
                        Some(_) => continue,
                    };
                    kept.push(KeptPart {
                        rule_idx: idx,
                        spec,
                        decls: decls.clone(),
                    });
                }
            }

            // Highest specificity first; for equal specificity the later
            // rule wins, so it is applied first. First writer wins per
            // element and property.
            let mut order: Vec<(usize, usize, Specificity)> = Vec::new();
            for (ri, rule) in rules.iter().enumerate() {
                for (pi, part) in rule.parts.iter().enumerate() {
                    if let Some(sel) = &part.parsed
                        && !part.removed
                    {
                        order.push((ri, pi, sel.specificity()));
                    }
                }
            }
            order.sort_by(|a, b| b.2.cmp(&a.2).then(b.0.cmp(&a.0)).then(b.1.cmp(&a.1)));

            // Properties already settled on each element, by its own style
            // attribute or by a higher-priority rule.
            let mut written: HashMap<NodeId, HashSet<String>> = style_attr_props.clone();
            // handled[rule][part][decl]: the declaration no longer needs the
            // stylesheet for any element the part matches.
            let mut handled: Vec<Vec<Vec<bool>>> = rules
                .iter()
                .map(|r| {
                    r.parts
                        .iter()
                        .map(|_| vec![false; r.declarations.len()])
                        .collect()
                })
                .collect();

            for &(ri, pi, spec) in &order {
                let rule = rules[ri];
                let part = &rule.parts[pi];
                if part.matched.is_empty() {
                    // Unused but kept (scripts present): nothing to inline.
                    continue;
                }
                for (di, (prop, val)) in rule.declarations.iter().enumerate() {
                    let target = decide_target(prop, val, ri, spec, &kept, &attr_selected);
                    for &nid in &part.matched {
                        let settled = written.entry(nid).or_default();
                        if settled.contains(prop) {
                            continue;
                        }
                        // The rule keeps this declaration, and it outranks
                        // every lower rule on this element, so those lower
                        // rules must not inline it either.
                        settled.insert(prop.clone());
                        if target == Target::Blocked {
                            continue;
                        }
                        if apply_declaration(doc, nid, prop, val, target) {
                            attrs_changed = true;
                        }
                    }
                    if target != Target::Blocked {
                        handled[ri][pi][di] = true;
                    }
                }
            }

            // Consume parts whose declarations are all handled, and drop
            // declarations handled for every surviving part of a rule that
            // has no unsupported part.
            let mut ri = 0;
            for item in &mut items {
                let CssItem::Rule(rule) = item else { continue };
                for (pi, part) in rule.parts.iter_mut().enumerate() {
                    if part.parsed.is_some() && !part.removed && handled[ri][pi].iter().all(|&h| h)
                    {
                        part.removed = true;
                        part.consumed = true;
                        rules_changed = true;
                    }
                }
                let has_unsupported = rule.parts.iter().any(|p| p.parsed.is_none());
                if !has_unsupported {
                    let surviving: Vec<usize> = rule
                        .parts
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| !p.removed)
                        .map(|(i, _)| i)
                        .collect();
                    if !surviving.is_empty() {
                        let before = rule.declarations.len();
                        let keep: Vec<bool> = (0..before)
                            .map(|di| !surviving.iter().all(|&pi| handled[ri][pi][di]))
                            .collect();
                        let mut di = 0;
                        rule.declarations.retain(|_| {
                            let k = keep[di];
                            di += 1;
                            k
                        });
                        if rule.declarations.len() != before {
                            rules_changed = true;
                        }
                    }
                }
                ri += 1;
            }
        }

        // Step 8: strip class names that only consumed selectors used.
        let mut classes_changed = false;
        if !scripts && !has_at_rules && !attr_selected.contains("class") {
            let mut protected: HashSet<String> = HashSet::new();
            for item in &items {
                let CssItem::Rule(rule) = item else { continue };
                for part in rule.parts.iter().filter(|p| !p.removed) {
                    match &part.parsed {
                        Some(sel) => protected.extend(sel.classes.iter().cloned()),
                        None => protected.extend(extract_class_names(&part.text)),
                    }
                }
            }

            let mut removable: HashMap<NodeId, HashSet<String>> = HashMap::new();
            for item in &items {
                let CssItem::Rule(rule) = item else { continue };
                for part in rule.parts.iter().filter(|p| p.consumed) {
                    let Some(sel) = &part.parsed else { continue };
                    for &nid in &part.matched {
                        removable.entry(nid).or_default().extend(
                            sel.classes
                                .iter()
                                .filter(|c| !protected.contains(*c))
                                .cloned(),
                        );
                    }
                }
            }

            for (nid, classes) in removable {
                if classes.is_empty() {
                    continue;
                }
                let NodeKind::Element(ref mut elem) = doc.node_mut(nid).kind else {
                    continue;
                };
                let Some(pos) = elem
                    .attributes
                    .iter()
                    .position(|a| a.name == "class" && a.prefix.is_none())
                else {
                    continue;
                };
                let old_val = elem.attributes[pos].value.clone();
                let new_val = old_val
                    .split_whitespace()
                    .filter(|c| !classes.contains(*c))
                    .collect::<Vec<_>>()
                    .join(" ");
                if new_val.is_empty() {
                    elem.attributes.remove(pos);
                    classes_changed = true;
                } else if new_val != old_val {
                    elem.attributes[pos].value = new_val;
                    classes_changed = true;
                }
            }
        }

        // Step 9: write back what remains, or drop the <style> elements.
        let remaining = serialize_items(&items);
        let style_changed = if remaining.is_empty() {
            for &sid in &style_nodes {
                doc.remove(sid);
            }
            true
        } else if rules_changed || style_nodes.len() > 1 || media_attr_seen {
            for &sid in &style_nodes[1..] {
                doc.remove(sid);
            }
            let first = style_nodes[0];
            let children: Vec<NodeId> = doc.children(first).collect();
            for child in children {
                doc.remove(child);
            }
            let text_node = doc.alloc(NodeKind::Text(remaining));
            doc.append_child(first, text_node);
            if let NodeKind::Element(ref mut elem) = doc.node_mut(first).kind {
                elem.attributes
                    .retain(|a| !(a.name == "media" && a.prefix.is_none()));
            }
            true
        } else {
            false
        };

        if attrs_changed || classes_changed || style_changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

/// True when a declaration value carries `!important`.
fn is_important(value: &str) -> bool {
    value.to_ascii_lowercase().contains("!important")
}

/// True when a declaration of `declared` (possibly a shorthand) sets `prop`.
fn declares(declared: &str, prop: &str) -> bool {
    declared == prop
        || declared == "all"
        || (declared == "font" && prop.starts_with("font-"))
        || (declared == "marker" && prop.starts_with("marker-"))
}

/// Decide how a declaration of a supported rule may be inlined.
///
/// Kept parts that set the same property stay in the stylesheet. If every
/// such part outranks ours (higher specificity, same specificity but later,
/// or `!important`) a presentation attribute keeps the cascade intact: they
/// beat it, as they beat our rule. If every such part ranks below ours a
/// `style` attribute does: it beats them, as our rule did. A mix of the two
/// (or a part whose specificity is unknown, an attribute selector on the
/// property, a non-presentation property, or `!important`) cannot be
/// represented, so the declaration stays.
fn decide_target(
    prop: &str,
    val: &str,
    rule_idx: usize,
    spec: Specificity,
    kept: &[KeptPart],
    attr_selected: &HashSet<String>,
) -> Target {
    if is_important(val) || !PRESENTATION_ATTRS.contains(&prop) || attr_selected.contains(prop) {
        return Target::Blocked;
    }
    let mut higher = false;
    let mut lower = false;
    for k in kept {
        // Other parts of the same rule carry the same values.
        if k.rule_idx == rule_idx {
            continue;
        }
        for (declared, important) in &k.decls {
            if !declares(declared, prop) {
                continue;
            }
            let Some(kspec) = k.spec else {
                return Target::Blocked;
            };
            if *important || kspec > spec || (kspec == spec && k.rule_idx > rule_idx) {
                higher = true;
            } else {
                lower = true;
            }
        }
    }
    match (higher, lower) {
        (true, true) => Target::Blocked,
        (false, true) => Target::Style,
        _ => Target::Attr,
    }
}

/// Write `prop: val` onto element `nid`. Returns true when the element changed.
fn apply_declaration(
    doc: &mut Document,
    nid: NodeId,
    prop: &str,
    val: &str,
    target: Target,
) -> bool {
    let NodeKind::Element(ref mut elem) = doc.node_mut(nid).kind else {
        return false;
    };
    let attr_pos = elem
        .attributes
        .iter()
        .position(|a| a.name == prop && a.prefix.is_none());
    match target {
        Target::Attr => {
            if let Some(pos) = attr_pos {
                if elem.attributes[pos].value == val {
                    return false;
                }
                elem.attributes[pos].value = val.to_string();
            } else {
                elem.attributes.push(Attribute {
                    prefix: None,
                    name: prop.to_string(),
                    value: val.to_string(),
                });
            }
            true
        }
        Target::Style => {
            // The style attribute wins over the presentation attribute, which
            // becomes dead weight.
            if let Some(pos) = attr_pos {
                elem.attributes.remove(pos);
            }
            let decl = format!("{prop}:{val}");
            if let Some(style) = elem
                .attributes
                .iter_mut()
                .find(|a| a.name == "style" && a.prefix.is_none())
            {
                let base = style.value.trim().trim_end_matches(';').to_string();
                style.value = if base.is_empty() {
                    decl
                } else {
                    format!("{base};{decl}")
                };
            } else {
                elem.attributes.push(Attribute {
                    prefix: None,
                    name: "style".to_string(),
                    value: decl,
                });
            }
            true
        }
        Target::Blocked => false,
    }
}

// --- CSS parser ---

/// Parse CSS text into rules and opaque @-rule blocks, in source order.
fn parse_css(css: &str) -> Option<Vec<CssItem>> {
    let stripped = strip_css_comments(css);
    let input = stripped.trim();
    let bytes = input.as_bytes();
    let mut items = Vec::new();
    let mut pos = 0;

    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        if bytes[pos] == b'@' {
            let start = pos;
            pos = skip_at_rule(input, pos)?;
            items.push(CssItem::AtRule(input[start..pos].to_string()));
            continue;
        }

        let open = find_char(input, pos, b'{')?;
        let selector = input[pos..open].trim();
        if selector.is_empty() {
            return None;
        }
        let close = find_matching_brace(input, open)?;
        let declarations = parse_declarations(input[open + 1..close].trim());

        let parts: Vec<SelectorPart> = split_selector_list(selector)
            .into_iter()
            .map(|p| {
                let text = p.trim().to_string();
                SelectorPart {
                    parsed: parse_simple_selector(&text),
                    text,
                    matched: Vec::new(),
                    removed: false,
                    consumed: false,
                }
            })
            .collect();
        if parts.iter().any(|p| p.text.is_empty()) {
            return None;
        }

        items.push(CssItem::Rule(CssRule {
            parts,
            declarations,
        }));
        pos = close + 1;
    }

    Some(items)
}

/// Find a byte in the string starting at `from`.
fn find_char(s: &str, from: usize, ch: u8) -> Option<usize> {
    s.as_bytes()[from..]
        .iter()
        .position(|&b| b == ch)
        .map(|p| p + from)
}

/// Find the `}` matching the `{` at `open`, skipping nested blocks and
/// quoted strings.
fn find_matching_brace(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1;
    let mut quote: Option<u8> = None;
    let mut i = open + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
        } else {
            match b {
                b'"' | b'\'' => quote = Some(b),
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Skip an @-rule starting at `pos`: up to `;` for statements like
/// `@import`, or past the matching `}` for blocks like `@media`.
fn skip_at_rule(s: &str, pos: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = pos + 1;
    while i < bytes.len() {
        match bytes[i] {
            b';' => return Some(i + 1),
            b'{' => return find_matching_brace(s, i).map(|c| c + 1),
            _ => i += 1,
        }
    }
    Some(i)
}

/// Parse `property: value; property: value` into pairs, honouring quotes
/// and parentheses (`url(data:...;base64,...)`).
pub(crate) fn parse_declarations(block: &str) -> Vec<(String, String)> {
    split_declarations(block)
        .into_iter()
        .filter_map(split_property)
        .filter(|(_, val)| !val.is_empty())
        .map(|(prop, val)| (prop.to_string(), val.to_string()))
        .collect()
}

// --- Selectors ---

/// Read a CSS identifier starting at `*i`, decoding backslash escapes.
/// Returns `None` for an empty identifier or a dangling backslash.
fn read_ident(chars: &[char], i: &mut usize) -> Option<String> {
    let mut out = String::new();
    while *i < chars.len() {
        let c = chars[*i];
        if c == '\\' {
            *i += 1;
            if *i >= chars.len() {
                return None;
            }
            let hex_start = *i;
            while *i < chars.len() && *i - hex_start < 6 && chars[*i].is_ascii_hexdigit() {
                *i += 1;
            }
            if *i > hex_start {
                let hex: String = chars[hex_start..*i].iter().collect();
                let decoded = u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or('\u{FFFD}');
                out.push(decoded);
                if *i < chars.len() && chars[*i].is_whitespace() {
                    *i += 1;
                }
            } else {
                out.push(chars[*i]);
                *i += 1;
            }
        } else if is_css_ident_char(c) {
            out.push(c);
            *i += 1;
        } else {
            break;
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Parse a compound selector made only of a type, classes and an id in any
/// order (`rect.a.b#x`, `#x.a`, `.a`). Anything else — combinators, pseudo
/// classes, attribute selectors, `*`, two ids — is unsupported.
fn parse_simple_selector(sel: &str) -> Option<SimpleSelector> {
    let chars: Vec<char> = sel.chars().collect();
    let mut i = 0;
    let mut out = SimpleSelector {
        elem: None,
        classes: Vec::new(),
        id: None,
    };
    if chars.first().is_some_and(|&c| c != '.' && c != '#') {
        out.elem = Some(read_ident(&chars, &mut i)?);
    }
    while i < chars.len() {
        match chars[i] {
            '.' => {
                i += 1;
                out.classes.push(read_ident(&chars, &mut i)?);
            }
            '#' => {
                i += 1;
                if out.id.is_some() {
                    return None;
                }
                out.id = Some(read_ident(&chars, &mut i)?);
            }
            _ => return None,
        }
    }
    if out.elem.is_none() && out.classes.is_empty() && out.id.is_none() {
        return None;
    }
    Some(out)
}

/// Elements matching a simple selector, in document order.
fn match_simple_selector(
    sel: &SimpleSelector,
    class_map: &HashMap<String, Vec<NodeId>>,
    id_map: &HashMap<String, Vec<NodeId>>,
    all_ids: &[NodeId],
    doc: &Document,
) -> Vec<NodeId> {
    let candidates: Vec<NodeId> = if let Some(id) = &sel.id {
        id_map.get(id).cloned().unwrap_or_default()
    } else if let Some(cls) = sel.classes.first() {
        class_map.get(cls).cloned().unwrap_or_default()
    } else {
        all_ids.to_vec()
    };

    candidates
        .into_iter()
        .filter(|&nid| {
            let NodeKind::Element(ref e) = doc.node(nid).kind else {
                return false;
            };
            if let Some(name) = &sel.elem
                && (e.name != *name || e.prefix.is_some())
            {
                return false;
            }
            if let Some(id) = &sel.id
                && e.attr("id") != Some(id)
            {
                return false;
            }
            if !sel.classes.is_empty() {
                let Some(val) = e.attr("class") else {
                    return false;
                };
                let have: Vec<&str> = val.split_whitespace().collect();
                if !sel.classes.iter().all(|c| have.contains(&c.as_str())) {
                    return false;
                }
            }
            true
        })
        .collect()
}

/// Class names mentioned anywhere in a selector (`.a`, `g .a:hover`,
/// `:not(.a)`), decoding escapes.
fn extract_class_names(selector: &str) -> Vec<String> {
    let chars: Vec<char> = selector.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '.' {
            i += 1;
            if let Some(name) = read_ident(&chars, &mut i) {
                out.push(name);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Pseudo-classes whose argument is a selector list and whose specificity
/// is that of their most specific alternative; the name itself adds nothing.
const LOGICAL_PSEUDOS: &[&str] = &[
    "is",
    "not",
    "has",
    "matches",
    "any",
    "-webkit-any",
    "-moz-any",
];

/// Functional pseudo-classes counting (0,1,0) whose argument is not a
/// selector list.
const PLAIN_FUNCTIONAL_PSEUDOS: &[&str] =
    &["nth-of-type", "nth-last-of-type", "lang", "dir", "state"];

/// Pseudo-elements that may be written with a single colon.
const LEGACY_PSEUDO_ELEMENTS: &[&str] = &["before", "after", "first-line", "first-letter"];

/// Index of the `)` matching the `(` at `open`, honouring quotes.
fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0u32;
    let mut quote: Option<char> = None;
    let mut i = open;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' {
                i += 1;
            } else if c == q {
                quote = None;
            }
        } else {
            match c {
                '\\' => i += 1,
                '"' | '\'' => quote = Some(c),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Index just past the `]` closing the attribute selector at `open`.
fn skip_attribute_selector(chars: &[char], open: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut i = open + 1;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' {
                i += 1;
            } else if c == q {
                quote = None;
            }
        } else {
            match c {
                '\\' => i += 1,
                '"' | '\'' => quote = Some(c),
                ']' => return Some(i + 1),
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn add_specificity(a: Specificity, b: Specificity) -> Specificity {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

/// Specificity of a selector list: that of its most specific alternative.
/// `None` when any alternative is not understood.
fn selector_list_specificity(list: &str) -> Option<Specificity> {
    let mut best = (0, 0, 0);
    for part in split_selector_list(list) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        best = best.max(selector_specificity(part)?);
    }
    Some(best)
}

/// Exact specificity of one complex selector per CSS Selectors 4, or `None`
/// when the selector is not understood (unknown functional pseudo-class,
/// nesting, stray punctuation). Never an estimate: the cascade decisions in
/// [`decide_target`] are only safe when this is exact.
fn selector_specificity(selector: &str) -> Option<Specificity> {
    let chars: Vec<char> = selector.trim().chars().collect();
    let mut spec = (0, 0, 0);
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '#' => {
                i += 1;
                read_ident(&chars, &mut i)?;
                spec.0 += 1;
            }
            '.' => {
                i += 1;
                read_ident(&chars, &mut i)?;
                spec.1 += 1;
            }
            '[' => {
                i = skip_attribute_selector(&chars, i)?;
                spec.1 += 1;
            }
            ':' => {
                i += 1;
                let pseudo_element = chars.get(i) == Some(&':');
                if pseudo_element {
                    i += 1;
                }
                let name = read_ident(&chars, &mut i)?.to_ascii_lowercase();
                let arg: Option<String> = if chars.get(i) == Some(&'(') {
                    let close = matching_paren(&chars, i)?;
                    let a = chars[i + 1..close].iter().collect();
                    i = close + 1;
                    Some(a)
                } else {
                    None
                };
                if pseudo_element {
                    // `::slotted(S)`, `::part(x)` are not understood.
                    if arg.is_some() {
                        return None;
                    }
                    spec.2 += 1;
                } else if LOGICAL_PSEUDOS.contains(&name.as_str()) {
                    spec = add_specificity(spec, selector_list_specificity(&arg?)?);
                } else if name == "where" {
                    arg?;
                } else if name == "nth-child" || name == "nth-last-child" {
                    let a = arg?;
                    spec.1 += 1;
                    // `:nth-child(An+B of S)` adds the specificity of S.
                    if let Some(pos) = a.to_ascii_lowercase().find(" of ") {
                        spec = add_specificity(spec, selector_list_specificity(&a[pos + 4..])?);
                    }
                } else if PLAIN_FUNCTIONAL_PSEUDOS.contains(&name.as_str()) {
                    arg?;
                    spec.1 += 1;
                } else if arg.is_some() {
                    // Unknown functional pseudo-class (`:host(S)`, ...).
                    return None;
                } else if LEGACY_PSEUDO_ELEMENTS.contains(&name.as_str()) {
                    spec.2 += 1;
                } else {
                    spec.1 += 1;
                }
            }
            '*' => i += 1,
            // Namespace separator: `*|rect`, `|rect`.
            '|' => i += 1,
            c if c.is_whitespace() || c == '>' || c == '+' || c == '~' => i += 1,
            c if c == '\\' || is_css_ident_char(c) => {
                read_ident(&chars, &mut i)?;
                if chars.get(i) == Some(&'|') {
                    // `svg|rect`: a namespace prefix, not a type.
                    i += 1;
                } else {
                    spec.2 += 1;
                }
            }
            _ => return None,
        }
    }
    Some(spec)
}

/// Serialize the surviving rules and @-rule blocks in source order.
fn serialize_items(items: &[CssItem]) -> String {
    let mut out = String::new();
    for item in items {
        match item {
            CssItem::AtRule(text) => out.push_str(text),
            CssItem::Rule(rule) => {
                let parts: Vec<&str> = rule
                    .parts
                    .iter()
                    .filter(|p| !p.removed)
                    .map(|p| p.text.as_str())
                    .collect();
                if parts.is_empty() {
                    continue;
                }
                out.push_str(&parts.join(","));
                out.push('{');
                for (i, (prop, val)) in rule.declarations.iter().enumerate() {
                    if i > 0 {
                        out.push(';');
                    }
                    out.push_str(prop);
                    out.push(':');
                    out.push_str(val);
                }
                out.push('}');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = InlineStyles.run(&mut doc);
        (result, serialize(&doc))
    }

    fn svg(body: &str) -> String {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{body}</svg>")
    }

    fn sel(s: &str) -> SimpleSelector {
        parse_simple_selector(s).unwrap_or_else(|| panic!("{s} should parse"))
    }

    #[test]
    fn no_style_element_unchanged() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn merge_two_style_elements() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{fill:red}</style><style>.b{fill:blue}</style><rect class=\"a\" width=\"10\" height=\"10\"/><rect class=\"b\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<style>"));
        assert!(output.contains("fill=\"red\""));
        assert!(output.contains("fill=\"blue\""));
    }

    #[test]
    fn simple_class_inlining() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red}</style><rect class=\"cls\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<style>"));
        assert!(!output.contains("class="));
        assert!(output.contains("fill=\"red\""));
    }

    #[test]
    fn id_selector_inlining() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>#my{fill:red}</style><rect id=\"my\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<style>"));
        assert!(output.contains("fill=\"red\""));
    }

    #[test]
    fn partial_inline_multiple_classes() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{fill:red}.b:hover{opacity:.5}</style><rect class=\"a b\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // .b:hover does not touch fill, so a presentation attribute is safe.
        assert!(output.contains("fill=\"red\""), "{output}");
        assert!(output.contains("class=\"b\""), "{output}");
        assert!(output.contains(".b:hover{opacity:.5}"), "{output}");
    }

    #[test]
    fn css_overrides_presentation_attribute() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red}</style><rect class=\"cls\" fill=\"blue\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("fill=\"red\""));
        assert!(!output.contains("fill=\"blue\""));
    }

    #[test]
    fn style_attr_shadows_rule_declaration() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red;stroke:blue}</style><rect class=\"cls\" style=\"fill:green\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // fill is dead on the only matched element; stroke is inlined. The
        // rule is fully consumed, as in SVGO.
        assert!(output.contains("stroke=\"blue\""), "{output}");
        assert!(output.contains("style=\"fill:green\""), "{output}");
        assert!(!output.contains("<style>"), "{output}");
        assert!(!output.contains("class="), "{output}");
    }

    #[test]
    fn at_media_blocks_inlining_but_unused_rules_go() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>@media(prefers-color-scheme:dark){.bg{fill:#000}}.bg{fill:#fff}.unused{fill:red}</style><rect class=\"bg\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<style>"));
        assert!(output.contains(".bg{fill:#fff}"));
        assert!(!output.contains(".unused"));
        assert!(output.contains("class=\"bg\""), "{output}");
        // Source order of @-rules and rules is preserved.
        assert!(
            output.contains("@media(prefers-color-scheme:dark){.bg{fill:#000}}.bg{fill:#fff}"),
            "{output}"
        );
    }

    #[test]
    fn pseudo_selector_preserved() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a:hover{opacity:.5}</style><rect class=\"a\" width=\"10\" height=\"10\"/></svg>";
        let (result, _output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn unused_rule_removed() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.used{fill:red}.unused{fill:blue}</style><rect class=\"used\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("unused"));
        assert!(!output.contains("blue"));
    }

    #[test]
    fn cdata_content_handled() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style><![CDATA[.cls{fill:red}]]></style><rect class=\"cls\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("fill=\"red\""));
        assert!(!output.contains("<style>"));
    }

    #[test]
    fn non_presentation_property_skipped() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red;custom-thing:val}</style><rect class=\"cls\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("fill=\"red\""));
        // The inlined declaration leaves the rule; the rest stays.
        assert!(output.contains(".cls{custom-thing:val}"), "{output}");
        assert!(output.contains("class=\"cls\""), "{output}");
    }

    #[test]
    fn empty_style_removed() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red}</style><rect class=\"cls\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<style>"));
        assert!(!output.contains("</style>"));
    }

    #[test]
    fn type_text_xsl_skipped() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style type=\"text/xsl\">.cls{fill:red}</style><rect class=\"cls\" width=\"10\" height=\"10\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn empty_type_attribute_is_css() {
        let (result, output) = run_pass(&svg(
            "<style type=\"\">.cls{fill:red}</style><rect class=\"cls\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("fill=\"red\""), "{output}");
    }

    #[test]
    fn selector_list_inlined() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a,.b{fill:red}</style><rect class=\"a\" width=\"10\" height=\"10\"/><rect class=\"b\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<style>"));
        assert_eq!(output.matches("fill=\"red\"").count(), 2);
    }

    #[test]
    fn selector_list_with_unsupported_part_keeps_declarations() {
        // `.a` is inlined and leaves the list; `.b:hover` keeps the rule.
        let (result, output) = run_pass(&svg(
            "<style>.a,.b:hover{fill:red}</style><rect class=\"a\"/><rect class=\"b\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains(".b:hover{fill:red}"), "{output}");
        assert!(!output.contains(".a,"), "{output}");
        assert!(output.contains("<rect fill=\"red\"/>"), "{output}");
        assert!(output.contains("class=\"b\""), "{output}");
    }

    #[test]
    fn element_type_selector() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>rect{fill:red}</style><rect width=\"10\" height=\"10\"/><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<rect width=\"10\" height=\"10\" fill=\"red\"/>"));
        assert!(output.contains("<circle cx=\"5\" cy=\"5\" r=\"5\"/>"));
        assert!(!output.contains("<style>"));
    }

    #[test]
    fn element_class_compound_selector() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>rect.special{fill:red}</style><rect class=\"special\" width=\"10\" height=\"10\"/><circle class=\"special\" cx=\"5\" cy=\"5\" r=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("fill=\"red\"").count(), 1);
        // The circle keeps its class: it was never matched.
        assert!(output.contains("<circle class=\"special\""), "{output}");
    }

    #[test]
    fn convergence_test() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls-1{fill:red;opacity:1}.cls-2{stroke:blue;stroke-width:2}</style><rect class=\"cls-1\" width=\"100\" height=\"100\"/><circle class=\"cls-2\" cx=\"50\" cy=\"50\" r=\"30\"/></svg>";
        let result1 = crate::optimize(input).unwrap();
        let result2 = crate::optimize(&result1.data).unwrap();
        assert_eq!(result1.data, result2.data, "should converge");
    }

    // --- WP6-31: compound selectors and non-ASCII identifiers ---

    #[test]
    fn parse_simple_selector_shapes() {
        assert_eq!(
            sel("rect.a.b#x"),
            SimpleSelector {
                elem: Some("rect".into()),
                classes: vec!["a".into(), "b".into()],
                id: Some("x".into())
            }
        );
        assert_eq!(sel("#x.a").id.as_deref(), Some("x"));
        assert_eq!(sel("#x.a").classes, vec!["a".to_string()]);
        assert_eq!(sel(".a#x").id.as_deref(), Some("x"));
        assert_eq!(sel(".ǎx").classes, vec!["ǎx".to_string()]);
        assert_eq!(sel("#ǎx").id.as_deref(), Some("ǎx"));
        assert_eq!(sel(r".a\:b").classes, vec!["a:b".to_string()]);
        assert_eq!(sel(r".\31 0").classes, vec!["10".to_string()]);
        assert_eq!(sel("rect.a.b").specificity(), (0, 2, 1));
        assert_eq!(sel("#x").specificity(), (1, 0, 0));
        for bad in [
            "#a#b", "..a", ".a.", "a.", "*", "", "g rect", ".a>.b", ".a:hover", "[x]", ".a,",
            "a\\", ".a b",
        ] {
            assert!(parse_simple_selector(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn multi_class_selector_requires_all_classes() {
        let (result, output) = run_pass(&svg(
            "<style>.a.b{fill:red}</style><rect class=\"a\"/><rect class=\"a b\"/><rect class=\"b a\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<rect class=\"a\"/>"), "{output}");
        assert_eq!(output.matches("fill=\"red\"").count(), 2, "{output}");
        assert!(!output.contains("<style>"), "{output}");
        assert!(!output.contains("class=\"a b\""), "{output}");
        assert!(!output.contains("class=\"b a\""), "{output}");
    }

    #[test]
    fn type_and_two_classes() {
        let (result, output) = run_pass(&svg(
            "<style>rect.a.b{fill:red}</style><rect class=\"a b\"/><circle class=\"a b\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<rect fill=\"red\"/>"), "{output}");
        assert!(output.contains("<circle class=\"a b\"/>"), "{output}");
    }

    #[test]
    fn id_with_class_selectors() {
        for css in ["#x.a{fill:red}", "rect#x.a{fill:red}", ".a#x{fill:red}"] {
            let (result, output) = run_pass(&svg(&format!(
                "<style>{css}</style><rect id=\"x\" class=\"a\"/><rect id=\"y\" class=\"a\"/>"
            )));
            assert_eq!(result, PassResult::Changed, "{css}");
            assert!(
                output.contains("<rect id=\"x\" fill=\"red\"/>"),
                "{css}: {output}"
            );
            assert!(
                output.contains("<rect id=\"y\" class=\"a\"/>"),
                "{css}: {output}"
            );
            assert!(!output.contains("<style>"), "{css}: {output}");
        }
        // The rule is unused when the id carries a different class.
        let (_, output) = run_pass(&svg(
            "<style>#x.a{fill:red}</style><rect id=\"x\" class=\"b\"/>",
        ));
        assert!(!output.contains("<style>"), "{output}");
        assert!(output.contains("class=\"b\""), "{output}");
    }

    #[test]
    fn svgo_fixture_inline_styles_21_segment_minor() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.segment.minor{stroke-width:1.5;stroke:#15c6aa}</style><g transform=\"translate(200.662 362.87)\"><path d=\"M163.502-303.979h3.762\" class=\"segment minor\"/></g></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<style>"), "{output}");
        assert!(!output.contains("class="), "{output}");
        assert!(output.contains("stroke-width=\"1.5\""), "{output}");
        assert!(output.contains("stroke=\"#15c6aa\""), "{output}");
    }

    #[test]
    fn non_ascii_class_and_id_inlined() {
        let (result, output) = run_pass(&svg(
            "<style>.ǎccent{fill:red}</style><rect class=\"ǎccent\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, svg("<rect fill=\"red\"/>"));

        let (result, output) = run_pass(&svg("<style>#ǎ{fill:red}</style><rect id=\"ǎ\"/>"));
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, svg("<rect id=\"ǎ\" fill=\"red\"/>"));
    }

    #[test]
    fn non_ascii_pseudo_class_keeps_class_and_rule() {
        let input = svg("<style>.ǎx:hover{fill:red}</style><rect class=\"ǎx\"/>");
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }

    #[test]
    fn escaped_class_selector_matches_literal_class() {
        let (result, output) =
            run_pass(&svg(r#"<style>.a\:b{fill:red}</style><rect class="a:b"/>"#));
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, svg("<rect fill=\"red\"/>"));
    }

    // --- WP6-32: specificity and remaining-rule interplay ---

    #[test]
    fn class_beats_type_regardless_of_order() {
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}rect{fill:blue}</style><rect class=\"a\"/>",
        ));
        assert!(output.contains("fill=\"red\""), "{output}");
        assert!(!output.contains("blue"), "{output}");
        assert!(!output.contains("<style>"), "{output}");
    }

    #[test]
    fn id_beats_class_regardless_of_order() {
        let (_, output) = run_pass(&svg(
            "<style>#x{fill:red}.a{fill:blue}</style><rect id=\"x\" class=\"a\"/>",
        ));
        assert!(output.contains("fill=\"red\""), "{output}");
        assert!(!output.contains("blue"), "{output}");
        assert!(!output.contains("<style>"), "{output}");
    }

    #[test]
    fn compound_beats_single_class() {
        let (_, output) = run_pass(&svg(
            "<style>rect.a{fill:red}.a{fill:blue}</style><rect class=\"a\"/><circle class=\"a\"/>",
        ));
        assert!(output.contains("<rect fill=\"red\"/>"), "{output}");
        assert!(output.contains("<circle fill=\"blue\"/>"), "{output}");
        assert!(!output.contains("<style>"), "{output}");
    }

    #[test]
    fn equal_specificity_later_rule_wins() {
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}.b{fill:blue}</style><rect class=\"a b\"/>",
        ));
        assert!(output.contains("fill=\"blue\""), "{output}");
        assert!(!output.contains("red"), "{output}");
        assert!(!output.contains("<style>"), "{output}");

        let (_, output) = run_pass(&svg(
            "<style>.b{fill:blue}.a{fill:red}</style><rect class=\"a b\"/>",
        ));
        assert!(output.contains("fill=\"red\""), "{output}");
        assert!(!output.contains("<style>"), "{output}");
    }

    #[test]
    fn lower_unsupported_rule_forces_style_attribute() {
        // `.a` (0,1,0) beat `svg path` (0,0,2); a presentation attribute
        // would lose to the remaining rule, a style attribute does not.
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}svg path{fill:blue}</style><path class=\"a\" d=\"M0 0h10\"/>",
        ));
        assert!(output.contains("style=\"fill:red\""), "{output}");
        assert!(!output.contains("fill=\"red\""), "{output}");
        assert!(
            output.contains("<style>svg path{fill:blue}</style>"),
            "{output}"
        );
        assert!(!output.contains("class="), "{output}");
    }

    #[test]
    fn style_attribute_replaces_presentation_attribute() {
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}svg path{fill:blue}</style><path class=\"a\" fill=\"green\" d=\"M0 0h10\"/>",
        ));
        assert!(output.contains("style=\"fill:red\""), "{output}");
        assert!(!output.contains("green"), "{output}");
    }

    #[test]
    fn higher_unsupported_rule_allows_presentation_attribute() {
        // `.a:hover` outranks `.a` and must keep winning on hover: a
        // presentation attribute ranks below it, exactly like `.a` did.
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}.a:hover{fill:blue}</style><rect class=\"a\"/>",
        ));
        assert!(output.contains("fill=\"red\""), "{output}");
        assert!(output.contains("class=\"a\""), "{output}");
        assert!(
            output.contains("<style>.a:hover{fill:blue}</style>"),
            "{output}"
        );
    }

    #[test]
    fn mixed_unsupported_rules_block_inlining() {
        let input = svg(
            "<style>.a{fill:red}svg rect{fill:blue}.a:hover{fill:green}</style><rect class=\"a\"/>",
        );
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn important_in_unsupported_rule_counts_as_higher() {
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}svg rect{fill:blue!important}</style><rect class=\"a\"/>",
        ));
        assert!(output.contains("fill=\"red\""), "{output}");
        assert!(output.contains("svg rect{fill:blue!important}"), "{output}");
    }

    #[test]
    fn unsupported_rule_without_the_property_is_irrelevant() {
        let (_, output) = run_pass(&svg(
            "<style>#k{fill:red}svg path{stroke:blue}</style><path id=\"k\"/>",
        ));
        assert!(output.contains("<path id=\"k\" fill=\"red\"/>"), "{output}");
        assert!(
            output.contains("<style>svg path{stroke:blue}</style>"),
            "{output}"
        );
    }

    #[test]
    fn shorthand_in_unsupported_rule_covers_longhands() {
        let (_, output) = run_pass(&svg(
            "<style>.a{font-size:10px}svg text{font:12px serif}</style><text class=\"a\">x</text>",
        ));
        assert!(output.contains("style=\"font-size:10px\""), "{output}");
    }

    #[test]
    fn attribute_selector_blocks_property() {
        let input = svg("<style>.a{fill:red}[fill]{stroke:purple}</style><rect class=\"a\"/>");
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged, "{output}");

        let input = svg(
            "<style>.a{fill:red}path:not([fill=blue]){stroke:purple}</style><path class=\"a\"/>",
        );
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged, "{output}");

        // Other properties of the same rule are still inlined.
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red;stroke:blue}[fill]{opacity:.5}</style><rect class=\"a\"/>",
        ));
        assert!(output.contains("stroke=\"blue\""), "{output}");
        assert!(!output.contains("fill=\"red\""), "{output}");
        assert!(output.contains(".a{fill:red}"), "{output}");
        assert!(output.contains("class=\"a\""), "{output}");
    }

    #[test]
    fn blocked_higher_rule_shadows_lower_rule() {
        // `.a.b` keeps `fill:red!important` in the stylesheet; the lower
        // `.a{fill:blue}` is dead on that element and must not be inlined
        // where it would override.
        let (_, output) = run_pass(&svg(
            "<style>.a.b{fill:red!important}.a{fill:blue}</style><rect class=\"a b\"/><rect class=\"a\"/>",
        ));
        assert!(output.contains("<rect class=\"a b\"/>"), "{output}");
        // Class `a` stays: the surviving `.a.b` rule still references it.
        assert!(
            output.contains("<rect class=\"a\" fill=\"blue\"/>"),
            "{output}"
        );
        assert!(output.contains(".a.b{fill:red!important}"), "{output}");
        assert!(!output.contains(".a{"), "{output}");
    }

    // --- WP6-30 / WP6-34: @-rules and media attributes ---

    #[test]
    fn keyframes_only_stylesheet_kept() {
        let input = svg(
            "<style>@keyframes spin{to{transform:rotate(360deg)}}</style><rect class=\"spin\"/>",
        );
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
        assert_eq!(output, input);
    }

    #[test]
    fn font_face_kept_and_unused_class_rule_dropped() {
        let (result, output) = run_pass(&svg(
            "<style>@font-face{font-family:X;src:url(x.woff)}.unused{fill:red}</style><text class=\"t\">x</text>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains("<style>@font-face{font-family:X;src:url(x.woff)}</style>"),
            "{output}"
        );
        assert!(output.contains("class=\"t\""), "{output}");
    }

    #[test]
    fn media_print_only_rule_kept_with_class() {
        let input = svg("<style>@media print{.a{fill:red}}</style><rect class=\"a\"/>");
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn media_block_protects_class_used_only_inside_it() {
        let (result, output) = run_pass(&svg(
            "<style>@media print{.a{fill:red}}.b{stroke:blue}</style><rect class=\"a b\"/>",
        ));
        assert_eq!(result, PassResult::Unchanged, "{output}");
        assert!(output.contains("class=\"a b\""), "{output}");
    }

    #[test]
    fn media_print_rule_keeps_class_on_every_element() {
        let (result, output) = run_pass(&svg(
            "<style>@media print{.p{fill:red}}</style><rect class=\"p\"/><rect class=\"p\"/>",
        ));
        assert_eq!(result, PassResult::Unchanged, "{output}");
        assert_eq!(output.matches("class=\"p\"").count(), 2);
    }

    #[test]
    fn style_media_attribute_becomes_media_block() {
        let (result, output) = run_pass(&svg(
            "<style media=\"print\">.st0{fill:blue}</style><style media=\"screen\">.st0{fill:red}</style><rect class=\"st0\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains(
                "<style>@media print{.st0{fill:blue}}@media screen{.st0{fill:red}}</style>"
            ),
            "{output}"
        );
        assert!(output.contains("class=\"st0\""), "{output}");
        assert!(!output.contains("media="), "{output}");
    }

    #[test]
    fn style_media_hover_rules_stay_separately_scoped() {
        let (_, output) = run_pass(&svg(
            "<style media=\"print\">.a:hover{fill:blue}</style><style media=\"screen\">.a:hover{fill:red}</style><rect class=\"a\"/>",
        ));
        assert!(
            output.contains("@media print{.a:hover{fill:blue}}@media screen{.a:hover{fill:red}}"),
            "{output}"
        );
    }

    #[test]
    fn single_media_print_sheet_not_inlined() {
        let (result, output) = run_pass(&svg(
            "<style media=\"print\">.a{fill:red}</style><rect class=\"a\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            svg("<style>@media print{.a{fill:red}}</style><rect class=\"a\"/>")
        );
        // Idempotent.
        let (result, again) = run_pass(&output);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(again, output);
    }

    #[test]
    fn media_all_is_unscoped() {
        let (result, output) = run_pass(&svg(
            "<style media=\"all\">.a{fill:red}</style><rect class=\"a\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, svg("<rect fill=\"red\"/>"));
    }

    #[test]
    fn mixed_keyframes_and_used_rule_unchanged() {
        let input =
            svg("<style>@keyframes k{to{opacity:0}}.b{fill:red}</style><rect class=\"b\"/>");
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
        assert_eq!(output, input);
    }

    // --- WP6-33: class cleanup and scripts ---

    #[test]
    fn svgo_fixture_inline_styles_23_empty_rule_drops_only_its_class() {
        let (result, output) = run_pass(&svg(
            "<style>.a {}</style><g class=\"a\"><circle class=\"b\" cx=\"42.97\" cy=\"24.92\" r=\"1.14\"/></g>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            svg("<g><circle class=\"b\" cx=\"42.97\" cy=\"24.92\" r=\"1.14\"/></g>")
        );
    }

    #[test]
    fn whitespace_only_style_keeps_classes() {
        let (result, output) = run_pass(&svg("<style>  \n </style><rect class=\"st0\"/>"));
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output, svg("<rect class=\"st0\"/>"));
    }

    #[test]
    fn unmatched_elements_keep_their_classes() {
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}</style><rect class=\"a\"/><rect class=\"js-hook\"/>",
        ));
        assert!(output.contains("<rect fill=\"red\"/>"), "{output}");
        assert!(output.contains("class=\"js-hook\""), "{output}");
    }

    #[test]
    fn script_keeps_unused_rules_and_classes() {
        let (result, output) = run_pass(&svg(
            "<script>document.querySelector('.js-hook')</script><style>.a{fill:red}.hl{stroke:blue}</style><rect class=\"a js-hook\"/>",
        ));
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("fill=\"red\""), "{output}");
        assert!(output.contains("class=\"a js-hook\""), "{output}");
        assert!(
            output.contains("<style>.hl{stroke:blue}</style>"),
            "{output}"
        );
    }

    #[test]
    fn event_handler_and_javascript_link_keep_unused_rules() {
        for body in [
            "<style>.hl{stroke:blue}</style><rect onclick=\"f()\"/>",
            "<style>.hl{stroke:blue}</style><a href=\"javascript:f()\"><rect/></a>",
        ] {
            let input = svg(body);
            let (result, output) = run_pass(&input);
            assert_eq!(result, PassResult::Unchanged, "{output}");
            assert!(output.contains(".hl{stroke:blue}"), "{output}");
        }
    }

    #[test]
    fn class_attribute_selector_disables_class_cleanup() {
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}rect[class~=a]{stroke:blue}</style><rect class=\"a\"/>",
        ));
        assert!(output.contains("class=\"a\""), "{output}");
        assert!(output.contains("rect[class~=a]{stroke:blue}"), "{output}");
    }

    #[test]
    fn class_used_by_unsupported_descendant_selector_is_protected() {
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red}.a rect{stroke:blue}</style><g class=\"a\"><rect/></g>",
        ));
        assert!(output.contains("<g class=\"a\" fill=\"red\">"), "{output}");
        assert!(
            output.contains("<style>.a rect{stroke:blue}</style>"),
            "{output}"
        );
    }

    // --- WP6-35: foreignObject ---

    #[test]
    fn svgo_fixture_inline_styles_17_foreign_object_style_untouched() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><foreignObject width=\"100%\" height=\"100%\"><style>div { color: red; }</style><body xmlns=\"http://www.w3.org/1999/xhtml\"><div>hello, world</div></body></foreignObject></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
        assert_eq!(output, input);
    }

    #[test]
    fn svgo_fixture_merge_styles_12_foreign_object_style_not_merged() {
        let input = svg(
            "<foreignObject><style>.st0{fill:yellow}</style></foreignObject><style>.st1{fill:red}</style><rect class=\"st1\"/>",
        );
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            svg(
                "<foreignObject><style>.st0{fill:yellow}</style></foreignObject><rect fill=\"red\"/>"
            )
        );
    }

    #[test]
    fn svg_rule_matching_html_inside_foreign_object_is_preserved() {
        let input = svg(
            "<style>div{color:red}.a{fill:red}</style><rect class=\"a\"/><foreignObject><div xmlns=\"http://www.w3.org/1999/xhtml\" class=\"a\">x</div></foreignObject>",
        );
        let (_, output) = run_pass(&input);
        assert!(
            output.contains("<style>div{color:red}.a{fill:red}</style>"),
            "{output}"
        );
        assert!(
            output.contains("<div xmlns=\"http://www.w3.org/1999/xhtml\" class=\"a\">x</div>"),
            "{output}"
        );
        assert!(output.contains("<rect class=\"a\"/>"), "{output}");
    }

    // --- WP6-36: declaration splitting ---

    #[test]
    fn data_url_with_semicolon_inlined_whole() {
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:url(data:image/svg+xml;base64,PHN2Zy8+);stroke:red}</style><rect class=\"a\"/>",
        ));
        assert!(
            output.contains("fill=\"url(data:image/svg+xml;base64,PHN2Zy8+)\""),
            "{output}"
        );
        assert!(output.contains("stroke=\"red\""), "{output}");
        assert!(!output.contains("<style>"), "{output}");
    }

    #[test]
    fn quoted_font_family_with_semicolon_inlined_whole() {
        let (_, output) = run_pass(&svg(
            "<style>.a{font-family:\"a;b\";fill:red}</style><text class=\"a\">x</text>",
        ));
        assert!(
            output.contains("font-family=\"&quot;a;b&quot;\""),
            "{output}"
        );
        assert!(output.contains("fill=\"red\""), "{output}");
    }

    #[test]
    fn parse_declarations_is_quote_and_paren_aware() {
        assert_eq!(
            parse_declarations("fill:url(data:x;base64,AA==); stroke : red ;"),
            vec![
                ("fill".to_string(), "url(data:x;base64,AA==)".to_string()),
                ("stroke".to_string(), "red".to_string())
            ]
        );
        assert_eq!(
            parse_declarations("font-family:\"a;b\";fill:"),
            vec![("font-family".to_string(), "\"a;b\"".to_string())]
        );
    }

    #[test]
    fn brace_inside_quoted_value_does_not_end_the_block() {
        let (_, output) = run_pass(&svg(
            "<style>.a::before{content:\"}\"}.b{fill:red}</style><rect class=\"a b\"/>",
        ));
        assert!(output.contains(".a::before{content:\"}\"}"), "{output}");
        assert!(output.contains("fill=\"red\""), "{output}");
    }

    // --- helpers ---

    #[test]
    fn selector_specificity_is_exact() {
        let spec = selector_specificity;
        assert_eq!(spec("svg path"), Some((0, 0, 2)));
        assert_eq!(spec(".a:hover"), Some((0, 2, 0)));
        assert_eq!(spec("#x > .a::before"), Some((1, 1, 1)));
        assert_eq!(spec("#x > .a:before"), Some((1, 1, 1)));
        assert_eq!(spec("rect[class~=a]"), Some((0, 1, 1)));
        assert_eq!(spec("a[title=\"x,y]\"]"), Some((0, 1, 1)));
        assert_eq!(spec("li:nth-child(2n+1)"), Some((0, 1, 1)));
        assert_eq!(spec("li:nth-child(2n+1 of .b, #c)"), Some((1, 1, 1)));
        assert_eq!(spec("path:not([fill=blue])"), Some((0, 1, 1)));
        assert_eq!(spec(":not(.a, #b)"), Some((1, 0, 0)));
        assert_eq!(spec(":where(.x) rect"), Some((0, 0, 1)));
        assert_eq!(spec(":is(#a,.b) rect"), Some((1, 0, 1)));
        assert_eq!(spec(":is(.a, :not(#b .c))"), Some((1, 1, 0)));
        assert_eq!(spec("g :has(> rect.x)"), Some((0, 1, 2)));
        assert_eq!(spec("*"), Some((0, 0, 0)));
        assert_eq!(spec("* > rect"), Some((0, 0, 1)));
        assert_eq!(spec("svg|rect"), Some((0, 0, 1)));
        assert_eq!(spec("*|rect"), Some((0, 0, 1)));
        assert_eq!(spec(":lang(en)"), Some((0, 1, 0)));
        assert_eq!(spec(".a ~ .b + .c"), Some((0, 3, 0)));
        // Not understood: unknown functional pseudo-classes, nesting,
        // pseudo-elements with arguments, dangling escapes.
        assert_eq!(spec("path:foo(bar)"), None);
        assert_eq!(spec(":host(.x)"), None);
        assert_eq!(spec("::slotted(rect)"), None);
        assert_eq!(spec("& .a"), None);
        assert_eq!(spec(":not("), None);
        assert_eq!(spec("rect["), None);
    }

    #[test]
    fn split_selector_list_ignores_nested_commas() {
        assert_eq!(
            split_selector_list(":is(#a,.b) rect, .c, [t=\"x,y\"]"),
            vec![":is(#a,.b) rect", " .c", " [t=\"x,y\"]"]
        );
        assert_eq!(split_selector_list("rect"), vec!["rect"]);
    }

    // --- exact specificity in the cascade decision ---

    #[test]
    fn where_rule_ranks_below_later_type_rule() {
        // `:where(.x) rect` is (0,0,1), same as `rect`, and earlier: the
        // later rule wins (red). A presentation attribute would lose to the
        // kept rule, so the declaration goes to the style attribute.
        let (_, output) = run_pass(&svg(
            "<style>:where(.x) rect{fill:blue}rect{fill:red}</style><rect class=\"x\"/>",
        ));
        assert!(output.contains("style=\"fill:red\""), "{output}");
        assert!(!output.contains("fill=\"red\""), "{output}");
        assert!(
            output.contains("<style>:where(.x) rect{fill:blue}</style>"),
            "{output}"
        );
    }

    #[test]
    fn is_rule_ranks_as_its_most_specific_argument() {
        // `:is(#a,.b) rect` is (1,0,1) and outranks `#y` (1,0,0): the kept
        // rule must keep winning, so `#y` becomes a presentation attribute.
        let (_, output) = run_pass(&svg(
            "<style>:is(#a,.b) rect{fill:blue}#y{fill:red}</style><g id=\"a\"><rect id=\"y\"/></g>",
        ));
        assert!(output.contains("<rect id=\"y\" fill=\"red\"/>"), "{output}");
        assert!(!output.contains("style="), "{output}");
        assert!(
            output.contains("<style>:is(#a,.b) rect{fill:blue}</style>"),
            "{output}"
        );
    }

    #[test]
    fn script_kept_unmatched_rule_ranks_in_cascade() {
        // `.hl` matches nothing yet but a script may add the class; `#x`
        // outranks it, so after the click the element must stay blue: the
        // style attribute beats the kept `.hl` rule as `#x` did.
        let (_, output) = run_pass(&svg(
            "<style>#x{stroke:blue}.hl{stroke:red}</style><rect id=\"x\" onclick=\"this.classList.add('hl')\"/>",
        ));
        assert!(output.contains("style=\"stroke:blue\""), "{output}");
        assert!(!output.contains("stroke=\"blue\""), "{output}");
        assert!(
            output.contains("<style>.hl{stroke:red}</style>"),
            "{output}"
        );

        // Reversed ranking: `.hl` (0,1,0) beats `rect` (0,0,1) once added,
        // so the presentation attribute is the right target.
        let (_, output) = run_pass(&svg(
            "<style>rect{stroke:blue}.hl{stroke:red}</style><rect onclick=\"this.classList.add('hl')\"/>",
        ));
        assert!(output.contains("stroke=\"blue\""), "{output}");
        assert!(!output.contains("style="), "{output}");
    }

    #[test]
    fn selector_list_with_mixed_specificity_blocks() {
        // Which alternative matches the path is unknown: `svg path` ranks
        // below `.a`, `g :hover` above it. Neither target is safe.
        let input = svg(
            "<style>.a{fill:red}svg path, g :hover{fill:blue}</style><g><path class=\"a\"/></g>",
        );
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged, "{output}");
    }

    #[test]
    fn unknown_functional_pseudo_blocks_competing_declarations() {
        let input = svg("<style>.a{fill:red}path:foo(bar){fill:blue}</style><path class=\"a\"/>");
        let (result, output) = run_pass(&input);
        assert_eq!(result, PassResult::Unchanged, "{output}");

        // Other properties are unaffected.
        let (_, output) = run_pass(&svg(
            "<style>.a{fill:red;stroke:green}path:foo(bar){fill:blue}</style><path class=\"a\"/>",
        ));
        assert!(output.contains("stroke=\"green\""), "{output}");
        assert!(!output.contains("fill=\"red\""), "{output}");
        assert!(output.contains(".a{fill:red}"), "{output}");
    }

    #[test]
    fn comma_inside_pseudo_argument_is_one_selector_part() {
        let (_, output) = run_pass(&svg(
            "<style>:is(#a,.b) rect, .c{fill:blue}</style><g id=\"a\"><rect/></g><rect class=\"c\"/>",
        ));
        assert!(output.contains("<rect fill=\"blue\"/>"), "{output}");
        assert!(
            output.contains("<style>:is(#a,.b) rect{fill:blue}</style>"),
            "{output}"
        );
    }

    #[test]
    fn extract_class_names_handles_nesting_and_escapes() {
        assert_eq!(extract_class_names("g .a:hover > .b"), vec!["a", "b"]);
        assert_eq!(extract_class_names(":not(.x)"), vec!["x"]);
        assert_eq!(extract_class_names(r".a\:b"), vec!["a:b"]);
        assert_eq!(extract_class_names("rect[fill]"), Vec::<String>::new());
    }
}
