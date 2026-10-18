//! Shared helpers used by several passes: attribute/element tables, tree
//! queries, script/stylesheet detection, inline-style splitting and a small
//! computed-style resolver.
//!
//! Everything here is `pub(crate)`; passes import it via `super::*` glob
//! re-export in `passes/mod.rs`.
//!
//! Several helpers are consumed only by later work packages (WP1-WP6); the
//! module-level `dead_code` allowance below should be dropped once they are
//! all in use.
#![allow(dead_code)]

use std::collections::HashSet;

use crate::ast::{Document, Element, NodeId, NodeKind};

// ---------------------------------------------------------------------------
// Attribute / element tables
// ---------------------------------------------------------------------------

/// Attributes that may contain `url(#id)` references. Moving or rewriting
/// coordinate spaces of an element carrying one of these can break the
/// referenced resource (gradients, clip paths, masks, markers ...).
pub(crate) const URL_REF_ATTRS: &[&str] = &[
    "clip-path",
    "fill",
    "filter",
    "marker-end",
    "marker-mid",
    "marker-start",
    "mask",
    "stroke",
];

/// Elements that can safely receive a `transform` attribute.
pub(crate) const MOVEABLE_ELEMENTS: &[&str] = &[
    "circle", "ellipse", "line", "path", "polygon", "polyline", "rect", "g", "text", "use",
    "image", "svg",
];

/// Inheritable presentation attributes (matches SVGO's `inheritableAttrs`
/// minus `presentationNonInheritableGroupAttrs`). These inherit from parent
/// to child in the SVG cascade.
pub(crate) const INHERITABLE_PROPS: &[&str] = &[
    "clip-rule",
    "color",
    "color-interpolation",
    "color-interpolation-filters",
    "color-profile",
    "cursor",
    "direction",
    "dominant-baseline",
    "fill",
    "fill-opacity",
    "fill-rule",
    "font",
    "font-family",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-variant",
    "font-weight",
    "image-rendering",
    "letter-spacing",
    "marker",
    "marker-end",
    "marker-mid",
    "marker-start",
    "paint-order",
    "pointer-events",
    "shape-rendering",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "text-anchor",
    "text-rendering",
    "visibility",
    "word-spacing",
    "writing-mode",
];

/// Event-handler attributes, the union of SVGO's `attrsGroups.animationEvent`,
/// `documentEvent`, `documentElementEvent`, `globalEvent` and
/// `graphicalEvent` (deduplicated, sorted). Any of these on an element means
/// a script may run.
pub(crate) const EVENT_ATTRS: &[&str] = &[
    "onabort",
    "onactivate",
    "onbegin",
    "oncancel",
    "oncanplay",
    "oncanplaythrough",
    "onchange",
    "onclick",
    "onclose",
    "oncopy",
    "oncuechange",
    "oncut",
    "ondblclick",
    "ondrag",
    "ondragend",
    "ondragenter",
    "ondragleave",
    "ondragover",
    "ondragstart",
    "ondrop",
    "ondurationchange",
    "onemptied",
    "onend",
    "onended",
    "onerror",
    "onfocus",
    "onfocusin",
    "onfocusout",
    "oninput",
    "oninvalid",
    "onkeydown",
    "onkeypress",
    "onkeyup",
    "onload",
    "onloadeddata",
    "onloadedmetadata",
    "onloadstart",
    "onmousedown",
    "onmouseenter",
    "onmouseleave",
    "onmousemove",
    "onmouseout",
    "onmouseover",
    "onmouseup",
    "onmousewheel",
    "onpaste",
    "onpause",
    "onplay",
    "onplaying",
    "onprogress",
    "onratechange",
    "onrepeat",
    "onreset",
    "onresize",
    "onscroll",
    "onseeked",
    "onseeking",
    "onselect",
    "onshow",
    "onstalled",
    "onsubmit",
    "onsuspend",
    "ontimeupdate",
    "ontoggle",
    "onunload",
    "onvolumechange",
    "onwaiting",
    "onzoom",
];

/// Animation elements whose `attributeName` can retarget a property.
const ANIMATION_ELEMS: &[&str] = &[
    "animate",
    "animateColor",
    "animateMotion",
    "animateTransform",
    "set",
];

