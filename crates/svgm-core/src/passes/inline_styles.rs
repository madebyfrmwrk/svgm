use super::{Pass, PassResult};
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

impl Pass for InlineStyles {
    fn name(&self) -> &'static str {
        "inlineStyles"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let ids = doc.traverse();

        // Step 1: Collect <style> elements and their CSS text.
        let mut style_node_ids: Vec<NodeId> = Vec::new();
        let mut css_text = String::new();

        for &nid in &ids {
            if let NodeKind::Element(ref elem) = doc.node(nid).kind
                && elem.name == "style"
                && elem.prefix.is_none()
            {
                // Skip non-CSS style types
                if let Some(t) = elem.attr("type")
                    && t != "text/css"
                {
                    continue;
                }
                style_node_ids.push(nid);

                // Collect text content (Text and CData children)
                for child_id in doc.children(nid) {
                    match &doc.node(child_id).kind {
                        NodeKind::Text(t) => css_text.push_str(t),
                        NodeKind::CData(t) => css_text.push_str(t),
                        _ => {}
                    }
                }
            }
        }

        if style_node_ids.is_empty() {
            return PassResult::Unchanged;
        }

        // Step 2: Parse CSS into rules.
        let parse_result = match parse_css(&css_text) {
            Some(r) => r,
            None => return PassResult::Unchanged, // Parse failure: preserve entirely
        };

        let CssParseResult {
            mut rules,
            has_at_rules,
            at_rule_blocks,
        } = parse_result;

        // Step 3: Classify selectors.
        for rule in &mut rules {
            rule.selector_kind = classify_selector(&rule.selector);
        }

        // Step 4: Build DOM usage maps.
        let mut class_map: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut id_map: HashMap<String, NodeId> = HashMap::new();
        let mut style_attr_props: HashMap<NodeId, HashSet<String>> = HashMap::new();

