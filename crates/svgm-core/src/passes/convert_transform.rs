use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

pub struct ConvertTransform {
    pub precision: u32,
}

impl Default for ConvertTransform {
    fn default() -> Self {
        Self { precision: 3 }
    }
}

impl Pass for ConvertTransform {
    fn name(&self) -> &'static str {
        "convertTransform"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                let transform_pos = elem
                    .attributes
                    .iter()
                    .position(|a| a.name == "transform" && a.prefix.is_none());
                let Some(pos) = transform_pos else { continue };

                let transform_str = &elem.attributes[pos].value;
                let Some(matrix) = parse_and_merge_transforms(transform_str) else {
                    continue;
                };

                if matrix.is_identity(self.precision) {
                    // Identity transform — remove the attribute entirely
                    elem.attributes.remove(pos);
                    changed = true;
                    continue;
                }

                // Try to apply pure translate directly to element coordinates
                if let Some((tx, ty)) = matrix.as_translate(self.precision)
                    && apply_translate_to_element(elem, tx, ty, self.precision)
                {
                    elem.attributes.remove(pos);
                    changed = true;
                    continue;
                }

                // Simplify the transform string
                let simplified = matrix.serialize(self.precision);
                if simplified.len() < elem.attributes[pos].value.len() {
                    elem.attributes[pos].value = simplified;
                    changed = true;
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

/// 2D affine transformation matrix: [a c e; b d f; 0 0 1]
#[derive(Debug, Clone, Copy)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    fn translate(tx: f64, ty: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        }
    }

