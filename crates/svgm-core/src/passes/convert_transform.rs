use super::convert_path_data::{parse_path, serialize_path};
use super::{Pass, PassResult};
use crate::ast::{Document, NodeId, NodeKind};

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
                // Process transform, gradientTransform, and patternTransform
                for attr_name in &["transform", "gradientTransform", "patternTransform"] {
                    let Some(pos) = elem
                        .attributes
                        .iter()
                        .position(|a| a.name == *attr_name && a.prefix.is_none())
                    else {
                        continue;
                    };

                    let transform_str = &elem.attributes[pos].value;
                    let Some(matrix) = parse_and_merge_transforms(transform_str) else {
                        continue;
                    };

                    if matrix.is_identity(self.precision) {
                        elem.attributes.remove(pos);
                        changed = true;
                        continue;
                    }

                    // For gradient/pattern transforms, only simplify the string
                    if *attr_name != "transform" {
                        let simplified = matrix.serialize(self.precision);
                        if simplified.len() < elem.attributes[pos].value.len() {
                            elem.attributes[pos].value = simplified;
                            changed = true;
                        }
                        continue;
                    }
                }

                // The rest of transform-specific logic (translate application, etc.)
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
                    elem.attributes.remove(pos);
                    changed = true;
                    continue;
                }

                // Try to apply pure translate directly to element coordinates
                if let Some((tx, ty)) = matrix.as_translate(self.precision) {
                    if apply_translate_to_element(elem, tx, ty, self.precision) {
                        elem.attributes.remove(pos);
                        changed = true;
                        continue;
                    }

                    // Try to apply translate to path d attribute
                    if elem.name == "path" {
                        let d_value = elem
                            .attributes
                            .iter()
                            .find(|a| a.name == "d" && a.prefix.is_none())
                            .map(|a| a.value.clone());
                        if let Some(d_val) = d_value
                            && let Some(new_d) =
                                apply_translate_to_path(&d_val, tx, ty, self.precision)
                        {
                            // Compare: applied d (no transform) vs original d + translate attr
                            // Full attr overhead: ` transform="translate(tx,ty)"`
                            let tx_s = fmt(tx, self.precision);
                            let ty_s = fmt(ty, self.precision);
                            let transform_attr_overhead =
                                " transform=\"translate(,)\"".len() + tx_s.len() + ty_s.len();
                            if new_d.len() <= d_val.len() + transform_attr_overhead {
                                // Applying translate produces shorter or equal output
                                elem.attributes
                                    .iter_mut()
                                    .find(|a| a.name == "d" && a.prefix.is_none())
                                    .unwrap()
                                    .value = new_d;
                                elem.attributes.remove(pos);
                                changed = true;
                                continue;
                            }
                            // Keeping translate is shorter — don't apply
                        }
                    }
                }

                // Simplify the transform string
                let simplified = matrix.serialize(self.precision);
                if simplified.len() < elem.attributes[pos].value.len() {
                    elem.attributes[pos].value = simplified;
                    changed = true;
                }
            }
        }

        // Phase 3: Push transforms from single-child groups to child
        changed |= self.push_transforms_down(doc);

        if changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

/// Attributes with group-level semantics that should prevent transform push-down.
const GROUP_ONLY_ATTRS: &[&str] = &["clip-path", "mask", "filter"];

impl ConvertTransform {
    /// Push transforms from single-child groups down to the child element.
    fn push_transforms_down(&self, doc: &mut Document) -> bool {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            // Check: is this a <g> with a transform and exactly one element child?
            let child_id = {
                let node = doc.node(id);
                let NodeKind::Element(ref elem) = node.kind else {
                    continue;
                };
                if elem.name != "g" {
                    continue;
                }
                let has_transform = elem
                    .attributes
                    .iter()
                    .any(|a| a.name == "transform" && a.prefix.is_none());
                if !has_transform {
                    continue;
                }
                // Skip groups with group-level semantic attrs
                let has_group_only = elem
                    .attributes
                    .iter()
                    .any(|a| GROUP_ONLY_ATTRS.contains(&a.name.as_str()));
                if has_group_only {
                    continue;
                }
                let children: Vec<NodeId> = doc.children(id).collect();
                if children.len() != 1 {
                    continue;
                }
                let child = doc.node(children[0]);
                if matches!(child.kind, NodeKind::Element(_)) {
                    children[0]
                } else {
                    continue;
                }
            };

            // Get group transform matrix
            let group_tf_str = {
                let NodeKind::Element(ref elem) = doc.node(id).kind else {
                    continue;
                };
                match elem
                    .attributes
                    .iter()
                    .find(|a| a.name == "transform" && a.prefix.is_none())
                {
                    Some(a) => a.value.clone(),
                    None => continue,
                }
            };
            let Some(group_matrix) = parse_and_merge_transforms(&group_tf_str) else {
                continue;
            };

            // Get child transform matrix (identity if none)
            let child_matrix = {
                let NodeKind::Element(ref child_elem) = doc.node(child_id).kind else {
                    continue;
                };
                child_elem
                    .attributes
                    .iter()
                    .find(|a| a.name == "transform" && a.prefix.is_none())
                    .and_then(|a| parse_and_merge_transforms(&a.value))
                    .unwrap_or_else(Matrix::identity)
            };

            // Compose and set on child
            let composed = group_matrix.multiply(&child_matrix);
            let composed_str = composed.serialize(self.precision);

            if let NodeKind::Element(ref mut child_elem) = doc.node_mut(child_id).kind {
                if let Some(attr) = child_elem
                    .attributes
                    .iter_mut()
                    .find(|a| a.name == "transform" && a.prefix.is_none())
                {
                    attr.value = composed_str;
                } else {
                    child_elem.attributes.push(crate::ast::Attribute {
                        prefix: None,
                        name: "transform".to_string(),
                        value: composed_str,
                    });
                }
            }

            // Remove transform from group
            if let NodeKind::Element(ref mut elem) = doc.node_mut(id).kind {
                elem.attributes
                    .retain(|a| !(a.name == "transform" && a.prefix.is_none()));
            }

            changed = true;
        }