        for &nid in &ids {
            if let NodeKind::Element(ref elem) = doc.node(nid).kind {
                // Build class map
                if let Some(class_val) = elem.attr("class") {
                    for class_name in class_val.split_whitespace() {
                        class_map
                            .entry(class_name.to_string())
                            .or_default()
                            .push(nid);
                    }
                }
                // Build id map
                if let Some(id_val) = elem.attr("id") {
                    id_map.insert(id_val.to_string(), nid);
                }
                // Track style attribute properties
                if let Some(style_val) = elem.attr("style") {
                    let props: HashSet<String> = style_val
                        .split(';')
                        .filter_map(|decl| {
                            let parts: Vec<&str> = decl.splitn(2, ':').collect();
                            if parts.len() == 2 {
                                Some(parts[0].trim().to_string())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if !props.is_empty() {
                        style_attr_props.insert(nid, props);
                    }
                }
            }
        }

        // Step 5: Match selectors to elements.
        for rule in &mut rules {
            if rule.selector_kind == SelectorKind::Unsupported {
                continue;
            }
            rule.matched_elements = match_selector(
                &rule.selector,
                &rule.selector_kind,
                &class_map,
                &id_map,
                &ids,
                doc,
            );
        }

        // Step 6: Remove unused rules (supported selectors matching zero elements).
        let mut any_removed = false;
        for rule in &mut rules {
            if rule.selector_kind != SelectorKind::Unsupported && rule.matched_elements.is_empty() {
                rule.removed = true;
                any_removed = true;
            }
        }

        // Step 7: Inline safe rules.
        let mut any_inlined = false;
        if !has_at_rules {
            for rule in &mut rules {
                if rule.removed || rule.selector_kind == SelectorKind::Unsupported {
                    continue;
                }
                // Skip rules with !important
                if rule
                    .declarations
                    .iter()
                    .any(|(_, v)| v.contains("!important"))
                {
                    continue;
                }

                let mut all_inlined = true;
                for &nid in &rule.matched_elements {
                    let elem_style_props = style_attr_props.get(&nid);
                    for (prop, val) in &rule.declarations {
                        // Only inline presentation attributes
                        if !PRESENTATION_ATTRS.contains(&prop.as_str()) {
                            all_inlined = false;
                            continue;
                        }
                        // Skip if element's style="" already sets this property
                        if let Some(props) = elem_style_props
                            && props.contains(prop)
                        {
                            all_inlined = false;
                            continue;
                        }
                        // Set the presentation attribute on the element
                        let elem = match &mut doc.node_mut(nid).kind {
                            NodeKind::Element(e) => e,
                            _ => continue,
                        };
                        // Overwrite existing presentation attr (CSS beats pres attrs in cascade)
                        if let Some(attr) = elem
                            .attributes
                            .iter_mut()
                            .find(|a| a.name == *prop && a.prefix.is_none())
                        {
                            attr.value = val.clone();
                        } else {
                            elem.attributes.push(Attribute {
                                prefix: None,
                                name: prop.clone(),
                                value: val.clone(),
                            });
                        }
                        any_inlined = true;
                    }
                }

                if all_inlined {
                    rule.consumed = true;
                }
            }
        }

        // Step 8: Clean up class attributes.
        let mut any_class_cleaned = false;
        let mut still_needed_classes: HashSet<String> = HashSet::new();
        for rule in &rules {
            if rule.removed || rule.consumed {
                continue;
            }
            for class_name in extract_class_names_from_selector(&rule.selector) {
                still_needed_classes.insert(class_name);
            }
        }

        for &nid in &ids {
            if let NodeKind::Element(ref mut elem) = doc.node_mut(nid).kind
                && let Some(pos) = elem
                    .attributes
                    .iter()
                    .position(|a| a.name == "class" && a.prefix.is_none())
            {
                let old_val = elem.attributes[pos].value.clone();
                let new_classes: Vec<&str> = old_val
                    .split_whitespace()
                    .filter(|c| still_needed_classes.contains(*c))
                    .collect();

                if new_classes.is_empty() {
                    elem.attributes.remove(pos);
                    any_class_cleaned = true;
                } else {
                    let new_val = new_classes.join(" ");
                    if new_val != old_val {
                        elem.attributes[pos].value = new_val;
                        any_class_cleaned = true;
                    }
                }
            }
        }

        // Step 9: Write back remaining CSS or remove <style> elements.
        let remaining_rules: Vec<&CssRule> =
            rules.iter().filter(|r| !r.removed && !r.consumed).collect();

        let any_style_changed = if remaining_rules.is_empty() {
            // Remove all <style> elements
            for &sid in &style_node_ids {
                doc.remove(sid);
            }
            true
        } else {
            // Serialize remaining rules back into CSS text
            let new_css = serialize_rules(&remaining_rules, &at_rule_blocks);

            // Merge: keep only the first <style>, remove the rest
            let merged = style_node_ids.len() > 1;
            if merged {
                for &sid in &style_node_ids[1..] {
                    doc.remove(sid);
                }
            }

            // Update first <style> element's text content
            let first_style = style_node_ids[0];
            let children: Vec<NodeId> = doc.children(first_style).collect();
            for child in children {
                doc.remove(child);
            }
            let text_node = doc.alloc(NodeKind::Text(new_css));
            doc.append_child(first_style, text_node);
            // Changed if we merged multiple style elements or if any rules were removed/consumed
            merged || any_removed || any_inlined
        };

        if any_removed || any_inlined || any_class_cleaned || any_style_changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

// --- CSS Parser ---

struct CssParseResult {
    rules: Vec<CssRule>,
    has_at_rules: bool,
    /// Preserved @rule blocks as opaque text (not parsed, just kept for re-serialization).
    at_rule_blocks: Vec<String>,
}

struct CssRule {
    selector: String,
    declarations: Vec<(String, String)>,
    selector_kind: SelectorKind,
    matched_elements: Vec<NodeId>,
    removed: bool,
    consumed: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum SelectorKind {
    /// Not yet classified.
    #[default]
    Unknown,
    /// One of our supported simple selectors.
    Supported,
    /// Selector we don't understand — preserve, never inline, never remove.
    Unsupported,
}

/// Parse CSS text into a list of rules and detect @-rules.
fn parse_css(css: &str) -> Option<CssParseResult> {
    let mut rules = Vec::new();
    let mut has_at_rules = false;
    let mut at_rule_blocks = Vec::new();

    // Strip CSS comments first
    let stripped = strip_css_comments(css);
    let input = stripped.trim();

    let mut pos = 0;
    let bytes = input.as_bytes();

    while pos < bytes.len() {
        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // Detect @-rules — preserve the text
        if bytes[pos] == b'@' {
            has_at_rules = true;
            let start = pos;
            pos = skip_at_rule(input, pos)?;
            at_rule_blocks.push(input[start..pos].to_string());
            continue;
        }

        // Parse selector: everything up to '{'
        let open = find_char(input, pos, b'{')?;
        let selector = input[pos..open].trim().to_string();
        if selector.is_empty() {
            return None; // Parse failure
        }

        // Parse declaration block: everything between '{' and '}'
        let close = find_matching_brace(input, open)?;
        let block = input[open + 1..close].trim();

        let declarations = parse_declarations(block);

        rules.push(CssRule {
            selector,
            declarations,
            selector_kind: SelectorKind::Unknown,
            matched_elements: Vec::new(),
            removed: false,
            consumed: false,
        });

        pos = close + 1;
    }

    Some(CssParseResult {
        rules,
        has_at_rules,
        at_rule_blocks,
    })
}

/// Strip /* ... */ comments from CSS text.
fn strip_css_comments(css: &str) -> String {
    let mut result = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Find closing */
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2; // skip */
        } else {
            result.push(css[i..].chars().next().unwrap());
            i += css[i..].chars().next().unwrap().len_utf8();
        }
    }
    result
}

/// Find a character in the string starting at `from`.
fn find_char(s: &str, from: usize, ch: u8) -> Option<usize> {
    s.as_bytes()[from..]
        .iter()
        .position(|&b| b == ch)
        .map(|p| p + from)
}

/// Find the matching '}' for a '{' at position `open`, handling nesting.
fn find_matching_brace(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1;
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Skip an @-rule starting at position `pos`.
fn skip_at_rule(s: &str, pos: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = pos + 1;
    // Look for either ';' (simple @import) or '{' (block @media)
    while i < bytes.len() {
        match bytes[i] {
            b';' => return Some(i + 1),
            b'{' => {
                let close = find_matching_brace(s, i)?;
                return Some(close + 1);
            }
            _ => i += 1,
        }
    }
    Some(i) // End of input
}

/// Parse "property: value; property: value" into pairs.
fn parse_declarations(block: &str) -> Vec<(String, String)> {
    let mut decls = Vec::new();
    for part in block.split(';') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(colon) = trimmed.find(':') {
            let prop = trimmed[..colon].trim().to_string();
            let val = trimmed[colon + 1..].trim().to_string();
            if !prop.is_empty() && !val.is_empty() {
                decls.push((prop, val));
            }
        }
    }
    decls
}

// --- Selector classification ---

/// Classify whether a selector is in our supported subset.
fn classify_selector(selector: &str) -> SelectorKind {
    // Handle comma-separated selector lists
    for part in selector.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return SelectorKind::Unsupported;
        }
        if !is_simple_selector(trimmed) {
            return SelectorKind::Unsupported;
        }
    }
    SelectorKind::Supported
}

/// Check if a single selector (no commas) is simple enough for us.
fn is_simple_selector(sel: &str) -> bool {
    let sel = sel.trim();
    if sel.is_empty() {
        return false;
    }

    // Reject: contains whitespace (descendant combinator), >, +, ~
    if sel.contains(char::is_whitespace)
        || sel.contains('>')
        || sel.contains('+')
        || sel.contains('~')
    {
        return false;
    }

    // Reject: contains pseudo-class/element (: at any position)
    if sel.contains(':') {
        return false;
    }

    // Reject: contains attribute selector
    if sel.contains('[') {
        return false;
    }

    // Reject: universal selector alone
    if sel == "*" {
        return false;
    }

    // What's left: element names, .class, #id, or compounds like element.class
    // Validate: only contains [a-zA-Z0-9], '.', '#', '-', '_'
    sel.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '#' || c == '-' || c == '_')
}

