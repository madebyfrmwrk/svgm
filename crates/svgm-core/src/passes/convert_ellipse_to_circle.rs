use super::{Pass, PassResult};
use crate::ast::{Attribute, Document, NodeKind};

/// Converts `<ellipse>` to `<circle>` when `rx` equals `ry`.
pub struct ConvertEllipseToCircle;

impl Pass for ConvertEllipseToCircle {
    fn name(&self) -> &'static str {
        "convertEllipseToCircle"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            let elem = match &node.kind {
                NodeKind::Element(e) if e.name == "ellipse" && e.prefix.is_none() => e,
                _ => continue,
            };

            let rx = elem.attr("rx").unwrap_or("0");
            let ry = elem.attr("ry").unwrap_or("0");

            // Convert if rx == ry (string comparison, like SVGO)
            if rx != ry {
                continue;
            }

            let r_value = rx.to_string();

            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                elem.name = "circle".to_string();
                elem.attributes
                    .retain(|a| a.prefix.is_some() || (a.name != "rx" && a.name != "ry"));
                elem.attributes.push(Attribute {
                    prefix: None,
                    name: "r".to_string(),
                    value: r_value,
                });
            }
            changed = true;
        }

        if changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn converts_equal_radii() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><ellipse cx="10" cy="10" rx="5" ry="5"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(ConvertEllipseToCircle.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("<circle"));
        assert!(output.contains("r=\"5\""));
        assert!(!output.contains("rx"));
        assert!(!output.contains("ry"));
    }

    #[test]
    fn keeps_different_radii() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><ellipse cx="10" cy="10" rx="5" ry="10"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(ConvertEllipseToCircle.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(output.contains("<ellipse"));
    }

    #[test]
    fn converts_zero_radii() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><ellipse cx="10" cy="10"/></svg>"#;
        let mut doc = parse(input).unwrap();
        // Both default to "0", so they're equal
        assert_eq!(ConvertEllipseToCircle.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("<circle"));
        assert!(output.contains("r=\"0\""));
    }
}