        changed
    }
}

/// Apply a pure translate to path d attribute coordinates.
fn apply_translate_to_path(d: &str, tx: f64, ty: f64, precision: u32) -> Option<String> {
    let mut commands = parse_path(d)?;
    for cmd in &mut commands {
        match cmd.cmd {
            'M' | 'L' | 'T' => {
                cmd.args[0] += tx;
                cmd.args[1] += ty;
            }
            'C' => {
                for i in (0..6).step_by(2) {
                    cmd.args[i] += tx;
                    cmd.args[i + 1] += ty;
                }
            }
            'S' | 'Q' => {
                for i in (0..4).step_by(2) {
                    cmd.args[i] += tx;
                    cmd.args[i + 1] += ty;
                }
            }
            'H' => {
                cmd.args[0] += tx;
            }
            'V' => {
                cmd.args[0] += ty;
            }
            'A' => {
                // Only translate the endpoint; radii, rotation, flags unchanged
                cmd.args[5] += tx;
                cmd.args[6] += ty;
            }
            // Relative commands and Z: unchanged
            _ => {}
        }
    }
    Some(serialize_path(&commands, precision))
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

        let matrix_str = format!(
            "matrix({},{},{},{},{},{})",
            fmt(self.a, precision),
            fmt(self.b, precision),
            fmt(self.c, precision),
            fmt(self.d, precision),
            fmt(self.e, precision),
            fmt(self.f, precision),
        );

        // Try translate+scale: b=0, c=0 (diagonal matrix with translation)
        if approx_eq(self.b, 0.0, precision) && approx_eq(self.c, 0.0, precision) {
            let t_part = if approx_eq(self.e, 0.0, precision) && approx_eq(self.f, 0.0, precision) {
                String::new()
            } else if approx_eq(self.f, 0.0, precision) {
                format!("translate({})", fmt(self.e, precision))
            } else {
                format!(
                    "translate({} {})",
                    fmt(self.e, precision),
                    fmt(self.f, precision)
                )
            };
            let s_part = if approx_eq(self.a, self.d, precision) {
                format!("scale({})", fmt(self.a, precision))
            } else {
                format!(
                    "scale({} {})",
                    fmt(self.a, precision),
                    fmt(self.d, precision)
                )
            };
            let composed = if t_part.is_empty() {
                s_part
            } else {
                format!("{t_part}{s_part}")
            };
            if composed.len() < matrix_str.len() {
                return composed;
            }
        }

        // Try rotate+scale: a=d, b=-c (rotation matrix, possibly with scale)
        if approx_eq(self.a, self.d, precision)
            && approx_eq(self.b, -self.c, precision)
            && approx_eq(self.e, 0.0, precision)
            && approx_eq(self.f, 0.0, precision)
        {
            let scale = (self.a * self.a + self.b * self.b).sqrt();
            let angle = self.b.atan2(self.a).to_degrees();
            let r_part = format!("rotate({})", fmt(angle, precision));
            let composed = if approx_eq(scale, 1.0, precision) {
                r_part
            } else {
                format!("{}scale({})", r_part, fmt(scale, precision))
            };
            if composed.len() < matrix_str.len() {
                return composed;
            }
        }

        matrix_str
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
    // Normalize European decimal commas: "0,7282" → "0.7282"
    // A comma between digits (with no space before it) is a decimal separator.
    // A comma followed by a space or preceded by a space is an argument separator.
    let s = normalize_european_decimals(s);
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

/// Normalize European decimal commas in transform strings.
/// "translate(0,7282, 0,9693)" → "translate(0.7282, 0.9693)"
/// Detection: if the string contains no dots and has "digit,digit" patterns,
/// it uses European decimal commas. In that case, commas between digits (no surrounding
/// space) are decimal separators; commas followed by space are argument separators.
fn normalize_european_decimals(s: &str) -> String {
    // Only apply if the string uses European format: no dots, has digit,digit patterns,
    // AND uses ", " (comma-space) as argument separators.
    // Normal SVG: "translate(10,20)" — single comma = arg separator
    // European:   "translate(0,7282, 0,9693)" — comma-space = arg, comma-no-space = decimal
    let has_dots = s.contains('.');
    if has_dots {
        return s.to_string();
    }
    let has_comma_space = s.contains(", ");
    if !has_comma_space {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let has_european = bytes
        .windows(3)
        .any(|w| w[0].is_ascii_digit() && w[1] == b',' && w[2].is_ascii_digit());
    if !has_european {
        return s.to_string();
    }

    // Replace digit,digit commas with dots; keep comma+space as arg separators
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b','
            && i > 0
            && bytes[i - 1].is_ascii_digit()
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_digit()
        {
            result.push('.');
        } else {
            result.push(bytes[i] as char);
        }
        i += 1;
    }
    result
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
    fn applies_translate_to_path() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path transform="translate(10,20)" d="M0 0L10 10"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(
            !output.contains("transform"),
            "translate should be applied to path d: {output}"
        );
    }

    #[test]
    fn applies_translate_to_path_with_arc() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path transform="translate(10,20)" d="M0 0A25 25 0 0 1 50 50"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        assert!(
            !output.contains("transform"),
            "translate should be applied to arc path: {output}"
        );
        // Arc radii should be unchanged (25 25)
        assert!(
            output.contains("25 25") || output.contains("25,25"),
            "arc radii should be unchanged: {output}"
        );
    }

    #[test]
    fn preserves_non_translate_on_path() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path transform="rotate(45)" d="M0 0L10 10"/></svg>"#;
        let mut doc = parse(input).unwrap();
        ConvertTransform::default().run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("transform"),
            "rotate should not be applied to path d: {output}"
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

    // ── Phase 3: Transform push-down tests ─────────────────────────────

    #[test]
    fn pushes_transform_to_single_child() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><rect width="50" height="50"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        // Phase 3 pushes transform to child; group no longer has transform
        // (collapse_groups would remove the <g> on next pass, and translate
        // would be applied to rect coords on the next optimizer iteration)
        assert!(
            !output.contains("<g transform") && output.contains("translate(10,20)"),
            "group should lose transform, child should gain it: {output}"
        );
    }

    #[test]
    fn composes_group_and_child_transforms() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><rect transform="translate(5,5)" width="50" height="50"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(
            ConvertTransform::default().run(&mut doc),
            PassResult::Changed
        );
        let output = serialize(&doc);
        // Group translate(10,20) + child translate(5,5) = translate(15,25) on child
        // The child's translate(5,5) was applied to coords in the first traversal (x=5,y=5),
        // then Phase 3 composes the group's translate into the child.
        assert!(
            !output.contains("<g transform"),
            "group should lose transform: {output}"
        );
        assert!(
            output.contains("translate(10,20)") || output.contains("translate(15,25)"),
            "child should have composed or group transform: {output}"
        );
    }

    #[test]
    fn skips_multi_child_group_pushdown() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><rect/><circle r="5"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        ConvertTransform::default().run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("<g") && output.contains("translate(10,20)"),
            "multi-child group should keep transform: {output}"
        );
    }

    #[test]
    fn skips_pushdown_with_clip_path() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)" clip-path="url(#c)"><rect/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        ConvertTransform::default().run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("clip-path") && output.contains("translate(10,20)"),
            "group with clip-path should not push transform: {output}"
        );
    }

    #[test]
    fn pushes_transform_to_path_child() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><path d="M0 0L50 50"/></g></svg>"#;
        let mut doc = parse(input).unwrap();
        ConvertTransform::default().run(&mut doc);
        let output = serialize(&doc);
        // Phase 3 pushes transform to path child; group no longer has it
        assert!(
            !output.contains("<g transform"),
            "group should lose transform: {output}"
        );
        // Path now has the translate (will be applied to d on next iteration)
        assert!(
            output.contains("translate(10,20)"),
            "path should have the pushed transform: {output}"
        );
    }

    #[test]
    fn full_optimize_applies_pushed_transform() {
        // End-to-end: group transform → push to path → apply to d → collapse group
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate(10,20)"><path d="M0 0L50 50"/></g></svg>"#;
        let result = crate::optimize(input).unwrap();
        assert!(
            !result.data.contains("transform"),
            "full optimize should apply transform to path: {}",
            result.data
        );
        assert!(
            !result.data.contains("<g"),
            "full optimize should collapse group: {}",
            result.data
        );
    }
}
