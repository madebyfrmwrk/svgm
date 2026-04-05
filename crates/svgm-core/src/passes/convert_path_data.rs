use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

pub struct ConvertPathData {
    pub precision: u32,
}

impl Default for ConvertPathData {
    fn default() -> Self {
        Self { precision: 3 }
    }
}

impl Pass for ConvertPathData {
    fn name(&self) -> &'static str {
        "convertPathData"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                if let Some(d_attr) = elem.attributes.iter_mut().find(|a| a.name == "d" && a.prefix.is_none()) {
                    if let Some(optimized) = optimize_path(&d_attr.value, self.precision) {
                        if optimized.len() < d_attr.value.len() {
                            d_attr.value = optimized;
                            changed = true;
                        }
                    }
                }
            }
        }

        if changed { PassResult::Changed } else { PassResult::Unchanged }
    }
}

/// A parsed path command with its coordinates.
#[derive(Debug, Clone)]
struct PathCmd {
    cmd: char,
    args: Vec<f64>,
}

/// Optimize a path `d` attribute string.
fn optimize_path(d: &str, precision: u32) -> Option<String> {
    let commands = parse_path(d)?;
    if commands.is_empty() {
        return None;
    }

    // Convert absolute commands to relative where shorter
    let commands = abs_to_rel(commands);

    // Serialize with optimal formatting
    let result = serialize_path(&commands, precision);
    Some(result)
}

/// Parse a path `d` string into a list of commands.
fn parse_path(d: &str) -> Option<Vec<PathCmd>> {
    let mut commands = Vec::new();
    let mut chars = d.chars().peekable();
    let mut current_cmd: Option<char> = None;

    while chars.peek().is_some() {
        // Skip whitespace and commas
        skip_ws_comma(&mut chars);

        if chars.peek().is_none() {
            break;
        }

        // Check if next char is a command letter
        if let Some(&c) = chars.peek() {
            if is_command(c) {
                current_cmd = Some(c);
                chars.next();
                skip_ws_comma(&mut chars);
            }
        }

        let cmd = current_cmd?;
        let arg_count = args_for_command(cmd);

        if arg_count == 0 {
            commands.push(PathCmd { cmd, args: vec![] });
            // Z doesn't change implicit next command
            if cmd == 'Z' || cmd == 'z' {
                current_cmd = None;
            }
            continue;
        }

        // Read args in groups
        loop {
            skip_ws_comma(&mut chars);
            if chars.peek().is_none() {
                break;
            }

            // Check if next is a new command
            if let Some(&c) = chars.peek() {
                if is_command(c) {
                    break;
                }
            }

            let mut args = Vec::with_capacity(arg_count);
            for i in 0..arg_count {
                skip_ws_comma(&mut chars);
                // For arc commands, args 3 and 4 are flags (0 or 1)
                if (cmd == 'A' || cmd == 'a') && (i == 3 || i == 4) {
                    if let Some(&c) = chars.peek() {
                        if c == '0' || c == '1' {
                            chars.next();
                            args.push(if c == '1' { 1.0 } else { 0.0 });
                            continue;
                        }
                    }
                    return None; // invalid arc flag
                }
                if let Some(n) = parse_number(&mut chars) {
                    args.push(n);
                } else if i == 0 {
                    // No more arguments for this command — break out
                    break;
                } else {
                    return None; // incomplete command
                }
            }

            if args.len() == arg_count {
                commands.push(PathCmd { cmd, args });

                // Implicit repeat: M becomes L, m becomes l
                if cmd == 'M' {
                    current_cmd = Some('L');
                } else if cmd == 'm' {
                    current_cmd = Some('l');
                }
            } else {
                break;
            }
        }
    }

    Some(commands)
}

