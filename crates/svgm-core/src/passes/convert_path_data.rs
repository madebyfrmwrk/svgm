use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

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
            if let NodeKind::Element(ref mut elem) = node.kind
                && let Some(d_attr) = elem
                    .attributes
                    .iter_mut()
                    .find(|a| a.name == "d" && a.prefix.is_none())
                && let Some(optimized) = optimize_path(&d_attr.value, self.precision)
                && optimized.len() < d_attr.value.len()
            {
                d_attr.value = optimized;
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

    // Phase 1: Normalize to absolute, expand S→C and T→Q for re-analysis
    let commands = normalize_to_absolute(commands);

    // Phase 2: Geometric simplifications
    let commands = simplify_curves(commands, precision);
    let commands = detect_shorthands(commands, precision);
    let commands = remove_redundant(commands, precision);

    // Phase 3: Pick shorter abs/rel per command
    let commands = abs_to_rel(commands);

    // Serialize with optimal formatting
    let result = serialize_path(&commands, precision);
    Some(result)
}

/// Check if two values are equal after rounding to the given precision.
fn approx_eq(a: f64, b: f64, precision: u32) -> bool {
    let factor = 10f64.powi(precision as i32);
    (a * factor).round() == (b * factor).round()
}

/// Convert all commands to absolute and expand S→C, T→Q shorthands.
/// This gives a clean baseline for geometric analysis.
fn normalize_to_absolute(commands: Vec<PathCmd>) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0;
    let mut cy: f64 = 0.0;
    let mut sx: f64 = 0.0;
    let mut sy: f64 = 0.0;
    // Last control point for cubic (used by S/s expansion)
    let mut last_cubic_cp: Option<(f64, f64)> = None;
    // Last control point for quadratic (used by T/t expansion)
    let mut last_quad_cp: Option<(f64, f64)> = None;

    for cmd in commands {
        match cmd.cmd {
            'M' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                sx = cx;
                sy = cy;
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(cmd);
            }
            'm' => {
                cx += cmd.args[0];
                cy += cmd.args[1];
                sx = cx;
                sy = cy;
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(PathCmd {
                    cmd: 'M',
                    args: vec![cx, cy],
                });
            }
            'L' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(cmd);
            }
            'l' => {
                cx += cmd.args[0];
                cy += cmd.args[1];
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(PathCmd {
                    cmd: 'L',
                    args: vec![cx, cy],
                });
            }
            'H' => {
                cx = cmd.args[0];
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(PathCmd {
                    cmd: 'L',
                    args: vec![cx, cy],
                });
            }
            'h' => {
                cx += cmd.args[0];
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(PathCmd {
                    cmd: 'L',
                    args: vec![cx, cy],
                });
            }
            'V' => {
                cy = cmd.args[0];
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(PathCmd {
                    cmd: 'L',
                    args: vec![cx, cy],
                });
            }
            'v' => {
                cy += cmd.args[0];
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(PathCmd {
                    cmd: 'L',
                    args: vec![cx, cy],
                });
            }
            'C' => {
                last_cubic_cp = Some((cmd.args[2], cmd.args[3]));
                last_quad_cp = None;
                cx = cmd.args[4];
                cy = cmd.args[5];
                result.push(cmd);
            }
            'c' => {
                let abs = vec![
                    cx + cmd.args[0],
                    cy + cmd.args[1],
                    cx + cmd.args[2],
                    cy + cmd.args[3],
                    cx + cmd.args[4],
                    cy + cmd.args[5],
                ];
                last_cubic_cp = Some((abs[2], abs[3]));
                last_quad_cp = None;
                cx = abs[4];
                cy = abs[5];
                result.push(PathCmd {
                    cmd: 'C',
                    args: abs,
                });
            }
            'S' => {
                // Expand: first control point is reflection of last cubic cp
                let cp1 = last_cubic_cp
                    .map(|(cpx, cpy)| (2.0 * cx - cpx, 2.0 * cy - cpy))
                    .unwrap_or((cx, cy));
                let abs = vec![
                    cp1.0,
                    cp1.1,
                    cmd.args[0],
                    cmd.args[1],
                    cmd.args[2],
                    cmd.args[3],
                ];
                last_cubic_cp = Some((abs[2], abs[3]));
                last_quad_cp = None;
                cx = abs[4];
                cy = abs[5];
                result.push(PathCmd {
                    cmd: 'C',
                    args: abs,
                });
            }
            's' => {
                let cp1 = last_cubic_cp
                    .map(|(cpx, cpy)| (2.0 * cx - cpx, 2.0 * cy - cpy))
                    .unwrap_or((cx, cy));
                let abs = vec![
                    cp1.0,
                    cp1.1,
                    cx + cmd.args[0],
                    cy + cmd.args[1],
                    cx + cmd.args[2],
                    cy + cmd.args[3],
                ];
                last_cubic_cp = Some((abs[2], abs[3]));
                last_quad_cp = None;
                cx = abs[4];
                cy = abs[5];
                result.push(PathCmd {
                    cmd: 'C',
                    args: abs,
                });
            }
            'Q' => {
                last_quad_cp = Some((cmd.args[0], cmd.args[1]));
                last_cubic_cp = None;
                cx = cmd.args[2];
                cy = cmd.args[3];
                result.push(cmd);
            }
            'q' => {
                let abs = vec![
                    cx + cmd.args[0],
                    cy + cmd.args[1],
                    cx + cmd.args[2],
                    cy + cmd.args[3],
                ];
                last_quad_cp = Some((abs[0], abs[1]));
                last_cubic_cp = None;
                cx = abs[2];
                cy = abs[3];
                result.push(PathCmd {
                    cmd: 'Q',
                    args: abs,
                });
            }
            'T' => {
                let cp = last_quad_cp
                    .map(|(cpx, cpy)| (2.0 * cx - cpx, 2.0 * cy - cpy))
                    .unwrap_or((cx, cy));
                last_quad_cp = Some(cp);
                last_cubic_cp = None;
                cx = cmd.args[0];
                cy = cmd.args[1];
                result.push(PathCmd {
                    cmd: 'Q',
                    args: vec![cp.0, cp.1, cx, cy],
                });
            }
            't' => {
                let cp = last_quad_cp
                    .map(|(cpx, cpy)| (2.0 * cx - cpx, 2.0 * cy - cpy))
                    .unwrap_or((cx, cy));
                last_quad_cp = Some(cp);
                last_cubic_cp = None;
                cx += cmd.args[0];
                cy += cmd.args[1];
                result.push(PathCmd {
                    cmd: 'Q',
                    args: vec![cp.0, cp.1, cx, cy],
                });
            }
            'A' => {
                last_cubic_cp = None;
                last_quad_cp = None;
                cx = cmd.args[5];
                cy = cmd.args[6];
                result.push(cmd);
            }
            'a' => {
                last_cubic_cp = None;
                last_quad_cp = None;
                let abs = vec![
                    cmd.args[0],
                    cmd.args[1],
                    cmd.args[2],
                    cmd.args[3],
                    cmd.args[4],
                    cx + cmd.args[5],
                    cy + cmd.args[6],
                ];
                cx = abs[5];
                cy = abs[6];
                result.push(PathCmd {
                    cmd: 'A',
                    args: abs,
                });
            }
            'Z' | 'z' => {
                cx = sx;
                cy = sy;
                last_cubic_cp = None;
                last_quad_cp = None;
                result.push(PathCmd {
                    cmd: 'Z',
                    args: vec![],
                });
            }
            _ => {
                result.push(cmd);
            }
        }
    }
    result
}