/// Animation elements considered by [`animation_targets`]. Kept identical to
/// the list `removeHiddenElems` has always used.
const ANIMATION_TARGET_ELEMS: &[&str] = &["animate", "animateTransform", "animateMotion", "set"];

// ---------------------------------------------------------------------------
// Tree queries
// ---------------------------------------------------------------------------

fn elem(doc: &Document, id: NodeId) -> Option<&Element> {
    match &doc.node(id).kind {
        NodeKind::Element(e) => Some(e),
        _ => None,
    }
}

/// True when the direct parent of `id` is an unprefixed element named `name`.
pub(crate) fn parent_is(doc: &Document, id: NodeId, name: &str) -> bool {
    doc.node(id)
        .parent
        .and_then(|p| elem(doc, p))
        .is_some_and(|e| e.prefix.is_none() && e.name == name)
}

/// True when any proper ancestor of `id` is an unprefixed element whose name
/// is in `names`.
pub(crate) fn has_ancestor(doc: &Document, id: NodeId, names: &[&str]) -> bool {
    let mut current = doc.node(id).parent;
    while let Some(pid) = current {
        if let Some(e) = elem(doc, pid)
            && e.prefix.is_none()
            && names.contains(&e.name.as_str())
        {
            return true;
        }
        current = doc.node(pid).parent;
    }
    false
}

/// True when `id` is inside a `<foreignObject>`.
pub(crate) fn inside_foreign_object(doc: &Document, id: NodeId) -> bool {
    has_ancestor(doc, id, &["foreignObject"])
}

/// Set of elements targeted by animation elements, either explicitly via
/// `href`/`xlink:href="#id"` or implicitly (the animation's parent).
pub(crate) fn animation_targets(doc: &Document) -> HashSet<NodeId> {
    let ids = doc.traverse();

    let mut id_to_node: std::collections::HashMap<&str, NodeId> = std::collections::HashMap::new();
    for &nid in &ids {
        if let NodeKind::Element(ref e) = doc.node(nid).kind
            && let Some(id_val) = e.attr("id")
        {
            id_to_node.insert(id_val, nid);
        }
    }

    let mut targets = HashSet::new();
    for &nid in &ids {
        if let NodeKind::Element(ref e) = doc.node(nid).kind
            && ANIMATION_TARGET_ELEMS.contains(&e.name.as_str())
        {
            let href_target = e
                .attr("href")
                .or_else(|| {
                    e.attributes
                        .iter()
                        .find(|a| a.name == "href" && a.prefix.as_deref() == Some("xlink"))
                        .map(|a| a.value.as_str())
                })
                .and_then(|v| v.strip_prefix('#'))
                .and_then(|id| id_to_node.get(id).copied());

            if let Some(target) = href_target {
                targets.insert(target);
            } else if let Some(parent) = doc.node(nid).parent {
                targets.insert(parent);
            }
        }
    }
    targets
}

/// True when a direct child of `id` is an animation element (`animate`,
/// `animateColor`, `animateMotion`, `animateTransform`, `set`) whose
/// `attributeName` equals `attr_name`.
pub(crate) fn has_animated_attr(doc: &Document, id: NodeId, attr_name: &str) -> bool {
    doc.children(id).any(|c| {
        elem(doc, c).is_some_and(|e| {
            e.prefix.is_none()
                && ANIMATION_ELEMS.contains(&e.name.as_str())
                && e.attr("attributeName") == Some(attr_name)
        })
    })
}

// ---------------------------------------------------------------------------
// Script / stylesheet detection
// ---------------------------------------------------------------------------

/// True when the document may run script: a `<script>` element, an `<a>`
/// whose `href`/`xlink:href` is a `javascript:` URL, or any element with an
/// event-handler attribute ([`EVENT_ATTRS`]).
pub(crate) fn has_scripts(doc: &Document) -> bool {
    for id in doc.traverse() {
        let Some(e) = elem(doc, id) else { continue };
        if e.prefix.is_none() && e.name == "script" {
            return true;
        }
        if e.prefix.is_none()
            && e.name == "a"
            && e.attributes.iter().any(|a| {
                a.name == "href"
                    && (a.prefix.is_none() || a.prefix.as_deref() == Some("xlink"))
                    && a.value
                        .trim()
                        .to_ascii_lowercase()
                        .starts_with("javascript:")
            })
        {
            return true;
        }
        if e.attributes
            .iter()
            .any(|a| a.prefix.is_none() && EVENT_ATTRS.contains(&a.name.as_str()))
        {
            return true;
        }
    }
    false
}