/// Convert absolute commands to relative where the relative form is shorter.
fn abs_to_rel(commands: Vec<PathCmd>) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0; // current x
    let mut cy: f64 = 0.0; // current y
    let mut sx: f64 = 0.0; // subpath start x
    let mut sy: f64 = 0.0; // subpath start y

    for cmd in commands {
        match cmd.cmd {
            'M' => {
                let x = cmd.args[0];
                let y = cmd.args[1];
                let rx = x - cx;
                let ry = y - cy;

                // Use relative if shorter
                let abs_str = format_num(x).len() + format_num(y).len();
                let rel_str = format_num(rx).len() + format_num(ry).len();

                if rel_str < abs_str && !(cx == 0.0 && cy == 0.0) {
                    result.push(PathCmd { cmd: 'm', args: vec![rx, ry] });
                } else {
                    result.push(cmd.clone());
                }
                cx = x;
                cy = y;
                sx = x;
                sy = y;
            }
            'm' => {
                cx += cmd.args[0];
                cy += cmd.args[1];
                sx = cx;
                sy = cy;
                result.push(cmd);
            }
            'L' => {
                let x = cmd.args[0];
                let y = cmd.args[1];
                let rx = x - cx;
                let ry = y - cy;

                // Check for H/V shortcuts
                if ry == 0.0 {
                    let abs_h = format_num(x);
                    let rel_h = format_num(rx);
                    if rel_h.len() <= abs_h.len() {
                        result.push(PathCmd { cmd: 'h', args: vec![rx] });
                    } else {
                        result.push(PathCmd { cmd: 'H', args: vec![x] });
                    }
                } else if rx == 0.0 {
                    let abs_v = format_num(y);
                    let rel_v = format_num(ry);
                    if rel_v.len() <= abs_v.len() {
                        result.push(PathCmd { cmd: 'v', args: vec![ry] });
                    } else {
                        result.push(PathCmd { cmd: 'V', args: vec![y] });
                    }
                } else {
                    let abs_len = format_num(x).len() + format_num(y).len();
                    let rel_len = format_num(rx).len() + format_num(ry).len();
                    if rel_len < abs_len {
                        result.push(PathCmd { cmd: 'l', args: vec![rx, ry] });
                    } else {
                        result.push(cmd.clone());
                    }
                }
                cx = x;
                cy = y;
            }
            'l' => {
                cx += cmd.args[0];
                cy += cmd.args[1];
                // Convert to h/v if one component is 0
                if cmd.args[1] == 0.0 {
                    result.push(PathCmd { cmd: 'h', args: vec![cmd.args[0]] });
                } else if cmd.args[0] == 0.0 {
                    result.push(PathCmd { cmd: 'v', args: vec![cmd.args[1]] });
                } else {
                    result.push(cmd);
                }
            }
            'H' => {
                let x = cmd.args[0];
                let rx = x - cx;
                let abs_s = format_num(x);
                let rel_s = format_num(rx);
                if rel_s.len() < abs_s.len() {
                    result.push(PathCmd { cmd: 'h', args: vec![rx] });
                } else {
                    result.push(cmd.clone());
                }
                cx = x;
            }
            'h' => {
                cx += cmd.args[0];
                result.push(cmd);
            }
            'V' => {
                let y = cmd.args[0];
                let ry = y - cy;
                let abs_s = format_num(y);
                let rel_s = format_num(ry);
                if rel_s.len() < abs_s.len() {
                    result.push(PathCmd { cmd: 'v', args: vec![ry] });
                } else {
                    result.push(cmd.clone());
                }
                cy = y;
            }
            'v' => {
                cy += cmd.args[0];
                result.push(cmd);
            }
            'C' => {
                let args = &cmd.args;
                let rx: Vec<f64> = vec![
                    args[0] - cx, args[1] - cy,
                    args[2] - cx, args[3] - cy,
                    args[4] - cx, args[5] - cy,
                ];
                let abs_len: usize = args.iter().map(|n| format_num(*n).len()).sum();
                let rel_len: usize = rx.iter().map(|n| format_num(*n).len()).sum();
                if rel_len < abs_len {
                    result.push(PathCmd { cmd: 'c', args: rx });
                } else {
                    result.push(cmd.clone());
                }
                cx = args[4];
                cy = args[5];
            }
            'c' => {
                cx += cmd.args[4];
                cy += cmd.args[5];
                result.push(cmd);
            }
            'S' => {
                let args = &cmd.args;
                let rx: Vec<f64> = vec![
                    args[0] - cx, args[1] - cy,
                    args[2] - cx, args[3] - cy,
                ];
                let abs_len: usize = args.iter().map(|n| format_num(*n).len()).sum();
                let rel_len: usize = rx.iter().map(|n| format_num(*n).len()).sum();
                if rel_len < abs_len {
                    result.push(PathCmd { cmd: 's', args: rx });
                } else {
                    result.push(cmd.clone());
                }
                cx = args[2];
                cy = args[3];
            }
            's' => {
                cx += cmd.args[2];
                cy += cmd.args[3];
                result.push(cmd);
            }
            'Q' => {
                let args = &cmd.args;
                let rx: Vec<f64> = vec![
                    args[0] - cx, args[1] - cy,
                    args[2] - cx, args[3] - cy,
                ];
                let abs_len: usize = args.iter().map(|n| format_num(*n).len()).sum();
                let rel_len: usize = rx.iter().map(|n| format_num(*n).len()).sum();
                if rel_len < abs_len {
                    result.push(PathCmd { cmd: 'q', args: rx });
                } else {
                    result.push(cmd.clone());
                }
                cx = args[2];
                cy = args[3];
            }
            'q' => {
                cx += cmd.args[2];
                cy += cmd.args[3];
                result.push(cmd);
            }
            'T' => {
                let x = cmd.args[0];
                let y = cmd.args[1];
                let rx = x - cx;
                let ry = y - cy;
                let abs_len = format_num(x).len() + format_num(y).len();
                let rel_len = format_num(rx).len() + format_num(ry).len();
                if rel_len < abs_len {
                    result.push(PathCmd { cmd: 't', args: vec![rx, ry] });
                } else {
                    result.push(cmd.clone());
                }
                cx = x;
                cy = y;
            }
            't' => {
                cx += cmd.args[0];
                cy += cmd.args[1];
                result.push(cmd);
            }
            'A' => {
                let args = &cmd.args;
                // Only the endpoint (args[5], args[6]) is relative-able
                let rx = args[5] - cx;
                let ry = args[6] - cy;
                let abs_endpoint = format_num(args[5]).len() + format_num(args[6]).len();
                let rel_endpoint = format_num(rx).len() + format_num(ry).len();
                if rel_endpoint < abs_endpoint {
                    result.push(PathCmd {
                        cmd: 'a',
                        args: vec![args[0], args[1], args[2], args[3], args[4], rx, ry],
                    });
                } else {
                    result.push(cmd.clone());
                }
                cx = args[5];
                cy = args[6];
            }
            'a' => {
                cx += cmd.args[5];
                cy += cmd.args[6];
                result.push(cmd);
            }
            'Z' | 'z' => {
                cx = sx;
                cy = sy;
                result.push(PathCmd { cmd: 'z', args: vec![] });
            }
            _ => {
                result.push(cmd);
            }
        }
    }

    result
}