/// Convert degenerate curves to lines where control points are collinear with endpoints.
fn simplify_curves(commands: Vec<PathCmd>, precision: u32) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0;
    let mut cy: f64 = 0.0;

    for cmd in commands {
        match cmd.cmd {
            'C' => {
                let (x1, y1) = (cmd.args[0], cmd.args[1]);
                let (x2, y2) = (cmd.args[2], cmd.args[3]);
                let (x, y) = (cmd.args[4], cmd.args[5]);
                // Check if all control points are collinear with the line from (cx,cy) to (x,y)
                if is_collinear(cx, cy, x1, y1, x, y, precision)
                    && is_collinear(cx, cy, x2, y2, x, y, precision)
                {
                    result.push(PathCmd {
                        cmd: 'L',
                        args: vec![x, y],
                    });
                } else {
                    result.push(cmd);
                }
                cx = x;
                cy = y;
            }
            'Q' => {
                let (cpx, cpy) = (cmd.args[0], cmd.args[1]);
                let (x, y) = (cmd.args[2], cmd.args[3]);
                if is_collinear(cx, cy, cpx, cpy, x, y, precision) {
                    result.push(PathCmd {
                        cmd: 'L',
                        args: vec![x, y],
                    });
                } else {
                    result.push(cmd);
                }
                cx = x;
                cy = y;
            }
            'M' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                result.push(cmd);
            }
            'L' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                result.push(cmd);
            }
            'A' => {
                cx = cmd.args[5];
                cy = cmd.args[6];
                result.push(cmd);
            }
            'Z' => {
                // Z doesn't change cx/cy tracking for simplify purposes
                // (subpath start is tracked separately in abs_to_rel)
                result.push(cmd);
            }
            _ => {
                result.push(cmd);
            }
        }
    }
    result
}

