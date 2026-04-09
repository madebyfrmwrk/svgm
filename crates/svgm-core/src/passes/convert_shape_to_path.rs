use super::{Pass, PassResult};
use crate::ast::{Attribute, Document, NodeKind};

pub struct ConvertShapeToPath {
    pub precision: u32,
}

impl Default for ConvertShapeToPath {
    fn default() -> Self {
        Self { precision: 3 }
    }
}

impl Pass for ConvertShapeToPath {
    fn name(&self) -> &'static str {
        "convertShapeToPath"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            let elem = match &node.kind {
                NodeKind::Element(e) => e,
                _ => continue,
            };

            let shape_name = elem.name.as_str();
            match shape_name {
                "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {}
                _ => continue,
            }

            // Safety: skip shapes with pathLength attribute
            if elem.attr("pathLength").is_some() {
                continue;
            }

            // Skip shapes with animation children targeting geometric attributes
            if has_animated_geometry(doc, id, shape_name) {
                continue;
            }

            // Generate path d string
            let d = match shape_name {
                "rect" => rect_to_path_d(elem, self.precision),
                "circle" => circle_to_path_d(elem, self.precision),
                "ellipse" => ellipse_to_path_d(elem, self.precision),
                "line" => line_to_path_d(elem, self.precision),
                "polyline" => poly_to_path_d(elem, self.precision, false),
                "polygon" => poly_to_path_d(elem, self.precision, true),
                _ => None,
            };

            let Some(d) = d else { continue };

            // Only convert if the path form is strictly shorter in bytes.
            if !is_path_shorter(elem, &d) {
                continue;
            }

            // Mutate in place: shape → path
            let geometric = geometric_attr_names(shape_name);
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                elem.name = "path".to_string();
                elem.attributes
                    .retain(|a| a.prefix.is_some() || !geometric.contains(&a.name.as_str()));
                elem.attributes.push(Attribute {
                    prefix: None,
                    name: "d".to_string(),
                    value: d,
                });
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

/// Geometric attribute names for each shape type.
fn geometric_attr_names(shape_name: &str) -> &'static [&'static str] {
    match shape_name {
        "rect" => &["x", "y", "width", "height", "rx", "ry"],
        "circle" => &["cx", "cy", "r"],
        "ellipse" => &["cx", "cy", "rx", "ry"],
        "line" => &["x1", "y1", "x2", "y2"],
        "polyline" | "polygon" => &["points"],
        _ => &[],
    }
}