/// Concatenated text of every unprefixed `<style>` element's text/CDATA
/// children.
fn stylesheet_text(doc: &Document) -> String {
    let mut out = String::new();
    for id in doc.traverse() {
        let Some(e) = elem(doc, id) else { continue };
        if e.prefix.is_some() || e.name != "style" {
            continue;
        }
        for c in doc.children(id) {
            if let NodeKind::Text(t) | NodeKind::CData(t) = &doc.node(c).kind {
                out.push_str(t);
                out.push('\n');
            }
        }
    }
    out
}

/// True when any unprefixed `<style>` element has non-whitespace content.
pub(crate) fn has_stylesheet(doc: &Document) -> bool {
    for id in doc.traverse() {
        let Some(e) = elem(doc, id) else { continue };
        if e.prefix.is_some() || e.name != "style" {
            continue;
        }
        for c in doc.children(id) {
            if let NodeKind::Text(t) | NodeKind::CData(t) = &doc.node(c).kind
                && !t.trim().is_empty()
            {
                return true;
            }
        }
    }
    false
}

/// True when a stylesheet may affect structure-sensitive optimizations.
/// Currently an alias of [`has_stylesheet`]; a later work package (WP6) may
/// refine it to ignore stylesheets whose rules are all inlined/safe.
pub(crate) fn has_structural_css(doc: &Document) -> bool {
    has_stylesheet(doc)
}

/// Remove `/* ... */` comments from CSS text.
fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Decode CSS backslash escapes in an identifier (`xml\:space` ->
/// `xml:space`, `\3A ` -> `:`).
fn unescape_css_ident(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        // Hex escape: up to six hex digits, optionally followed by one space.
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(|h| h.is_ascii_hexdigit()) {
            hex.push(chars.next().unwrap());
        }
        if !hex.is_empty() {
            if chars.peek().is_some_and(|w| w.is_whitespace()) {
                chars.next();
            }
            if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                out.push(ch);
            }
        } else if let Some(next) = chars.next() {
            out.push(next);
        }
    }
    out
}

