use super::{Pass, PassResult, has_structural_css};
use crate::ast::{Attribute, Document, NodeKind};

/// Converts `<ellipse>` to `<circle>` when `rx` equals `ry`, or when one
/// radius is `auto` / absent (SVG2: an auto radius takes the other radius'
/// value, so the ellipse already renders as a circle).
pub struct ConvertEllipseToCircle;

impl Pass for ConvertEllipseToCircle {
    fn name(&self) -> &'static str {
        "convertEllipseToCircle"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        // A stylesheet may select on the element name (`g ellipse`,
        // `ellipse:first-child`): renaming the element would change which
        // rules apply. Same guard as convertShapeToPath.
        if has_structural_css(doc) {
            return PassResult::Unchanged;
        }

        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            let elem = match &node.kind {
                NodeKind::Element(e) if e.name == "ellipse" && e.prefix.is_none() => e,
                _ => continue,
            };

            // Absent and "auto" both mean SVG2 auto.
            let rx = elem.attr("rx").filter(|v| v.trim() != "auto");
            let ry = elem.attr("ry").filter(|v| v.trim() != "auto");

            let r_value = match (rx, ry) {
                // Both auto: the used value of both is 0 (not rendered).
                (None, None) => "0".to_string(),
                (Some(rx), None) => rx.to_string(),
                (None, Some(ry)) => ry.to_string(),
                // String comparison, like SVGO.
                (Some(rx), Some(ry)) if rx == ry => rx.to_string(),
                _ => continue,
            };

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
        // Both auto: used value 0
        assert_eq!(ConvertEllipseToCircle.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("<circle"));
        assert!(output.contains("r=\"0\""));
    }

    #[test]
    fn converts_single_radius_svg2_auto() {
        // SVG2: a missing radius is auto = the other radius -> a circle.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><ellipse cx="10" cy="10" ry="7"/><ellipse cx="1" cy="1" rx="3"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(ConvertEllipseToCircle.run(&mut doc), PassResult::Changed);
        assert_eq!(
            serialize(&doc),
            r#"<svg xmlns="http://www.w3.org/2000/svg"><circle cx="10" cy="10" r="7"/><circle cx="1" cy="1" r="3"/></svg>"#
        );
    }

    #[test]
    fn converts_explicit_auto_radius() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><ellipse cx="10" cy="10" rx="auto" ry="7"/><ellipse rx="4" ry="auto"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(ConvertEllipseToCircle.run(&mut doc), PassResult::Changed);
        assert_eq!(
            serialize(&doc),
            r#"<svg xmlns="http://www.w3.org/2000/svg"><circle cx="10" cy="10" r="7"/><circle r="4"/></svg>"#
        );
    }

    #[test]
    fn structural_css_blocks_conversion() {
        // `g ellipse{fill:red}` would stop matching after the rename and
        // `g circle{fill:blue}` would start to: the shape must stay an ellipse.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><style>g ellipse{fill:red}g circle{fill:blue}</style><g><ellipse rx="5" ry="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(ConvertEllipseToCircle.run(&mut doc), PassResult::Unchanged);
        let output = serialize(&doc);
        assert!(output.contains("<ellipse rx=\"5\" ry=\"5\"/>"), "{output}");
        assert!(!output.contains("<circle"), "{output}");
    }

    #[test]
    fn empty_stylesheet_does_not_block_conversion() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><style> </style><ellipse rx="5" ry="5"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(ConvertEllipseToCircle.run(&mut doc), PassResult::Changed);
        assert!(serialize(&doc).contains("<circle r=\"5\"/>"));
    }
}
