use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Attributes whose values are numeric and can be cleaned up.
const NUMERIC_ATTRS: &[&str] = &[
    "width",
    "height",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "fx",
    "fy",
    "fr",
    "stroke-width",
    "stroke-dashoffset",
    "stroke-miterlimit",
    "opacity",
    "fill-opacity",
    "stroke-opacity",
    "stop-opacity",
    "font-size",
    "letter-spacing",
    "word-spacing",
    "baseline-shift",
    "dx",
    "dy",
];

pub struct CleanupNumericValues {
    pub precision: u32,
}

impl Default for CleanupNumericValues {
    fn default() -> Self {
        Self { precision: 3 }
    }
}

impl Pass for CleanupNumericValues {
    fn name(&self) -> &'static str {
        "cleanupNumericValues"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                for attr in &mut elem.attributes {
                    if attr.prefix.is_some() {
                        continue;
                    }
                    if NUMERIC_ATTRS.contains(&attr.name.as_str())
                        && let Some(cleaned) = cleanup_numeric(&attr.value, self.precision)
                        && cleaned != attr.value
                    {
                        attr.value = cleaned;
                        changed = true;
                    }
                    // Also clean viewBox values
                    if attr.name == "viewBox"
                        && let Some(cleaned) = cleanup_viewbox(&attr.value, self.precision)
                        && cleaned != attr.value
                    {
                        attr.value = cleaned;
                        changed = true;
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

/// Clean a single numeric value: remove trailing zeros, default `px` unit, leading zero.
fn cleanup_numeric(value: &str, precision: u32) -> Option<String> {
    let trimmed = value.trim();

    // Strip `px` suffix (default SVG unit)
    let (num_str, _unit) = if let Some(s) = trimmed.strip_suffix("px") {
        (s, "")
    } else if trimmed.ends_with('%')
        || trimmed.ends_with("em")
        || trimmed.ends_with("ex")
        || trimmed.ends_with("pt")
        || trimmed.ends_with("pc")
        || trimmed.ends_with("cm")
        || trimmed.ends_with("mm")
        || trimmed.ends_with("in")
    {
        // Keep non-default units as-is for now
        return None;
    } else {
        (trimmed, "")
    };

    let num: f64 = num_str.parse().ok()?;
    let rounded = round_to(num, precision);
    let result = format_number(rounded);
    Some(result)
}

/// Clean viewBox: "0 0 100.000 200.000" → "0 0 100 200"
fn cleanup_viewbox(value: &str, precision: u32) -> Option<String> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }
    let mut cleaned = Vec::with_capacity(4);
    for part in &parts {
        let num: f64 = part.parse().ok()?;
        cleaned.push(format_number(round_to(num, precision)));
    }
    Some(cleaned.join(" "))
}

fn round_to(value: f64, precision: u32) -> f64 {
    let factor = 10f64.powi(precision as i32);
    (value * factor).round() / factor
}

/// Format a number, stripping trailing zeros and unnecessary decimal points.
/// Also removes leading zero for values between -1 and 1.
fn format_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }

    let s = format!("{:.10}", value);
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');

    // Remove leading zero: 0.5 → .5, -0.5 → -.5

    if let Some(rest) = s.strip_prefix("0.") {
        format!(".{rest}")
    } else if let Some(rest) = s.strip_prefix("-0.") {
        format!("-.{rest}")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(0.5), ".5");
        assert_eq!(format_number(-0.5), "-.5");
        assert_eq!(format_number(100.0), "100");
        assert_eq!(format_number(3.14), "3.14");
    }

    #[test]
    fn test_cleanup_numeric() {
        assert_eq!(cleanup_numeric("100.000px", 3), Some("100".to_string()));
        assert_eq!(cleanup_numeric("0.500", 3), Some(".5".to_string()));
        assert_eq!(cleanup_numeric("10.1234567", 3), Some("10.123".to_string()));
        assert_eq!(cleanup_numeric("50%", 3), None); // percentage kept as-is
    }

    #[test]
    fn test_cleanup_viewbox() {
        assert_eq!(
            cleanup_viewbox("0 0 100.000 200.000", 3),
            Some("0 0 100 200".to_string())
        );
    }

    #[test]
    fn pass_cleans_numeric_attrs() {
        use crate::parser::parse;
        use crate::serializer::serialize;

        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100.000px" height="200.000px" viewBox="0 0 100.000 200.000"><rect x="10.00" y="20.00"/></svg>"#;
        let mut doc = parse(input).unwrap();
        let pass = CleanupNumericValues::default();
        assert_eq!(pass.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("width=\"100\""));
        assert!(output.contains("height=\"200\""));
        assert!(output.contains("viewBox=\"0 0 100 200\""));
    }
}