/// Every `property: value` declaration found in the innermost `{}` blocks of
/// all `<style>` elements, regardless of selector and nesting depth (rules
/// inside `@media`/`@supports` are included, so a consumer can ask "could any
/// stylesheet rule set `marker-mid` on some element?" without resolving
/// selectors). Values keep their text verbatim (trailing `!important`
/// included). Comments are stripped first; string literals are skipped so a
/// brace inside `content:"}"` does not end a block.
pub(crate) fn stylesheet_declarations(doc: &Document) -> Vec<(String, String)> {
    let css = strip_css_comments(&stylesheet_text(doc));
    let mut out = Vec::new();
    let chars: Vec<char> = css.chars().collect();
    // Start index (exclusive of the brace) of each currently open block, and
    // whether a nested block has been seen inside it.
    let mut stack: Vec<(usize, bool)> = Vec::new();
    let mut quote: Option<char> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' => {
                if let Some(top) = stack.last_mut() {
                    top.1 = true;
                }
                stack.push((i + 1, false));
            }
            '}' => {
                if let Some((start, nested)) = stack.pop()
                    && !nested
                {
                    let block: String = chars[start..i].iter().collect();
                    for decl in split_declarations(&block) {
                        if let Some((name, value)) = split_property(decl) {
                            out.push((name.to_string(), value.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Attribute names used in attribute selectors (`[fill=black]`,
/// `text[xml\:space]`) across all `<style>` elements. Only `[` in selector
/// position counts: outside `{}` blocks and outside string literals.
pub(crate) fn attributes_in_stylesheets(doc: &Document) -> HashSet<String> {
    let css = strip_css_comments(&stylesheet_text(doc));
    let mut out = HashSet::new();
    let chars: Vec<char> = css.chars().collect();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '[' if depth == 0 => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                let start = j;
                while j < chars.len() {
                    let d = chars[j];
                    if d == '\\' {
                        // Skip the escape: up to six hex digits plus one
                        // optional whitespace, or a single escaped char.
                        j += 1;
                        let hex_start = j;
                        while j < chars.len() && j - hex_start < 6 && chars[j].is_ascii_hexdigit() {
                            j += 1;
                        }
                        if j == hex_start || (j < chars.len() && chars[j].is_whitespace()) {
                            j += 1;
                        }
                        continue;
                    }
                    if matches!(d, '=' | '~' | '|' | '^' | '$' | '*' | ']') || d.is_whitespace() {
                        break;
                    }
                    j += 1;
                }
                let j = j.min(chars.len());
                let raw: String = chars[start..j].iter().collect();
                let name = unescape_css_ident(&raw);
                if !name.is_empty() {
                    out.insert(name);
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Inline style splitting
// ---------------------------------------------------------------------------

/// Split a `style` attribute value into declarations on `;`, honouring
/// parentheses (`url(data:...;base64,...)`) and quoted strings (`"a; b"`,
/// with backslash escapes). Pieces are trimmed; empty pieces are dropped.
pub(crate) fn split_declarations(style: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut start = 0;
    let bytes = style.as_bytes();
    let mut i = 0;
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
                b'(' => depth += 1,
                b')' => depth = (depth - 1).max(0),
                b';' if depth == 0 => {
                    let piece = style[start..i].trim();
                    if !piece.is_empty() {
                        out.push(piece);
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    let piece = style[start.min(style.len())..].trim();
    if !piece.is_empty() {
        out.push(piece);
    }
    out
}

/// Split a declaration at the first `:` outside quotes/parentheses into
/// `(name, value)`, both trimmed. `None` when there is no such `:` or the
/// name is empty.
pub(crate) fn split_property(decl: &str) -> Option<(&str, &str)> {
    let bytes = decl.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
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
                b'(' => depth += 1,
                b')' => depth = (depth - 1).max(0),
                b':' if depth == 0 => {
                    let name = decl[..i].trim();
                    if name.is_empty() {
                        return None;
                    }
                    return Some((name, decl[i + 1..].trim()));
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Strip a trailing `!important` (any case, any inner whitespace) and trim.
fn strip_important(value: &str) -> &str {
    let v = value.trim();
    const KW: &str = "important";
    if v.len() >= KW.len() && v[v.len() - KW.len()..].eq_ignore_ascii_case(KW) {
        let before = v[..v.len() - KW.len()].trim_end();
        if let Some(rest) = before.strip_suffix('!') {
            return rest.trim();
        }
    }
    v
}

/// Value of `prop` in the element's unprefixed `style` attribute (last
/// occurrence wins), with a trailing `!important` stripped.
pub(crate) fn style_declaration(elem: &Element, prop: &str) -> Option<String> {
    let style = elem.attr("style")?;
    let mut found = None;
    for decl in split_declarations(style) {
        if let Some((name, value)) = split_property(decl)
            && name == prop
        {
            found = Some(strip_important(value).to_string());
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Computed style
// ---------------------------------------------------------------------------

/// Result of resolving a presentation property on an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Computed {
    /// The property resolves to this value.
    Static(String),
    /// A stylesheet or script could override anything; treat as unknown.
    Dynamic,
    /// Nothing sets the property.
    Unset,
}

/// Resolve `prop` on element `id`: the `style` attribute declaration beats
/// the presentation attribute; `inherit` continues to the parent; an
/// inheritable property ([`INHERITABLE_PROPS`]) walks the ancestors (nearest
/// wins). Returns [`Computed::Dynamic`] when the document has a stylesheet or
/// script.
pub(crate) fn computed_style(doc: &Document, id: NodeId, prop: &str) -> Computed {
    let dynamic = has_stylesheet(doc) || has_scripts(doc);
    computed_style_with(doc, id, prop, dynamic)
}

/// [`computed_style`] with a precomputed `dynamic` flag
/// (`has_stylesheet(doc) || has_scripts(doc)`) so loops do not re-scan the
/// document per element.
pub(crate) fn computed_style_with(
    doc: &Document,
    id: NodeId,
    prop: &str,
    dynamic: bool,
) -> Computed {
    if dynamic {
        return Computed::Dynamic;
    }
    let inheritable = INHERITABLE_PROPS.contains(&prop);
    let mut current = Some(id);
    let mut first = true;
    while let Some(nid) = current {
        if let Some(e) = elem(doc, nid) {
            let own = style_declaration(e, prop).or_else(|| e.attr(prop).map(str::to_string));
            match own {
                Some(v) if v.trim() == "inherit" => {}
                Some(v) => return Computed::Static(v),
                None if !first && !inheritable => return Computed::Unset,
                None => {}
            }
        }
        first = false;
        current = doc.node(nid).parent;
    }
    Computed::Unset
}

// ---------------------------------------------------------------------------
// CSS syntax
// ---------------------------------------------------------------------------

/// True when `c` can appear in a CSS identifier: alphanumerics, `-`, `_`,
/// and any non-ASCII code point (CSS Syntax Level 3).
pub(crate) fn is_css_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn find(doc: &Document, name: &str) -> NodeId {
        doc.traverse()
            .into_iter()
            .find(|&id| elem(doc, id).is_some_and(|e| e.name == name))
            .unwrap_or_else(|| panic!("no <{name}>"))
    }

    #[test]
    fn event_attrs_sorted_and_unique() {
        let mut sorted = EVENT_ATTRS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, EVENT_ATTRS);
        for a in ["onbegin", "onzoom", "oncopy", "onwaiting", "onactivate"] {
            assert!(EVENT_ATTRS.contains(&a), "{a}");
        }
    }

    #[test]
    fn parent_is_checks_direct_parent_only() {
        let doc = parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><g><path/></g></svg>"#).unwrap();
        let path = find(&doc, "path");
        assert!(parent_is(&doc, path, "g"));
        assert!(!parent_is(&doc, path, "svg"));
        assert!(!parent_is(&doc, doc.root, "svg"));
    }

    #[test]
    fn has_ancestor_walks_up_and_ignores_prefixed() {
        let doc = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:x="urn:x"><defs><g><path/></g></defs><x:defs><rect/></x:defs></svg>"#,
        )
        .unwrap();
        let path = find(&doc, "path");
        assert!(has_ancestor(&doc, path, &["defs", "symbol"]));
        assert!(has_ancestor(&doc, path, &["svg"]));
        assert!(!has_ancestor(&doc, path, &["symbol"]));
        // The element itself does not count.
        assert!(!has_ancestor(&doc, path, &["path"]));
        let rect = find(&doc, "rect");
        assert!(!has_ancestor(&doc, rect, &["defs"]));
    }

    #[test]
    fn inside_foreign_object_detects_ancestor() {
        let doc = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject><div><p/></div></foreignObject><rect/></svg>"#,
        )
        .unwrap();
        assert!(inside_foreign_object(&doc, find(&doc, "p")));
        assert!(!inside_foreign_object(&doc, find(&doc, "rect")));
    }

    #[test]
    fn animation_targets_href_and_parent() {
        let doc = parse(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><rect id="r"/><circle><animate attributeName="r"/></circle><animate xlink:href="#r" attributeName="x"/><path/></svg>"##,
        )
        .unwrap();
        let targets = animation_targets(&doc);
        assert!(targets.contains(&find(&doc, "rect")));
        assert!(targets.contains(&find(&doc, "circle")));
        assert!(!targets.contains(&find(&doc, "path")));
    }

    #[test]
    fn has_animated_attr_direct_children_only() {
        let doc = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g><animateTransform attributeName="transform"/><rect><set attributeName="fill"/></rect></g></svg>"#,
        )
        .unwrap();
        let g = find(&doc, "g");
        assert!(has_animated_attr(&doc, g, "transform"));
        assert!(!has_animated_attr(&doc, g, "fill"));
        assert!(has_animated_attr(&doc, find(&doc, "rect"), "fill"));
    }

    #[test]
    fn has_scripts_detects_script_links_and_handlers() {
        let no = parse(r##"<svg xmlns="http://www.w3.org/2000/svg"><a href="#x"/><rect/></svg>"##)
            .unwrap();
        assert!(!has_scripts(&no));

        let script = parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>"#).unwrap();
        assert!(has_scripts(&script));

        let js_link = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><a href="  JavaScript:alert(1)"/></svg>"#,
        )
        .unwrap();
        assert!(has_scripts(&js_link));

        let xlink = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><a xlink:href="javascript:x()"/></svg>"#,
        )
        .unwrap();
        assert!(has_scripts(&xlink));

        let handler =
            parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect onclick="f()"/></svg>"#)
                .unwrap();
        assert!(has_scripts(&handler));

        let anim =
            parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><animate onbegin="f()"/></svg>"#)
                .unwrap();
        assert!(has_scripts(&anim));
    }

    #[test]
    fn has_stylesheet_requires_non_whitespace_content() {
        let empty =
            parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><style>  </style></svg>"#).unwrap();
        assert!(!has_stylesheet(&empty));
        assert!(!has_structural_css(&empty));

        let css =
            parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><style>.a{fill:red}</style></svg>"#)
                .unwrap();
        assert!(has_stylesheet(&css));
        assert!(has_structural_css(&css));

        let cdata = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><style><![CDATA[.a{fill:red}]]></style></svg>"#,
        )
        .unwrap();
        assert!(has_stylesheet(&cdata));
    }

    #[test]
    fn split_declarations_respects_parens_and_quotes() {
        assert_eq!(
            split_declarations("fill:red; stroke : blue ;;"),
            vec!["fill:red", "stroke : blue"]
        );
        assert_eq!(
            split_declarations("fill:url(data:image/svg+xml;base64,PHN2Zy8+);stroke:red"),
            vec!["fill:url(data:image/svg+xml;base64,PHN2Zy8+)", "stroke:red"]
        );
        assert_eq!(
            split_declarations(r#"font-family:"a; b";fill:red"#),
            vec![r#"font-family:"a; b""#, "fill:red"]
        );
        assert_eq!(
            split_declarations(r#"content:'a\'; b';fill:red"#),
            vec![r#"content:'a\'; b'"#, "fill:red"]
        );
        assert_eq!(split_declarations(""), Vec::<&str>::new());
        assert_eq!(split_declarations(" ; ; "), Vec::<&str>::new());
        // Unbalanced close parens do not swallow the rest.
        assert_eq!(split_declarations("a:b);c:d"), vec!["a:b)", "c:d"]);
    }

    #[test]
    fn split_property_finds_first_top_level_colon() {
        assert_eq!(split_property(" fill : red "), Some(("fill", "red")));
        assert_eq!(
            split_property("fill:url(data:image/png;base64,AAA)"),
            Some(("fill", "url(data:image/png;base64,AAA)"))
        );
        assert_eq!(
            split_property(r#"content:"a:b""#),
            Some(("content", r#""a:b""#))
        );
        assert_eq!(split_property("fill:"), Some(("fill", "")));
        assert_eq!(split_property("fill"), None);
        assert_eq!(split_property(":red"), None);
        assert_eq!(split_property(r#""a:b""#), None);
    }

    #[test]
    fn style_declaration_last_wins_and_strips_important() {
        let doc = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect style="fill:red;stroke:blue;fill: green !important ;"/></svg>"#,
        )
        .unwrap();
        let e = elem(&doc, find(&doc, "rect")).unwrap();
        assert_eq!(style_declaration(e, "fill"), Some("green".to_string()));
        assert_eq!(style_declaration(e, "stroke"), Some("blue".to_string()));
        assert_eq!(style_declaration(e, "opacity"), None);

        let doc = parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#).unwrap();
        let e = elem(&doc, find(&doc, "rect")).unwrap();
        assert_eq!(style_declaration(e, "fill"), None);
    }

    #[test]
    fn computed_style_resolution() {
        let doc = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg" fill="red" opacity=".5"><g fill="blue" style="fill:green"><rect/><circle fill="inherit"/><path opacity="inherit"/></g><g opacity=".7"><ellipse opacity="inherit"/></g></svg>"#,
        )
        .unwrap();
        let rect = find(&doc, "rect");
        // style attr beats presentation attr on the group; inherited by rect.
        assert_eq!(
            computed_style(&doc, rect, "fill"),
            Computed::Static("green".into())
        );
        // opacity is not inheritable: only the element itself is consulted.
        assert_eq!(computed_style(&doc, rect, "opacity"), Computed::Unset);
        assert_eq!(computed_style(&doc, rect, "stroke"), Computed::Unset);
        // "inherit" continues to the parent.
        assert_eq!(
            computed_style(&doc, find(&doc, "circle"), "fill"),
            Computed::Static("green".into())
        );
        // Explicit inherit on a non-inheritable prop takes the parent's own
        // value only: the first <g> has none, so the root's does not apply.
        assert_eq!(
            computed_style(&doc, find(&doc, "path"), "opacity"),
            Computed::Unset
        );
        assert_eq!(
            computed_style(&doc, find(&doc, "ellipse"), "opacity"),
            Computed::Static(".7".into())
        );
        // Own value wins over ancestors.
        assert_eq!(
            computed_style(&doc, find(&doc, "g"), "fill"),
            Computed::Static("green".into())
        );
        assert_eq!(
            computed_style(&doc, find(&doc, "svg"), "fill"),
            Computed::Static("red".into())
        );
    }

    #[test]
    fn computed_style_dynamic_with_stylesheet_or_script() {
        let css = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><style>.a{}</style><rect fill="red"/></svg>"#,
        )
        .unwrap();
        assert_eq!(
            computed_style(&css, find(&css, "rect"), "fill"),
            Computed::Dynamic
        );

        let js = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="red" onclick="x()"/></svg>"#,
        )
        .unwrap();
        let rect = find(&js, "rect");
        assert_eq!(computed_style(&js, rect, "fill"), Computed::Dynamic);
        assert_eq!(
            computed_style_with(&js, rect, "fill", false),
            Computed::Static("red".into())
        );
        assert_eq!(
            computed_style_with(&js, rect, "fill", true),
            Computed::Dynamic
        );
    }

    #[test]
    fn attributes_in_stylesheets_reads_selector_attrs() {
        let attrs = |css: &str| {
            let svg =
                format!(r#"<svg xmlns="http://www.w3.org/2000/svg"><style>{css}</style></svg>"#);
            attributes_in_stylesheets(&parse(&svg).unwrap())
        };
        let set =
            |items: &[&str]| -> HashSet<String> { items.iter().map(|s| s.to_string()).collect() };
        assert_eq!(attrs("[fill=black]{}"), set(&["fill"]));
        assert_eq!(attrs(r"text[xml\:space]{}"), set(&["xml:space"]));
        assert_eq!(attrs("a{content:'[x]'}"), set(&[]));
        assert_eq!(attrs("/* [x] */ a{}"), set(&[]));
        assert_eq!(
            attrs("a[ href ~= 'x' ], b[data-foo]{} c[stroke]"),
            set(&["href", "data-foo", "stroke"])
        );
        assert_eq!(attrs(r"a[xml\3A space]{}"), set(&["xml:space"]));
        assert_eq!(attrs("a[fill"), set(&["fill"]));
    }

    #[test]
    fn attributes_in_stylesheets_merges_style_elements_and_cdata() {
        let doc = parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><style>[a]{}</style><style><![CDATA[[b]{}]]></style></svg>"#,
        )
        .unwrap();
        let got = attributes_in_stylesheets(&doc);
        assert!(got.contains("a") && got.contains("b") && got.len() == 2);
    }

    #[test]
    fn css_ident_chars() {
        for c in ['a', 'Z', '0', '-', '_', 'é', '中'] {
            assert!(is_css_ident_char(c), "{c}");
        }
        for c in [' ', '.', '#', '[', ':', '>', '{'] {
            assert!(!is_css_ident_char(c), "{c}");
        }
    }

    #[test]
    fn constants_match_expected_shape() {
        assert!(URL_REF_ATTRS.contains(&"clip-path"));
        assert!(MOVEABLE_ELEMENTS.contains(&"path"));
        assert!(INHERITABLE_PROPS.contains(&"fill"));
        assert!(!INHERITABLE_PROPS.contains(&"opacity"));
    }

    #[test]
    fn strip_important_variants() {
        assert_eq!(strip_important("red"), "red");
        assert_eq!(strip_important("red !important"), "red");
        assert_eq!(strip_important("red ! IMPORTANT "), "red");
        assert_eq!(strip_important("red!important"), "red");
        assert_eq!(strip_important("important"), "important");
    }
}
