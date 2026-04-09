use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

pub struct CleanupAttrs;

impl Pass for CleanupAttrs {
    fn name(&self) -> &'static str {
        "cleanupAttrs"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                for attr in &mut elem.attributes {
                    // Minify inline style attributes
                    if attr.name == "style" && attr.prefix.is_none() {
                        let minified = minify_inline_style(&attr.value);
                        if minified != attr.value {
                            attr.value = minified;
                            changed = true;
                        }
                    } else {
                        let cleaned = cleanup_whitespace(&attr.value);
                        if cleaned != attr.value {
                            attr.value = cleaned;
                            changed = true;
                        }
                    }
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

/// Minify an inline style attribute: strip spaces around colons/semicolons,
/// remove trailing semicolons.
fn minify_inline_style(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for (i, part) in s.split(';').enumerate() {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if i > 0 && !result.is_empty() {
            result.push(';');
        }
        if let Some(colon) = trimmed.find(':') {
            let prop = trimmed[..colon].trim();
            let val = trimmed[colon + 1..].trim();
            result.push_str(prop);
            result.push(':');
            result.push_str(val);
        } else {
            result.push_str(trimmed);
        }
    }
    result
}

/// Collapse runs of whitespace (spaces, tabs, newlines) into a single space,
/// and trim leading/trailing whitespace.
fn cleanup_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_ws = true; // trim leading
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                result.push(' ');
                prev_ws = true;
            }
        } else {
            result.push(ch);
            prev_ws = false;
        }
    }
    // Trim trailing space
    if result.ends_with(' ') {
        result.pop();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn collapses_whitespace_in_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect class=\"  a   b  \"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("class=\"a b\""));
    }

    #[test]
    fn handles_newlines_in_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M 0\n0 L\n10 10\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupAttrs.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("d=\"M 0 0 L 10 10\""));
    }
}
