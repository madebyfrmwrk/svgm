use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

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
                    let cleaned = cleanup_whitespace(&attr.value);
                    if cleaned != attr.value {
                        attr.value = cleaned;
                        changed = true;
                    }
                }
            }
        }

        if changed { PassResult::Changed } else { PassResult::Unchanged }
    }
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