// --- Selector matching ---

/// Match a selector to elements in the DOM.
fn match_selector(
    selector: &str,
    kind: &SelectorKind,
    class_map: &HashMap<String, Vec<NodeId>>,
    id_map: &HashMap<String, NodeId>,
    all_ids: &[NodeId],
    doc: &Document,
) -> Vec<NodeId> {
    if *kind == SelectorKind::Unsupported {
        return Vec::new();
    }

    // Handle comma-separated list: union of all matches
    let mut matched: Vec<NodeId> = Vec::new();
    let mut seen: HashSet<NodeId> = HashSet::new();

    for part in selector.split(',') {
        let trimmed = part.trim();
        for nid in match_single_selector(trimmed, class_map, id_map, all_ids, doc) {
            if seen.insert(nid) {
                matched.push(nid);
            }
        }
    }

    matched
}

/// Match a single simple selector (no commas).
fn match_single_selector(
    sel: &str,
    class_map: &HashMap<String, Vec<NodeId>>,
    id_map: &HashMap<String, NodeId>,
    all_ids: &[NodeId],
    doc: &Document,
) -> Vec<NodeId> {
    let sel = sel.trim();

    // Parse the selector into parts: optional element name, optional class, optional id
    let (elem_name, class_name, id_name) = parse_simple_selector(sel);

    // Start with the broadest match set, then narrow
    let candidates: Vec<NodeId> = if let Some(ref cls) = class_name {
        class_map.get(cls.as_str()).cloned().unwrap_or_default()
    } else if let Some(ref id) = id_name {
        id_map.get(id.as_str()).into_iter().copied().collect()
    } else if let Some(ref name) = elem_name {
        // Type selector: match all elements with that name
        all_ids
            .iter()
            .copied()
            .filter(|&nid| {
                if let NodeKind::Element(ref e) = doc.node(nid).kind {
                    e.name == *name && e.prefix.is_none()
                } else {
                    false
                }
            })
            .collect()
    } else {
        return Vec::new();
    };

    // Narrow by additional constraints
    candidates
        .into_iter()
        .filter(|&nid| {
            if let NodeKind::Element(ref e) = doc.node(nid).kind {
                if let Some(ref name) = elem_name
                    && e.name != *name
                {
                    return false;
                }
                if let Some(ref cls) = class_name {
                    match e.attr("class") {
                        Some(val) => {
                            if !val.split_whitespace().any(|c| c == cls) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                if let Some(ref id) = id_name
                    && e.attr("id") != Some(id)
                {
                    return false;
                }
                true
            } else {
                false
            }
        })
        .collect()
}

/// Parse a simple selector like "rect.cls" or ".cls" or "#id" or "rect"
/// into (optional_element, optional_class, optional_id).
fn parse_simple_selector(sel: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut elem_name = None;
    let mut class_name = None;
    let mut id_name = None;

    let mut current = sel;

    // Extract element name (starts with a letter, before . or #)
    if !current.is_empty() && !current.starts_with('.') && !current.starts_with('#') {
        let end = current.find(['.', '#']).unwrap_or(current.len());
        elem_name = Some(current[..end].to_string());
        current = &current[end..];
    }

    // Extract class name
    if let Some(rest) = current.strip_prefix('.') {
        let end = rest.find(['.', '#']).unwrap_or(rest.len());
        class_name = Some(rest[..end].to_string());
        current = &rest[end..];
    }

    // Extract id name
    if let Some(rest) = current.strip_prefix('#') {
        id_name = Some(rest.to_string());
    }

    (elem_name, class_name, id_name)
}

/// Extract class names referenced in a selector.
fn extract_class_names_from_selector(selector: &str) -> Vec<String> {
    let mut classes = Vec::new();
    for part in selector.split(',') {
        let trimmed = part.trim();
        let bytes = trimmed.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'.' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric()
                        || bytes[end] == b'-'
                        || bytes[end] == b'_')
                {
                    end += 1;
                }
                if end > start {
                    classes.push(trimmed[start..end].to_string());
                }
                i = end;
            } else {
                i += 1;
            }
        }
    }
    classes
}