/// Check if point (px, py) is collinear with the line from (x0, y0) to (x1, y1).
fn is_collinear(x0: f64, y0: f64, px: f64, py: f64, x1: f64, y1: f64, precision: u32) -> bool {
    // Cross product of vectors (x1-x0, y1-y0) and (px-x0, py-y0)
    let cross = (x1 - x0) * (py - y0) - (y1 - y0) * (px - x0);
    let tolerance = 0.5 / 10f64.powi(precision as i32);
    cross.abs() < tolerance
}

/// Detect consecutive cubics/quadratics that can use shorthand S/T notation.
fn detect_shorthands(commands: Vec<PathCmd>, precision: u32) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0;
    let mut cy: f64 = 0.0;
    let mut last_cubic_cp2: Option<(f64, f64)> = None;
    let mut last_quad_cp: Option<(f64, f64)> = None;

    for cmd in commands {
        match cmd.cmd {
            'C' => {
                let (x1, y1) = (cmd.args[0], cmd.args[1]);
                let (x2, y2) = (cmd.args[2], cmd.args[3]);
                let (x, y) = (cmd.args[4], cmd.args[5]);

                // Check if first control point matches the reflection of previous cubic cp2
                if let Some((prev_cp2x, prev_cp2y)) = last_cubic_cp2 {
                    let reflected_x = 2.0 * cx - prev_cp2x;
                    let reflected_y = 2.0 * cy - prev_cp2y;
                    if approx_eq(x1, reflected_x, precision)
                        && approx_eq(y1, reflected_y, precision)
                    {
                        result.push(PathCmd {
                            cmd: 'S',
                            args: vec![x2, y2, x, y],
                        });
                        last_cubic_cp2 = Some((x2, y2));
                        last_quad_cp = None;
                        cx = x;
                        cy = y;
                        continue;
                    }
                }

                last_cubic_cp2 = Some((x2, y2));
                last_quad_cp = None;
                cx = x;
                cy = y;
                result.push(cmd);
            }
            'Q' => {
                let (cpx, cpy) = (cmd.args[0], cmd.args[1]);
                let (x, y) = (cmd.args[2], cmd.args[3]);

                // Check if control point matches the reflection of previous quad cp
                if let Some((prev_cpx, prev_cpy)) = last_quad_cp {
                    let reflected_x = 2.0 * cx - prev_cpx;
                    let reflected_y = 2.0 * cy - prev_cpy;
                    if approx_eq(cpx, reflected_x, precision)
                        && approx_eq(cpy, reflected_y, precision)
                    {
                        result.push(PathCmd {
                            cmd: 'T',
                            args: vec![x, y],
                        });
                        last_quad_cp = Some((cpx, cpy));
                        last_cubic_cp2 = None;
                        cx = x;
                        cy = y;
                        continue;
                    }
                }

                last_quad_cp = Some((cpx, cpy));
                last_cubic_cp2 = None;
                cx = x;
                cy = y;
                result.push(cmd);
            }
            'M' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                last_cubic_cp2 = None;
                last_quad_cp = None;
                result.push(cmd);
            }
            'L' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                last_cubic_cp2 = None;
                last_quad_cp = None;
                result.push(cmd);
            }
            'A' => {
                cx = cmd.args[5];
                cy = cmd.args[6];
                last_cubic_cp2 = None;
                last_quad_cp = None;
                result.push(cmd);
            }
            'Z' => {
                last_cubic_cp2 = None;
                last_quad_cp = None;
                result.push(cmd);
            }
            _ => {
                result.push(cmd);
            }
        }
    }
    result
}