    fn scale(sx: f64, sy: f64) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        }
    }

    fn rotate(angle_deg: f64) -> Self {
        let r = angle_deg.to_radians();
        let (sin, cos) = r.sin_cos();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        }
    }

    fn skew_x(angle_deg: f64) -> Self {
        let t = angle_deg.to_radians().tan();
        Self {
            a: 1.0,
            b: 0.0,
            c: t,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    fn skew_y(angle_deg: f64) -> Self {
        let t = angle_deg.to_radians().tan();
        Self {
            a: 1.0,
            b: t,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    fn multiply(&self, other: &Matrix) -> Self {
        Self {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    fn is_identity(&self, precision: u32) -> bool {
        approx_eq(self.a, 1.0, precision)
            && approx_eq(self.b, 0.0, precision)
            && approx_eq(self.c, 0.0, precision)
            && approx_eq(self.d, 1.0, precision)
            && approx_eq(self.e, 0.0, precision)
            && approx_eq(self.f, 0.0, precision)
    }

    fn as_translate(&self, precision: u32) -> Option<(f64, f64)> {
        if approx_eq(self.a, 1.0, precision)
            && approx_eq(self.b, 0.0, precision)
            && approx_eq(self.c, 0.0, precision)
            && approx_eq(self.d, 1.0, precision)
        {
            Some((self.e, self.f))
        } else {
            None
        }
    }

    fn as_scale(&self, precision: u32) -> Option<(f64, f64)> {
        if approx_eq(self.b, 0.0, precision)
            && approx_eq(self.c, 0.0, precision)
            && approx_eq(self.e, 0.0, precision)
            && approx_eq(self.f, 0.0, precision)
        {
            Some((self.a, self.d))
        } else {
            None
        }
    }

    fn as_rotate(&self, precision: u32) -> Option<f64> {
        if approx_eq(self.e, 0.0, precision)
            && approx_eq(self.f, 0.0, precision)
            && approx_eq(self.a, self.d, precision)
            && approx_eq(self.b, -self.c, precision)
            && approx_eq(self.a * self.a + self.b * self.b, 1.0, precision)
        {
            Some(self.b.atan2(self.a).to_degrees())
        } else {
            None
        }
    }

    fn serialize(&self, precision: u32) -> String {
        // Try to represent as the shortest named transform
        if let Some((tx, ty)) = self.as_translate(precision) {
            if approx_eq(ty, 0.0, precision) {
                return format!("translate({})", fmt(tx, precision));
            }
            return format!("translate({},{})", fmt(tx, precision), fmt(ty, precision));
        }
        if let Some(angle) = self.as_rotate(precision) {
            return format!("rotate({})", fmt(angle, precision));
        }
        if let Some((sx, sy)) = self.as_scale(precision) {
            if approx_eq(sx, sy, precision) {
                return format!("scale({})", fmt(sx, precision));
            }
            return format!("scale({},{})", fmt(sx, precision), fmt(sy, precision));
        }
        // Fall back to matrix
        format!(
            "matrix({},{},{},{},{},{})",
            fmt(self.a, precision),
            fmt(self.b, precision),
            fmt(self.c, precision),
            fmt(self.d, precision),
            fmt(self.e, precision),
            fmt(self.f, precision),
        )
    }
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

/// Parse a transform attribute string and merge all transforms into a single matrix.
fn parse_and_merge_transforms(s: &str) -> Option<Matrix> {
    let mut result = Matrix::identity();
    let mut chars = s.chars().peekable();

    loop {
        skip_ws(&mut chars);
        if chars.peek().is_none() {
            break;
        }

        // Read function name
        let mut name = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_alphabetic() {
                name.push(c);
                chars.next();
            } else {
                break;
            }
        }

        if name.is_empty() {
            return None;
        }

        skip_ws(&mut chars);
        // Expect '('
        if chars.next() != Some('(') {
            return None;
        }

        // Read args
        let mut args = Vec::new();
        loop {
            skip_ws_comma(&mut chars);
            if let Some(&')') = chars.peek() {
                chars.next();
                break;
            }
            if let Some(n) = parse_num(&mut chars) {
                args.push(n);
            } else {
                return None;
            }
        }

        let m = match name.as_str() {
            "translate" => match args.len() {
                1 => Matrix::translate(args[0], 0.0),
                2 => Matrix::translate(args[0], args[1]),
                _ => return None,
            },
            "scale" => match args.len() {
                1 => Matrix::scale(args[0], args[0]),
                2 => Matrix::scale(args[0], args[1]),
                _ => return None,
            },
            "rotate" => match args.len() {
                1 => Matrix::rotate(args[0]),
                3 => {
                    // rotate(angle, cx, cy) = translate(cx,cy) rotate(angle) translate(-cx,-cy)
                    let t1 = Matrix::translate(args[1], args[2]);
                    let r = Matrix::rotate(args[0]);
                    let t2 = Matrix::translate(-args[1], -args[2]);
                    t1.multiply(&r).multiply(&t2)
                }
                _ => return None,
            },
            "skewX" if args.len() == 1 => Matrix::skew_x(args[0]),
            "skewY" if args.len() == 1 => Matrix::skew_y(args[0]),
            "matrix" if args.len() == 6 => Matrix {
                a: args[0],
                b: args[1],
                c: args[2],
                d: args[3],
                e: args[4],
                f: args[5],
            },
            _ => return None,
        };

        result = result.multiply(&m);

        // Optional comma between transforms
        skip_ws(&mut chars);
        if let Some(&',') = chars.peek() {
            chars.next();
        }
    }

    Some(result)
}

/// Apply a pure translate to element coordinate attributes.
/// Returns true if the translate was successfully applied.
fn apply_translate_to_element(
    elem: &mut crate::ast::Element,
    tx: f64,
    ty: f64,
    precision: u32,
) -> bool {
    // Only apply to elements with known coordinate attributes
    // Skip elements that might be referenced (<use>, etc.) or have complex semantics
    let name = elem.name.as_str();
    let attr_pairs: &[(&str, &str)] = match name {
        "rect" | "image" | "foreignObject" => &[("x", "y")],
        "circle" | "ellipse" => &[("cx", "cy")],
        "text" | "tspan" => &[("x", "y")],
        "use" => &[("x", "y")],
        "line" => &[("x1", "y1"), ("x2", "y2")],
        _ => return false,
    };

    // Verify all coordinate attributes either exist or default to 0
    for &(x_attr, y_attr) in attr_pairs {
        let x_val = elem
            .attributes
            .iter()
            .find(|a| a.name == x_attr && a.prefix.is_none())
            .map(|a| a.value.parse::<f64>().ok())
            .unwrap_or(Some(0.0));
        let y_val = elem
            .attributes
            .iter()
            .find(|a| a.name == y_attr && a.prefix.is_none())
            .map(|a| a.value.parse::<f64>().ok())
            .unwrap_or(Some(0.0));

        if x_val.is_none() || y_val.is_none() {
            return false; // Can't parse existing coordinate
        }
    }

    // Apply the translation
    for &(x_attr, y_attr) in attr_pairs {
        apply_offset(elem, x_attr, tx, precision);
        apply_offset(elem, y_attr, ty, precision);
    }

    true
}

fn apply_offset(elem: &mut crate::ast::Element, attr_name: &str, offset: f64, precision: u32) {
    if let Some(attr) = elem
        .attributes
        .iter_mut()
        .find(|a| a.name == attr_name && a.prefix.is_none())
    {
        if let Ok(val) = attr.value.parse::<f64>() {
            attr.value = fmt(val + offset, precision);
        }
    } else if !approx_eq(offset, 0.0, precision) {
        // Attribute doesn't exist — create it with the offset value (default was 0)
        elem.attributes.push(crate::ast::Attribute {
            prefix: None,
            name: attr_name.to_string(),
            value: fmt(offset, precision),
        });
    }
}

fn skip_ws(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(&c) = chars.peek() {
        if c.is_ascii_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
}

fn skip_ws_comma(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(&c) = chars.peek() {
        if c.is_ascii_whitespace() || c == ',' {
            chars.next();
        } else {
            break;
        }
    }
}

fn parse_num(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<f64> {
    skip_ws_comma(chars);
    let mut s = String::new();
    if let Some(&c) = chars.peek()
        && (c == '-' || c == '+')
    {
        s.push(c);
        chars.next();
    }
    let mut has_digits = false;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            s.push(c);
            chars.next();
            has_digits = true;
        } else {
            break;
        }
    }
    if let Some(&'.') = chars.peek() {
        s.push('.');
        chars.next();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                chars.next();
                has_digits = true;
            } else {
                break;
            }
        }
    }
    if !has_digits {
        return None;
    }
    if let Some(&c) = chars.peek()
        && (c == 'e' || c == 'E')
    {
        s.push(c);
        chars.next();
        if let Some(&c) = chars.peek()
            && (c == '+' || c == '-')
        {
            s.push(c);
            chars.next();
        }
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                chars.next();
            } else {
                break;
            }
        }
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn merges_consecutive_translates() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect transform="translate(10,20) translate(5,5)"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        // Merged translate(15,25) is applied directly to rect coords
        assert!(
            !output.contains("transform"),
            "transform should be applied to coords: {output}"
        );
        assert!(output.contains("x=\"15\""), "x should be 15: {output}");
        assert!(output.contains("y=\"25\""), "y should be 25: {output}");
    }

    #[test]
    fn merges_consecutive_scales() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect transform="scale(2) scale(3)"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(output.contains("scale(6)"), "should merge scales: {output}");
    }

    #[test]
    fn merges_consecutive_rotates() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect transform="rotate(45) rotate(45)"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(
            output.contains("rotate(90)"),
            "should merge rotates: {output}"
        );
    }

    #[test]
    fn removes_identity_transform() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect transform="translate(0,0)"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(
            !output.contains("transform"),
            "identity should be removed: {output}"
        );
    }

    #[test]
    fn removes_identity_scale() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect transform="scale(1)"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(
            !output.contains("transform"),
            "scale(1) should be removed: {output}"
        );
    }

    #[test]
    fn applies_translate_to_rect() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect transform="translate(10,20)" x="5" y="5" width="100" height="50"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(
            !output.contains("transform"),
            "translate should be applied: {output}"
        );
        assert!(output.contains("x=\"15\""), "x should be 5+10=15: {output}");
        assert!(output.contains("y=\"25\""), "y should be 5+20=25: {output}");
    }

    #[test]
    fn applies_translate_to_circle() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><circle transform="translate(10,20)" cx="50" cy="50" r="25"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(
            !output.contains("transform"),
            "translate should be applied: {output}"
        );
        assert!(
            output.contains("cx=\"60\""),
            "cx should be 50+10=60: {output}"
        );
        assert!(
            output.contains("cy=\"70\""),
            "cy should be 50+20=70: {output}"
        );
    }

    #[test]
    fn applies_translate_to_line() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><line transform="translate(10,20)" x1="0" y1="0" x2="100" y2="50"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(
            !output.contains("transform"),
            "translate should be applied: {output}"
        );
        assert!(
            output.contains("x1=\"10\""),
            "x1 should be 0+10=10: {output}"
        );
        assert!(
            output.contains("x2=\"110\""),
            "x2 should be 100+10=110: {output}"
        );
    }

    #[test]
    fn preserves_non_translate_on_rect() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect transform="rotate(45)" x="0" y="0"/></svg>"#;
        let mut doc = parse(input).unwrap();
        ConvertTransform::default().run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("transform"),
            "rotate should not be applied to coords: {output}"
        );
    }

    #[test]
    fn skips_path_for_translate_application() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path transform="translate(10,20)" d="M0 0L10 10"/></svg>"#;
        let mut doc = parse(input).unwrap();
        ConvertTransform::default().run(&mut doc);
        let output = serialize(&doc);
        // Path transform should be simplified but not applied to coordinates
        assert!(
            output.contains("translate(10,20)") || output.contains("transform"),
            "path translate should be kept (not applied to d): {output}"
        );
    }

    #[test]
    fn mixed_transforms_to_matrix() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect transform="translate(10,20) scale(2)"/></svg>"#;
        let mut doc = parse(input).unwrap();
        ConvertTransform::default().run(&mut doc);
        let output = serialize(&doc);
        // translate(10,20) scale(2) = matrix(2,0,0,2,10,20)
        // This is shorter than the two separate transforms
        assert!(
            output.contains("matrix("),
            "mixed transforms should become matrix: {output}"
        );
    }
}