/// Serialize remaining rules and preserved @rule blocks back to CSS text.
fn serialize_rules(rules: &[&CssRule], at_rule_blocks: &[String]) -> String {
    let mut out = String::new();
    // Emit @rule blocks first (they often contain fallbacks that should appear before overrides)
    for block in at_rule_blocks {
        out.push_str(block);
    }
    for rule in rules {
        out.push_str(&rule.selector);
        out.push('{');
        for (i, (prop, val)) in rule.declarations.iter().enumerate() {
            out.push_str(prop);
            out.push(':');
            out.push_str(val);
            if i + 1 < rule.declarations.len() {
                out.push(';');
            }
        }
        out.push('}');
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
        // Both rules inlined, style elements removed
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
    fn class_attribute_cleaned_after_inlining() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{fill:red}</style><rect class=\"a\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("class="));
    }

    #[test]
    fn partial_inline_multiple_classes() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a{fill:red}.b:hover{opacity:.5}</style><rect class=\"a b\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("fill=\"red\""));
        // .b still needed for :hover rule
        assert!(output.contains("class=\"b\""));
        // :hover rule preserved
        assert!(output.contains(".b:hover"));
    }

    #[test]
    fn css_overrides_presentation_attribute() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red}</style><rect class=\"cls\" fill=\"blue\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // CSS beats presentation attr
        assert!(output.contains("fill=\"red\""));
        assert!(!output.contains("fill=\"blue\""));
    }

    #[test]
    fn skip_inline_when_style_attr_conflicts() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls{fill:red;stroke:blue}</style><rect class=\"cls\" style=\"fill:green\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // fill: skip (style attr wins), stroke: inline
        assert!(output.contains("stroke=\"blue\""));
        // style attr preserved
        assert!(output.contains("style=\"fill:green\""));
        // Rule not fully consumed — .cls still in stylesheet for fill
        // Actually the rule has fill and stroke. fill was skipped, stroke was inlined.
        // So the rule is NOT fully consumed. .cls remains needed.
    }

    #[test]
    fn at_media_triggers_bail_out() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>@media(prefers-color-scheme:dark){.bg{fill:#000}}.bg{fill:#fff}.unused{fill:red}</style><rect class=\"bg\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // Inlining bailed out, but unused rules still removed
        assert!(output.contains("<style>"));
        assert!(output.contains(".bg{fill:#fff}"));
        assert!(!output.contains(".unused"));
    }

    #[test]
    fn pseudo_selector_preserved() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a:hover{opacity:.5}</style><rect class=\"a\" width=\"10\" height=\"10\"/></svg>";
        let (result, _output) = run_pass(input);
        // :hover can't be inlined, and selector is unsupported so not removed
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
        // fill inlined, custom-thing stays in stylesheet
        assert!(output.contains("fill=\"red\""));
        // Rule not fully consumed because custom-thing wasn't inlined
        assert!(output.contains("custom-thing"));
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
    fn selector_list_inlined() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a,.b{fill:red}</style><rect class=\"a\" width=\"10\" height=\"10\"/><rect class=\"b\" width=\"10\" height=\"10\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("<style>"));
        // Both elements get fill="red"
        let count = output.matches("fill=\"red\"").count();
        assert_eq!(count, 2);
    }

    #[test]
    fn element_type_selector() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>rect{fill:red}</style><rect width=\"10\" height=\"10\"/><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("fill=\"red\""));
        // Only rect gets it, not circle
        assert!(!output.contains("<style>"));
    }

    #[test]
    fn element_class_compound_selector() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>rect.special{fill:red}</style><rect class=\"special\" width=\"10\" height=\"10\"/><circle class=\"special\" cx=\"5\" cy=\"5\" r=\"5\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // Only rect.special matches, not circle.special
        let count = output.matches("fill=\"red\"").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn convergence_test() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.cls-1{fill:red;opacity:1}.cls-2{stroke:blue;stroke-width:2}</style><rect class=\"cls-1\" width=\"100\" height=\"100\"/><circle class=\"cls-2\" cx=\"50\" cy=\"50\" r=\"30\"/></svg>";
        let result1 = crate::optimize(input).unwrap();
        let result2 = crate::optimize(&result1.data).unwrap();
        assert_eq!(result1.data, result2.data, "should converge");
    }
}