/// Serialize path commands into an optimized string.
fn serialize_path(commands: &[PathCmd], precision: u32) -> String {
    let mut out = String::new();
    let mut prev_cmd: Option<char> = None;

    for cmd in commands {
        let c = cmd.cmd;

        // Omit repeated command letters (implicit repeat)
        let emit_cmd = if prev_cmd == Some(c) {
            // Same command — can omit the letter
            false
        } else if prev_cmd == Some('M') && c == 'L' {
            false
        } else if prev_cmd == Some('m') && c == 'l' {
            false
        } else {
            true
        };

        if emit_cmd {
            // No space needed before Z
            if c == 'z' || c == 'Z' {
                out.push(c);
                prev_cmd = Some(c);
                continue;
            }
            out.push(c);
        }

        // Write args with minimal separators
        for (i, &val) in cmd.args.iter().enumerate() {
            let s = round_and_format(val, precision);
            let need_separator = if i == 0 && emit_cmd {
                // After command letter — need separator only if number doesn't start with - or .
                !s.starts_with('-') && !s.starts_with('.')
            } else if i == 0 && !emit_cmd {
                // Implicit repeat — need separator if previous ended with digit and this starts with digit or .
                needs_separator_before(&out, &s)
            } else {
                // Between args
                needs_separator_before(&out, &s)
            };

            if need_separator {
                // Use space only if comma/nothing won't work
                out.push(' ');
            }
            out.push_str(&s);
        }

        prev_cmd = Some(c);
    }

    out
}