/// Remove redundant commands: lines to same point, empty subpaths.
fn remove_redundant(commands: Vec<PathCmd>, precision: u32) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0;
    let mut cy: f64 = 0.0;

    for cmd in &commands {
        match cmd.cmd {
            'L' => {
                let (x, y) = (cmd.args[0], cmd.args[1]);
                // Skip line to same point
                if approx_eq(x, cx, precision) && approx_eq(y, cy, precision) {
                    continue;
                }
                cx = x;
                cy = y;
                result.push(cmd.clone());
            }
            'M' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                result.push(cmd.clone());
            }
            'C' | 'S' | 'Q' | 'T' => {
                let args = &cmd.args;
                let (x, y) = (args[args.len() - 2], args[args.len() - 1]);
                cx = x;
                cy = y;
                result.push(cmd.clone());
            }
            'A' => {
                cx = cmd.args[5];
                cy = cmd.args[6];
                result.push(cmd.clone());
            }
            'Z' => {
                result.push(cmd.clone());
            }
            _ => {
                result.push(cmd.clone());
            }
        }
    }
    result
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
        if let Some(&c) = chars.peek()
            && is_command(c)
        {
            current_cmd = Some(c);
            chars.next();
            skip_ws_comma(&mut chars);
        }

        let mut cmd = current_cmd?;
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
            if let Some(&c) = chars.peek()
                && is_command(c)
            {
                break;
            }

            let mut args = Vec::with_capacity(arg_count);
            for i in 0..arg_count {
                skip_ws_comma(&mut chars);
                // For arc commands, args 3 and 4 are flags (0 or 1)
                if (cmd == 'A' || cmd == 'a') && (i == 3 || i == 4) {
                    if let Some(&c) = chars.peek()
                        && (c == '0' || c == '1')
                    {
                        chars.next();
                        args.push(if c == '1' { 1.0 } else { 0.0 });
                        continue;
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
                    cmd = 'L';
                } else if cmd == 'm' {
                    current_cmd = Some('l');
                    cmd = 'l';
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
                    result.push(PathCmd {
                        cmd: 'm',
                        args: vec![rx, ry],
                    });
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
                        result.push(PathCmd {
                            cmd: 'h',
                            args: vec![rx],
                        });
                    } else {
                        result.push(PathCmd {
                            cmd: 'H',
                            args: vec![x],
                        });
                    }
                } else if rx == 0.0 {
                    let abs_v = format_num(y);
                    let rel_v = format_num(ry);
                    if rel_v.len() <= abs_v.len() {
                        result.push(PathCmd {
                            cmd: 'v',
                            args: vec![ry],
                        });
                    } else {
                        result.push(PathCmd {
                            cmd: 'V',
                            args: vec![y],
                        });
                    }
                } else {
                    let abs_len = format_num(x).len() + format_num(y).len();
                    let rel_len = format_num(rx).len() + format_num(ry).len();
                    if rel_len < abs_len {
                        result.push(PathCmd {
                            cmd: 'l',
                            args: vec![rx, ry],
                        });
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
                    result.push(PathCmd {
                        cmd: 'h',
                        args: vec![cmd.args[0]],
                    });
                } else if cmd.args[0] == 0.0 {
                    result.push(PathCmd {
                        cmd: 'v',
                        args: vec![cmd.args[1]],
                    });
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
                    result.push(PathCmd {
                        cmd: 'h',
                        args: vec![rx],
                    });
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
                    result.push(PathCmd {
                        cmd: 'v',
                        args: vec![ry],
                    });
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
                    args[0] - cx,
                    args[1] - cy,
                    args[2] - cx,
                    args[3] - cy,
                    args[4] - cx,
                    args[5] - cy,
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
                let rx: Vec<f64> = vec![args[0] - cx, args[1] - cy, args[2] - cx, args[3] - cy];
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
                let rx: Vec<f64> = vec![args[0] - cx, args[1] - cy, args[2] - cx, args[3] - cy];
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
                    result.push(PathCmd {
                        cmd: 't',
                        args: vec![rx, ry],
                    });
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
                result.push(PathCmd {
                    cmd: 'z',
                    args: vec![],
                });
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
        } else {
            !(prev_cmd == Some('m') && c == 'l')
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
        // .X can self-separate only when the preceding number already contains
        // a decimal point (e.g. "1.5.4" → 1.5 and 0.4). If the preceding
        // number has no dot, ".4" after "0" would be read as "0.4" (one number).
        let has_prior_dot = out
            .bytes()
            .rev()
            .take_while(|&b| b.is_ascii_digit() || b == b'.')
            .any(|b| b == b'.');
        return !has_prior_dot;
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
    matches!(
        c,
        'M' | 'm'
            | 'L'
            | 'l'
            | 'H'
            | 'h'
            | 'V'
            | 'v'
            | 'C'
            | 'c'
            | 'S'
            | 's'
            | 'Q'
            | 'q'
            | 'T'
            | 't'
            | 'A'
            | 'a'
            | 'Z'
            | 'z'
    )
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
    if let Some(&c) = chars.peek()
        && (c == '-' || c == '+')
    {
        s.push(c);
        chars.next();
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
        assert!(
            result.contains('h')
                || result.contains('H')
                || result.contains('v')
                || result.contains('V'),
            "should use H/V shortcuts: {result}"
        );
    }

    #[test]
    fn handles_cubic_bezier() {
        let d = "M239.248 207.643C233.892 207.643 229.713 205.607 226.714 201.536";
        let result = optimize_path(d, 3).unwrap();
        assert!(
            result.len() <= d.len(),
            "should not grow: original={}, result={}",
            d.len(),
            result.len()
        );
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
        assert!(
            reparsed.is_some(),
            "optimized arc should be re-parseable: {result}"
        );
    }

    // ── Path torture tests ─────────────────────────────────────────────

    #[test]
    fn roundtrip_preserves_command_structure() {
        let paths = [
            "M10 80A25 25 0 0 1 50 80",
            "M0 0L10 0L10 10L0 10Z",
            "M0 0C10 10 20 20 30 30S50 50 60 60",
            "M0 0Q10 10 20 20T40 40",
            "M10 10l5 5L100 100l-3-3",
            "M150 0A150 150 0 1 0 150 300A150 150 0 1 0 150 0Z",
            "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2",
        ];
        for d in paths {
            let optimized = optimize_path(d, 3).unwrap();
            let reparsed = parse_path(&optimized);
            assert!(
                reparsed.is_some(),
                "failed to reparse optimized: {d} -> {optimized}"
            );
        }
    }

    #[test]
    fn optimize_twice_produces_same_output() {
        let paths = [
            "M 100 200 L 300 400",
            "M10 80A25 25 0 0 1 50 80",
            "M239.248 207.643C233.892 207.643 229.713 205.607 226.714 201.536",
            "M0.001 0.001L0.002 0.002",
            "M99999 99999L0 0",
            "M0 0L1 1L2 2L3 3L4 4L5 5",
            "M150 0A150 150 0 1 0 150 300A150 150 0 1 0 150 0Z",
            "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2",
        ];
        for d in paths {
            let first = optimize_path(d, 3).unwrap();
            let second = optimize_path(&first, 3).unwrap();
            assert_eq!(
                first, second,
                "not converged for: {d}\n  first:  {first}\n  second: {second}"
            );
        }
    }

    #[test]
    fn arc_flag_combinations_preserved() {
        for large_arc in [0, 1] {
            for sweep in [0, 1] {
                let d = format!("M10 80A25 25 0 {large_arc} {sweep} 50 80");
                let result = optimize_path(&d, 3).unwrap();
                let cmds = parse_path(&result).unwrap();
                let arc = cmds
                    .iter()
                    .find(|c| c.cmd == 'a' || c.cmd == 'A')
                    .expect(&format!("no arc found in optimized: {d} -> {result}"));
                assert_eq!(
                    arc.args[3] as i32, large_arc,
                    "large-arc-flag mangled for {d} -> {result}"
                );
                assert_eq!(
                    arc.args[4] as i32, sweep,
                    "sweep-flag mangled for {d} -> {result}"
                );
            }
        }
    }

    #[test]
    fn zero_radius_arc_survives() {
        let d = "M10 10A0 0 0 0 1 20 20";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        assert!(
            !cmds.is_empty(),
            "zero-radius arc should not produce empty path"
        );
    }

    #[test]
    fn negative_zero_normalized() {
        assert_eq!(format_num(-0.0), "0", "format_num(-0.0) should be '0'");
        assert_eq!(
            round_and_format(-0.0, 3),
            "0",
            "round_and_format(-0.0, 3) should be '0'"
        );
        assert_eq!(
            round_and_format(-0.0001, 3),
            "0",
            "near-negative-zero should round to '0'"
        );
    }

    #[test]
    fn large_coordinates_roundtrip() {
        let d = "M99999 99999L0 0";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        assert!(cmds.len() >= 2, "should have at least 2 commands: {result}");
    }

    #[test]
    fn tiny_decimals_dont_corrupt() {
        let d = "M0.001 0.001L0.002 0.002";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        assert!(cmds.len() >= 2, "should have at least 2 commands: {result}");
    }

    #[test]
    fn implicit_lineto_after_moveto() {
        let d = "M0 0 10 10 20 20";
        let cmds = parse_path(d).unwrap();
        assert_eq!(
            cmds.len(),
            3,
            "M with extra pairs should produce implicit L commands"
        );
        assert_eq!(cmds[0].cmd, 'M');
        assert_eq!(cmds[1].cmd, 'L');
        assert_eq!(cmds[2].cmd, 'L');
    }

    #[test]
    fn multiple_close_commands() {
        let d = "M0 0L10 10ZZ";
        let result = optimize_path(d, 3);
        assert!(result.is_some(), "double Z should not crash");
    }

    #[test]
    fn full_circle_arc_preserved() {
        let d = "M150 0A150 150 0 1 0 150 300A150 150 0 1 0 150 0Z";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        let arc_count = cmds.iter().filter(|c| c.cmd == 'a' || c.cmd == 'A').count();
        assert_eq!(arc_count, 2, "full circle must keep both arcs: {result}");
    }

    #[test]
    fn accumulated_rounding_stays_accurate() {
        let d =
            "M0 0L0.4 0.4L0.8 0.8L1.2 1.2L1.6 1.6L2.0 2.0L2.4 2.4L2.8 2.8L3.2 3.2L3.6 3.6L4.0 4.0";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        assert!(
            cmds.len() >= 11,
            "should preserve all line segments: {result}"
        );
    }

    #[test]
    fn compact_mixed_path_survives() {
        let d = "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        assert!(cmds.len() >= 4, "should preserve all commands: {result}");
        let second = optimize_path(&result, 3).unwrap();
        assert_eq!(result, second, "should converge: {result} vs {second}");
    }

    // ── New optimization tests ─────────────────────────────────────────

    #[test]
    fn cubic_to_shorthand_s() {
        // First: C 0 50 50 50 50 0 — quarter-circle-like curve from (0,0) to (50,0)
        // Reflection of cp2=(50,50) across endpoint (50,0) = (50, -50)
        // Second: C 50 -50 100 -50 100 0 — cp1 matches reflection → can become S
        let d = "M0 0C0 50 50 50 50 0C50-50 100-50 100 0";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        let has_s = cmds.iter().any(|c| c.cmd == 's' || c.cmd == 'S');
        assert!(has_s, "should detect S shorthand: {result}");
    }

    #[test]
    fn quadratic_to_shorthand_t() {
        // Q 10 20 30 40 followed by Q (2*30-10) (2*40-20) 60 80
        // = Q 10 20 30 40 Q 50 60 60 80
        let d = "M0 0Q10 20 30 40Q50 60 60 80";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        let has_t = cmds.iter().any(|c| c.cmd == 't' || c.cmd == 'T');
        assert!(has_t, "should detect T shorthand: {result}");
    }

    #[test]
    fn degenerate_cubic_becomes_line() {
        // Cubic where all control points are collinear: C on the line from (0,0) to (30,30)
        let d = "M0 0C10 10 20 20 30 30";
        let result = optimize_path(d, 3).unwrap();
        assert!(
            !result.contains('C') && !result.contains('c'),
            "degenerate cubic should become line: {result}"
        );
    }

    #[test]
    fn degenerate_quadratic_becomes_line() {
        let d = "M0 0Q15 15 30 30";
        let result = optimize_path(d, 3).unwrap();
        assert!(
            !result.contains('Q') && !result.contains('q'),
            "degenerate quadratic should become line: {result}"
        );
    }

    #[test]
    fn non_degenerate_cubic_preserved() {
        let d = "M0 0C0 50 50 50 50 0";
        let result = optimize_path(d, 3).unwrap();
        assert!(
            result.contains('c') || result.contains('C'),
            "non-degenerate cubic should stay as curve: {result}"
        );
    }

    #[test]
    fn remove_zero_length_line() {
        let d = "M10 10L10 10L20 20";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        // Should have M + L (the duplicate L10 10 removed)
        assert!(
            cmds.len() <= 2,
            "zero-length line should be removed: {result}"
        );
    }

    #[test]
    fn smooth_cubic_expansion_and_redetection() {
        // Input already has S shorthand; after expansion and re-detection it should still work
        let d = "M0 0C10 20 30 40 50 60S80 90 100 110";
        let result = optimize_path(d, 3).unwrap();
        let cmds = parse_path(&result).unwrap();
        assert!(cmds.len() >= 2, "should preserve commands: {result}");
        // Should converge
        let second = optimize_path(&result, 3).unwrap();
        assert_eq!(result, second, "should converge: {result}");
    }
}