/// Check if any direct animation child targets a geometric attribute of this shape.
fn has_animated_geometry(doc: &Document, id: crate::ast::NodeId, shape_name: &str) -> bool {
    let geometric = geometric_attr_names(shape_name);

    for child_id in doc.children(id) {
        let child = doc.node(child_id);
        if let NodeKind::Element(ref child_elem) = child.kind {
            match child_elem.name.as_str() {
                "animate" | "animateTransform" | "animateMotion" | "set" => {
                    if let Some(attr_name) = child_elem.attr("attributeName")
                        && geometric.contains(&attr_name)
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    false
}

/// Compare serialized element lengths: original shape vs equivalent path.
fn is_path_shorter(elem: &crate::ast::Element, d: &str) -> bool {
    let geometric = geometric_attr_names(&elem.name);

    // Original: <{shape_name} {all_attrs}/>
    // Path:     <path {non_geo_attrs} d="{d}"/>
    // Non-geometric attrs are identical, so we compare the difference.

    // Cost of shape name
    let shape_cost = elem.name.len();
    // Cost of geometric attributes: ` name="value"` each
    let mut geo_cost: usize = 0;
    for attr in &elem.attributes {
        if attr.prefix.is_none() && geometric.contains(&attr.name.as_str()) {
            geo_cost += 1 + attr.name.len() + 2 + attr.value.len() + 1;
            // space + name + =" + value + "
        }
    }

    // Path side: name "path" (4) + d attribute ` d="{d}"` (5 + d.len())
    let path_cost = 4 + 5 + d.len();
    let original_cost = shape_cost + geo_cost;

    path_cost < original_cost
}

// --- Shape conversion functions ---

fn rect_to_path_d(elem: &crate::ast::Element, precision: u32) -> Option<String> {
    let x = parse_attr(elem, "x").unwrap_or(0.0);
    let y = parse_attr(elem, "y").unwrap_or(0.0);
    let w = parse_attr(elem, "width")?;
    let h = parse_attr(elem, "height")?;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }

    let (rx, ry) = resolve_rect_radii(elem, w, h);

    if approx_eq(rx, 0.0, precision) && approx_eq(ry, 0.0, precision) {
        // Simple rect
        Some(format!(
            "M{} {}h{}v{}H{}z",
            fmt(x, precision),
            fmt(y, precision),
            fmt(w, precision),
            fmt(h, precision),
            fmt(x, precision),
        ))
    } else {
        // Rounded rect with arcs
        let w2 = w - 2.0 * rx;
        let h2 = h - 2.0 * ry;
        Some(format!(
            "M{} {}h{}a{} {} 0 0 1 {} {}v{}a{} {} 0 0 1 {} {}h{}a{} {} 0 0 1 {} {}v{}a{} {} 0 0 1 {} {}z",
            fmt(x + rx, precision),
            fmt(y, precision),
            fmt(w2, precision),
            fmt(rx, precision),
            fmt(ry, precision),
            fmt(rx, precision),
            fmt(ry, precision),
            fmt(h2, precision),
            fmt(rx, precision),
            fmt(ry, precision),
            fmt(-rx, precision),
            fmt(ry, precision),
            fmt(-w2, precision),
            fmt(rx, precision),
            fmt(ry, precision),
            fmt(-rx, precision),
            fmt(-ry, precision),
            fmt(-h2, precision),
            fmt(rx, precision),
            fmt(ry, precision),
            fmt(rx, precision),
            fmt(-ry, precision),
        ))
    }
}

/// Resolve rx/ry for a rect per SVG spec.
fn resolve_rect_radii(elem: &crate::ast::Element, w: f64, h: f64) -> (f64, f64) {
    let rx = parse_attr(elem, "rx");
    let ry = parse_attr(elem, "ry");

    let (mut rx, mut ry) = match (rx, ry) {
        (Some(rx), Some(ry)) => (rx, ry),
        (Some(rx), None) => (rx, rx),
        (None, Some(ry)) => (ry, ry),
        (None, None) => return (0.0, 0.0),
    };

    // Clamp per SVG spec
    rx = rx.max(0.0).min(w / 2.0);
    ry = ry.max(0.0).min(h / 2.0);

    (rx, ry)
}

fn circle_to_path_d(elem: &crate::ast::Element, precision: u32) -> Option<String> {
    let cx = parse_attr(elem, "cx").unwrap_or(0.0);
    let cy = parse_attr(elem, "cy").unwrap_or(0.0);
    let r = parse_attr(elem, "r")?;
    if r <= 0.0 {
        return None;
    }

    let d2r = 2.0 * r;
    Some(format!(
        "M{} {}a{} {} 0 1 0 {} 0a{} {} 0 1 0 {} 0z",
        fmt(cx - r, precision),
        fmt(cy, precision),
        fmt(r, precision),
        fmt(r, precision),
        fmt(d2r, precision),
        fmt(r, precision),
        fmt(r, precision),
        fmt(-d2r, precision),
    ))
}

fn ellipse_to_path_d(elem: &crate::ast::Element, precision: u32) -> Option<String> {
    let cx = parse_attr(elem, "cx").unwrap_or(0.0);
    let cy = parse_attr(elem, "cy").unwrap_or(0.0);
    let rx = parse_attr(elem, "rx")?;
    let ry = parse_attr(elem, "ry")?;
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }

    let d2rx = 2.0 * rx;
    Some(format!(
        "M{} {}a{} {} 0 1 0 {} 0a{} {} 0 1 0 {} 0z",
        fmt(cx - rx, precision),
        fmt(cy, precision),
        fmt(rx, precision),
        fmt(ry, precision),
        fmt(d2rx, precision),
        fmt(rx, precision),
        fmt(ry, precision),
        fmt(-d2rx, precision),
    ))
}

fn line_to_path_d(elem: &crate::ast::Element, precision: u32) -> Option<String> {
    let x1 = parse_attr(elem, "x1").unwrap_or(0.0);
    let y1 = parse_attr(elem, "y1").unwrap_or(0.0);
    let x2 = parse_attr(elem, "x2").unwrap_or(0.0);
    let y2 = parse_attr(elem, "y2").unwrap_or(0.0);

    Some(format!(
        "M{} {}L{} {}",
        fmt(x1, precision),
        fmt(y1, precision),
        fmt(x2, precision),
        fmt(y2, precision),
    ))
}

fn poly_to_path_d(elem: &crate::ast::Element, precision: u32, close: bool) -> Option<String> {
    let points_str = elem.attr("points")?;
    let points = parse_points(points_str)?;
    if points.is_empty() {
        return None;
    }

    let mut d = format!(
        "M{} {}",
        fmt(points[0].0, precision),
        fmt(points[0].1, precision)
    );
    for &(x, y) in &points[1..] {
        d.push('L');
        d.push_str(&fmt(x, precision));
        d.push(' ');
        d.push_str(&fmt(y, precision));
    }
    if close {
        d.push('z');
    }
    Some(d)
}

/// Parse SVG `points` attribute: numbers separated by commas/whitespace.
/// Returns None on parse failure or odd number of values.
fn parse_points(s: &str) -> Option<Vec<(f64, f64)>> {
    let nums = parse_number_list(s)?;
    if nums.len() % 2 != 0 || nums.is_empty() {
        return None;
    }
    let points: Vec<(f64, f64)> = nums.chunks(2).map(|c| (c[0], c[1])).collect();
    Some(points)
}

/// Parse a list of numbers separated by commas and/or whitespace.
fn parse_number_list(s: &str) -> Option<Vec<f64>> {
    let mut nums = Vec::new();
    let chars = s.as_bytes();
    let mut i = 0;

    while i < chars.len() {
        // Skip whitespace and commas
        while i < chars.len()
            && (chars[i] == b' '
                || chars[i] == b','
                || chars[i] == b'\t'
                || chars[i] == b'\n'
                || chars[i] == b'\r')
        {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }

        // Parse a number
        let start = i;
        // Optional sign
        if i < chars.len() && (chars[i] == b'-' || chars[i] == b'+') {
            i += 1;
        }
        // Digits before decimal
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        // Optional decimal point + digits
        if i < chars.len() && chars[i] == b'.' {
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
        // Optional exponent
        if i < chars.len() && (chars[i] == b'e' || chars[i] == b'E') {
            i += 1;
            if i < chars.len() && (chars[i] == b'-' || chars[i] == b'+') {
                i += 1;
            }
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }

        if i == start {
            return None; // Not a number
        }

        let num_str = std::str::from_utf8(&chars[start..i]).ok()?;
        let val: f64 = num_str.parse().ok()?;
        nums.push(val);
    }

    Some(nums)
}

// --- Helpers ---

fn parse_attr(elem: &crate::ast::Element, name: &str) -> Option<f64> {
    elem.attr(name).and_then(|v| v.parse::<f64>().ok())
}

fn approx_eq(a: f64, b: f64, precision: u32) -> bool {
    let factor = 10f64.powi(precision as i32);
    (a * factor).round() == (b * factor).round()
}

fn fmt(val: f64, precision: u32) -> String {
    let factor = 10f64.powi(precision as i32);
    let rounded = (val * factor).round() / factor;
    if rounded == 0.0 {
        return "0".to_string();
    }
    let s = format!("{:.prec$}", rounded, prec = precision as usize);
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let pass = ConvertShapeToPath::default();
        let result = pass.run(&mut doc);
        (result, serialize(&doc))
    }

    // --- Basic conversions ---

    #[test]
    fn rect_to_path() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect x=\"10\" y=\"20\" width=\"100\" height=\"50\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<path "));
        assert!(output.contains("d=\"M10 20h100v50H10z\""));
        assert!(!output.contains("<rect"));
    }

    #[test]
    fn rect_default_xy() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"100\" height=\"50\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("d=\"M0 0h100v50H0z\""));
    }

    #[test]
    fn circle_stays_when_path_is_longer() {
        // Arc-based path is longer than the original circle element
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"50\" cy=\"50\" r=\"25\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<circle"));
    }

    #[test]
    fn ellipse_stays_when_path_is_longer() {
        // Arc-based path is longer than the original ellipse element
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><ellipse cx=\"50\" cy=\"50\" rx=\"30\" ry=\"20\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<ellipse"));
    }

    #[test]
    fn line_to_path() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><line x1=\"0\" y1=\"0\" x2=\"100\" y2=\"50\" stroke=\"black\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<path "));
        assert!(output.contains("d=\"M0 0L100 50\""));
        assert!(output.contains("stroke=\"black\""));
    }

    #[test]
    fn polyline_to_path() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><polyline points=\"0,0 50,25 100,0\" stroke=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<path "));
        assert!(output.contains("d=\"M0 0L50 25L100 0\""));
        assert!(output.contains("stroke=\"red\""));
    }

    #[test]
    fn polygon_to_path() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><polygon points=\"0,0 50,25 100,0\" fill=\"blue\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<path "));
        assert!(output.contains("d=\"M0 0L50 25L100 0z\""));
        assert!(output.contains("fill=\"blue\""));
    }

    // --- Attribute preservation ---

    #[test]
    fn preserves_non_geometric_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect fill=\"red\" class=\"box\" x=\"0\" y=\"0\" width=\"100\" height=\"50\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("fill=\"red\""));
        assert!(output.contains("class=\"box\""));
        assert!(!output.contains("width="));
        assert!(!output.contains("height="));
    }

    #[test]
    fn preserves_id_attr() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"myRect\" x=\"0\" y=\"0\" width=\"100\" height=\"50\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("id=\"myRect\""));
    }

    // --- Animation skip ---

    #[test]
    fn skips_rect_with_animated_width() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"100\" height=\"50\"><animate attributeName=\"width\" from=\"0\" to=\"100\" dur=\"1s\"/></rect></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn converts_rect_with_non_geometric_animation() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"100\" height=\"50\"><animate attributeName=\"opacity\" from=\"0\" to=\"1\" dur=\"1s\"/></rect></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("<path "));
    }

    #[test]
    fn skips_circle_with_animated_r() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"50\" cy=\"50\" r=\"25\"><animate attributeName=\"r\" from=\"10\" to=\"50\" dur=\"2s\"/></circle></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Ellipse ---

    #[test]
    fn ellipse_equal_radii_unchanged_by_this_pass() {
        // convertEllipseToCircle handles ellipse→circle conversion as a separate pass.
        // This pass only converts shapes to paths when shorter.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><ellipse cx=\"50\" cy=\"50\" rx=\"25\" ry=\"25\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<ellipse"));
    }

    // --- Edge cases ---

    #[test]
    fn zero_size_rect_skipped() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"0\" height=\"50\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<rect"));
    }

    #[test]
    fn zero_radius_circle_skipped() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"50\" cy=\"50\" r=\"0\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<circle"));
    }

    #[test]
    fn polygon_empty_points_skipped() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><polygon points=\"\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<polygon"));
    }

    #[test]
    fn rect_rx_only_defaults_ry() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"100\" height=\"50\" rx=\"5\"/></svg>";
        let (_, output) = run_pass(input);
        // rx=5 ry=5 (defaulted from rx)
        // Whether this converts depends on length comparison — the rounded rect path is verbose
        // Just verify no panic and valid output
        assert!(output.contains("<svg"));
    }

    #[test]
    fn rect_rx_clamped() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\" rx=\"20\"/></svg>";
        let (_, output) = run_pass(input);
        // rx clamped to width/2 = 5, ry clamped to height/2 = 5
        assert!(output.contains("<svg"));
    }

    #[test]
    fn skips_shape_with_path_length() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"50\" cy=\"50\" r=\"25\" pathLength=\"100\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<circle"));
    }

    #[test]
    fn points_various_separators() {
        // Commas, spaces, mixed
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><polygon points=\"10 20, 30 40,50 60\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert!(output.contains("d=\"M10 20L30 40L50 60z\""));
    }

    #[test]
    fn polyline_odd_number_of_values_skipped() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><polyline points=\"10,20,30\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Length comparison ---

    #[test]
    fn skips_when_path_is_longer() {
        // A line with all defaults: <line/> is 4 chars for name, no geo attrs serialized (all default 0)
        // vs <path d="M0 0L0 0"/> — path is longer
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><line/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(output.contains("<line"));
    }

    // --- Integration with full optimizer ---

    #[test]
    fn full_optimizer_convergence() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect x=\"10\" y=\"20\" width=\"100\" height=\"50\" fill=\"red\"/><line x1=\"0\" y1=\"0\" x2=\"200\" y2=\"100\" stroke=\"black\"/></svg>";
        let result1 = crate::optimize(input).unwrap();
        let result2 = crate::optimize(&result1.data).unwrap();
        assert_eq!(result1.data, result2.data, "should converge in one pass");
    }
}