/// Check if we need a separator between the end of `out` and the start of `next`.
fn needs_separator_before(out: &str, next: &str) -> bool {
    if out.is_empty() {
        return false;
    }
    let last = out.as_bytes()[out.len() - 1];
    let first = next.as_bytes()[0];

    // If next starts with '-' or '.', it can self-separate in many cases
    if first == b'-' {
        // Minus is a valid separator if previous char is a digit
        return !last.is_ascii_digit() && last != b' ' && last != b',';
    }
    if first == b'.' {
        // Dot can self-separate only if previous number has no dot
        // For safety, add separator if last char is a digit
        return false; // Allow .5 to follow directly
    }

    // Otherwise need separator if last is digit and first is digit
    last.is_ascii_digit() || last == b'.'
}

fn round_and_format(val: f64, precision: u32) -> String {
    let factor = 10f64.powi(precision as i32);
    let rounded = (val * factor).round() / factor;
    format_num(rounded)
}

fn format_num(val: f64) -> String {
    if val == 0.0 {
        return "0".to_string();
    }

    // Format with enough decimals then strip trailing zeros
    let s = format!("{:.10}", val);
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

fn is_command(c: char) -> bool {
    matches!(c, 'M' | 'm' | 'L' | 'l' | 'H' | 'h' | 'V' | 'v'
        | 'C' | 'c' | 'S' | 's' | 'Q' | 'q' | 'T' | 't'
        | 'A' | 'a' | 'Z' | 'z')
}

fn args_for_command(cmd: char) -> usize {
    match cmd {
        'M' | 'm' | 'L' | 'l' | 'T' | 't' => 2,
        'H' | 'h' | 'V' | 'v' => 1,
        'C' | 'c' => 6,
        'S' | 's' | 'Q' | 'q' => 4,
        'A' | 'a' => 7,
        'Z' | 'z' => 0,
        _ => 0,
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

fn parse_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<f64> {
    skip_ws_comma(chars);
    let mut s = String::new();

    // Optional sign
    if let Some(&c) = chars.peek() {
        if c == '-' || c == '+' {
            s.push(c);
            chars.next();
        }
    }

    // Integer part
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

    // Decimal part
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

    // Exponent
    if let Some(&c) = chars.peek() {
        if c == 'e' || c == 'E' {
            s.push(c);
            chars.next();
            if let Some(&c) = chars.peek() {
                if c == '+' || c == '-' {
                    s.push(c);
                    chars.next();
                }
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
    }

    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse as parse_svg;
    use crate::serializer::serialize;

    #[test]
    fn optimizes_simple_path() {
        let d = "M 100 200 L 300 400";
        let result = optimize_path(d, 3).unwrap();
        assert!(result.len() <= d.len(), "should be shorter: {result}");
    }

    #[test]
    fn converts_l_to_h_v() {
        let d = "M0 0L100 0L100 200";
        let result = optimize_path(d, 3).unwrap();
        assert!(result.contains('h') || result.contains('H') || result.contains('v') || result.contains('V'),
            "should use H/V shortcuts: {result}");
    }

    #[test]
    fn handles_cubic_bezier() {
        let d = "M239.248 207.643C233.892 207.643 229.713 205.607 226.714 201.536";
        let result = optimize_path(d, 3).unwrap();
        assert!(result.len() <= d.len(), "should not grow: original={}, result={}", d.len(), result.len());
    }

    #[test]
    fn handles_close_path() {
        let d = "M0 0L10 0L10 10Z";
        let result = optimize_path(d, 3).unwrap();
        assert!(result.contains('z'), "should have close: {result}");
    }

    #[test]
    fn strips_leading_zeros() {
        let d = "M0.5 0.5L0.75 0.25";
        let result = optimize_path(d, 3).unwrap();
        assert!(result.contains(".5"), "should strip leading zero: {result}");
        assert!(!result.contains("0.5") || result.len() < d.len());
    }

    #[test]
    fn pass_optimizes_path_elements() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M 100 200 L 300 200 L 300 400"/></svg>"#;
        let mut doc = parse_svg(input).unwrap();
        let pass = ConvertPathData::default();
        assert_eq!(pass.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.len() < input.len(), "should produce shorter output");
    }

    #[test]
    fn preserves_arcs() {
        let d = "M10 80A25 25 0 0 1 50 80";
        let result = optimize_path(d, 3).unwrap();
        // Should not corrupt arc commands
        let reparsed = parse_path(&result);
        assert!(reparsed.is_some(), "optimized arc should be re-parseable: {result}");
    }
}
