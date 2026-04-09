use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Removes or simplifies the deprecated `enable-background` attribute.
/// If no `<filter>` elements exist, removes all `enable-background` attributes.
/// If filters exist, simplifies `enable-background="new 0 0 W H"` to `"new"`
/// when W and H match the element's width and height.
pub struct CleanupEnableBackground;

impl Pass for CleanupEnableBackground {
    fn name(&self) -> &'static str {
        "cleanupEnableBackground"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let ids = doc.traverse();

        // Check if any <filter> elements exist
        let has_filters = ids
            .iter()
            .any(|&id| matches!(&doc.node(id).kind, NodeKind::Element(e) if e.name == "filter"));

        let mut changed = false;

        for &id in &ids {
            let node = doc.node(id);
            let elem = match &node.kind {
                NodeKind::Element(e) => e,
                _ => continue,
            };

            // Handle enable-background as a standalone attribute
            let eb_idx = elem
                .attributes
                .iter()
                .position(|a| a.prefix.is_none() && a.name == "enable-background");

            if let Some(eb_idx) = eb_idx {
                if !has_filters {
                    let node = doc.node_mut(id);
                    if let NodeKind::Element(ref mut elem) = node.kind {
                        elem.attributes.remove(eb_idx);
                    }
                    changed = true;
                } else {
                    let eb_value = elem.attributes[eb_idx].value.clone();
                    if let Some(simplified) = try_simplify(&eb_value, elem) {
                        let node = doc.node_mut(id);
                        if let NodeKind::Element(ref mut elem) = node.kind {
                            elem.attributes[eb_idx].value = simplified;
                        }
                        changed = true;
                    }
                }
                continue;
            }

            // Handle enable-background inside style="" attribute
            let style_idx = elem
                .attributes
                .iter()
                .position(|a| a.prefix.is_none() && a.name == "style");
            if let Some(style_idx) = style_idx
                && elem.attributes[style_idx]
                    .value
                    .contains("enable-background")
                && !has_filters
            {
                // Remove the enable-background declaration from style
                let style_val = &elem.attributes[style_idx].value;
                let new_style = remove_prop_from_style(style_val, "enable-background");
                let node = doc.node_mut(id);
                if let NodeKind::Element(ref mut elem) = node.kind {
                    if new_style.is_empty() {
                        elem.attributes.remove(style_idx);
                    } else {
                        elem.attributes[style_idx].value = new_style;
                    }
                }
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

/// Remove a CSS property from an inline style string.
fn remove_prop_from_style(style: &str, prop: &str) -> String {
    style
        .split(';')
        .filter(|part| {
            let trimmed = part.trim();
            if let Some(colon) = trimmed.find(':') {
                trimmed[..colon].trim() != prop
            } else {
                !trimmed.is_empty()
            }
        })
        .collect::<Vec<_>>()
        .join(";")
        .trim_matches(';')
        .to_string()
}

/// Try to simplify `enable-background="new X Y W H"` to `"new"` when X=0, Y=0
/// and W/H match the element's width/height attributes.
fn try_simplify(value: &str, elem: &crate::ast::Element) -> Option<String> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() != 5 || parts[0] != "new" {
        return None;
    }

    let x: f64 = parts[1].parse().ok()?;
    let y: f64 = parts[2].parse().ok()?;
    if x != 0.0 || y != 0.0 {
        return None;
    }

    let eb_w = parts[3];
    let eb_h = parts[4];

    let elem_w = elem.attr("width")?;
    let elem_h = elem.attr("height")?;

    // Compare as floats to handle equivalent representations
    let ebw: f64 = eb_w.parse().ok()?;
    let ebh: f64 = eb_h.parse().ok()?;
    let ew: f64 = elem_w.parse().ok()?;
    let eh: f64 = elem_h.parse().ok()?;

    if (ebw - ew).abs() < f64::EPSILON && (ebh - eh).abs() < f64::EPSILON {
        Some("new".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_when_no_filters() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" enable-background="new 0 0 100 100"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupEnableBackground.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("enable-background"));
    }

    #[test]
    fn simplifies_when_matches_dimensions() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" enable-background="new 0 0 100 50"><defs><filter id="f"/></defs><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupEnableBackground.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("enable-background=\"new\""));
    }

    #[test]
    fn keeps_when_dimensions_differ() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="50" enable-background="new 0 0 100 50"><defs><filter id="f"/></defs><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupEnableBackground.run(&mut doc), PassResult::Unchanged);
    }

    #[test]
    fn unchanged_when_no_enable_background() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(CleanupEnableBackground.run(&mut doc), PassResult::Unchanged);
    }
}
