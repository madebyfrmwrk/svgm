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
pub(crate) struct PathCmd {
    pub(crate) cmd: char,
    pub(crate) args: Vec<f64>,
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
    let commands = convert_cubics_to_arcs(commands, precision);
    let commands = detect_shorthands(commands, precision);
    let commands = remove_redundant(commands, precision);

    // Phase 3: Pick shorter abs/rel per command
    let commands = abs_to_rel(commands, precision);

    // Phase 4: Merge consecutive same-direction lines (h+h→h, v+v→v)
    let commands = merge_consecutive(commands, precision);

    // Phase 5: Simplify arc radii (round equal rx/ry to lower precision when safe)
    let commands = simplify_arc_radii(commands, precision);

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
/// Also converts cubic Beziers to quadratic when losslessly possible.
fn simplify_curves(commands: Vec<PathCmd>, precision: u32) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0;
    let mut cy: f64 = 0.0;
    let error = 10f64.powi(-(precision as i32));

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
                } else if let Some(q_cmd) =
                    try_cubic_to_quad(cx, cy, x1, y1, x2, y2, x, y, error, precision)
                {
                    result.push(q_cmd);
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

/// Try to convert a cubic Bezier (C) to a quadratic (Q) when losslessly possible.
/// A cubic P0→P1→P2→P3 can be a quadratic P0→QP→P3 when:
///   QP_from_P1 = (3*P1 - P0) / 2
///   QP_from_P2 = (3*P2 - P3) / 2
/// and these two are approximately equal.
/// Only converts if the serialized Q form is shorter than the C form.
#[allow(clippy::too_many_arguments)]
fn try_cubic_to_quad(
    p0x: f64,
    p0y: f64,
    cp1x: f64,
    cp1y: f64,
    cp2x: f64,
    cp2y: f64,
    p3x: f64,
    p3y: f64,
    error: f64,
    precision: u32,
) -> Option<PathCmd> {
    // Derive candidate quadratic control points from each cubic control point
    let qx1 = (3.0 * cp1x - p0x) / 4.0;
    let qx2 = (3.0 * cp2x - p3x) / 4.0;
    if (qx1 - qx2).abs() >= error * 2.0 {
        return None;
    }
    let qy1 = (3.0 * cp1y - p0y) / 4.0;
    let qy2 = (3.0 * cp2y - p3y) / 4.0;
    if (qy1 - qy2).abs() >= error * 2.0 {
        return None;
    }

    // Quadratic control point (absolute)
    let qcpx = qx1 + qx2;
    let qcy = qy1 + qy2;

    // Only convert if the serialized Q is shorter than C
    let c_args = [cp1x, cp1y, cp2x, cp2y, p3x, p3y];
    let q_args = [qcpx, qcy, p3x, p3y];

    let c_len: usize = c_args
        .iter()
        .map(|&v| round_and_format(v, precision).len() + 1)
        .sum();
    let q_len: usize = q_args
        .iter()
        .map(|&v| round_and_format(v, precision).len() + 1)
        .sum();

    // Q command is 1 char shorter name savings (C→Q is same length), but 2 fewer args
    if q_len < c_len {
        Some(PathCmd {
            cmd: 'Q',
            args: vec![qcpx, qcy, p3x, p3y],
        })
    } else {
        None
    }
}

/// Check if point (px, py) is collinear with the line from (x0, y0) to (x1, y1).
fn is_collinear(x0: f64, y0: f64, px: f64, py: f64, x1: f64, y1: f64, precision: u32) -> bool {
    // Cross product of vectors (x1-x0, y1-y0) and (px-x0, py-y0)
    let cross = (x1 - x0) * (py - y0) - (y1 - y0) * (px - x0);
    let tolerance = 0.5 / 10f64.powi(precision as i32);
    cross.abs() < tolerance
}

// ── Cubic-to-arc conversion ────────────────────────────────────────────

const ARC_THRESHOLD: f64 = 2.5;
const ARC_TOLERANCE: f64 = 0.5;

/// Convert cubic Bezier curves that approximate circular arcs into SVG arc commands.
fn convert_cubics_to_arcs(commands: Vec<PathCmd>, precision: u32) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0;
    let mut cy: f64 = 0.0;
    let error = 10f64.powi(-(precision as i32));

    for cmd in &commands {
        match cmd.cmd {
            'C' => {
                // Convert to relative for circle fitting
                let rel = [
                    cmd.args[0] - cx,
                    cmd.args[1] - cy,
                    cmd.args[2] - cx,
                    cmd.args[3] - cy,
                    cmd.args[4] - cx,
                    cmd.args[5] - cy,
                ];

                if let Some(circle) = find_circle(&rel, error)
                    && is_arc(&rel, &circle, error)
                {
                    let r = circle.radius;
                    let angle = find_arc_angle(&rel, &circle);
                    let sweep: f64 = if rel[5] * rel[0] - rel[4] * rel[1] > 0.0 {
                        1.0
                    } else {
                        0.0
                    };
                    let large_arc: f64 = if angle > std::f64::consts::PI {
                        1.0
                    } else {
                        0.0
                    };

                    // Only convert if arc form is shorter
                    let arc_cmd = PathCmd {
                        cmd: 'A',
                        args: vec![r, r, 0.0, large_arc, sweep, cmd.args[4], cmd.args[5]],
                    };
                    let arc_len = arc_serialized_len(&arc_cmd, precision);
                    let cubic_len = cubic_serialized_len(cmd, precision);

                    if arc_len < cubic_len {
                        result.push(arc_cmd);
                    } else {
                        result.push(cmd.clone());
                    }
                } else {
                    result.push(cmd.clone());
                }

                cx = cmd.args[4];
                cy = cmd.args[5];
            }
            'M' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                result.push(cmd.clone());
            }
            'L' | 'Q' => {
                let n = cmd.args.len();
                cx = cmd.args[n - 2];
                cy = cmd.args[n - 1];
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

fn cubic_serialized_len(cmd: &PathCmd, precision: u32) -> usize {
    1 + cmd
        .args
        .iter()
        .map(|&v| round_and_format(v, precision).len() + 1)
        .sum::<usize>()
}

fn arc_serialized_len(cmd: &PathCmd, precision: u32) -> usize {
    1 + cmd
        .args
        .iter()
        .map(|&v| round_and_format(v, precision).len() + 1)
        .sum::<usize>()
}

struct Circle {
    center: (f64, f64),
    radius: f64,
}

/// Evaluate a relative cubic Bezier at parameter t.
/// `curve` = [cp1x, cp1y, cp2x, cp2y, endx, endy] relative to start (0,0).
fn cubic_at(curve: &[f64; 6], t: f64) -> (f64, f64) {
    let t1 = 1.0 - t;
    let x = 3.0 * t1 * t1 * t * curve[0] + 3.0 * t1 * t * t * curve[2] + t * t * t * curve[4];
    let y = 3.0 * t1 * t1 * t * curve[1] + 3.0 * t1 * t * t * curve[3] + t * t * t * curve[5];
    (x, y)
}

fn dist(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    ((x1 - x2).powi(2) + (y1 - y2).powi(2)).sqrt()
}

/// Find a circle passing through the start (0,0), midpoint, and endpoint of a cubic.
fn find_circle(curve: &[f64; 6], error: f64) -> Option<Circle> {
    let mid = cubic_at(curve, 0.5);
    let ex = curve[4];
    let ey = curve[5];

    // Perpendicular bisector of chord from (0,0) to mid
    let m1x = mid.0 / 2.0;
    let m1y = mid.1 / 2.0;
    // Direction perpendicular to the chord: rotate (mid.0, mid.1) by 90°
    let p1x = m1x + mid.1;
    let p1y = m1y - mid.0;

    // Perpendicular bisector of chord from mid to end
    let m2x = (mid.0 + ex) / 2.0;
    let m2y = (mid.1 + ey) / 2.0;
    let dx = ex - mid.0;
    let dy = ey - mid.1;
    let p2x = m2x + dy;
    let p2y = m2y - dx;

    // Intersect the two perpendicular bisector lines
    let center = line_intersection(m1x, m1y, p1x, p1y, m2x, m2y, p2x, p2y)?;
    let radius = dist(center.0, center.1, 0.0, 0.0);

    if radius < error {
        return None;
    }

    let tolerance = (ARC_THRESHOLD * error).min(ARC_TOLERANCE * radius / 100.0);

    // Validate: check points at t=1/4 and t=3/4
    for &t in &[0.25, 0.75] {
        let pt = cubic_at(curve, t);
        let d = dist(pt.0, pt.1, center.0, center.1);
        if (d - radius).abs() > tolerance {
            return None;
        }
    }

    Some(Circle { center, radius })
}

/// Check if a cubic Bezier fits a circular arc by testing 5 evenly-spaced points.
fn is_arc(curve: &[f64; 6], circle: &Circle, error: f64) -> bool {
    let tolerance = (ARC_THRESHOLD * error).min(ARC_TOLERANCE * circle.radius / 100.0);

    for &t in &[0.0, 0.25, 0.5, 0.75, 1.0] {
        let pt = cubic_at(curve, t);
        let d = dist(pt.0, pt.1, circle.center.0, circle.center.1);
        if (d - circle.radius).abs() > tolerance {
            return false;
        }
    }
    true
}

/// Compute the central angle swept by the arc.
fn find_arc_angle(curve: &[f64; 6], circle: &Circle) -> f64 {
    // Vectors from center to start (0,0) and end
    let x1 = -circle.center.0;
    let y1 = -circle.center.1;
    let x2 = curve[4] - circle.center.0;
    let y2 = curve[5] - circle.center.1;

    let dot = x1 * x2 + y1 * y2;
    let mag = ((x1 * x1 + y1 * y1) * (x2 * x2 + y2 * y2)).sqrt();
    if mag < 1e-10 {
        return 0.0;
    }
    (dot / mag).clamp(-1.0, 1.0).acos()
}

/// Find intersection of two lines: (x1,y1)→(x2,y2) and (x3,y3)→(x4,y4).
#[allow(clippy::too_many_arguments)]
fn line_intersection(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
    x4: f64,
    y4: f64,
) -> Option<(f64, f64)> {
    let a1 = y1 - y2;
    let b1 = x2 - x1;
    let c1 = x1 * y2 - x2 * y1;

    let a2 = y3 - y4;
    let b2 = x4 - x3;
    let c2 = x3 * y4 - x4 * y3;

    let denom = a1 * b2 - a2 * b1;
    if denom.abs() < 1e-10 {
        return None; // Parallel
    }

    let x = (b1 * c2 - b2 * c1) / denom;
    let y = (a2 * c1 - a1 * c2) / denom;
    Some((x, y))
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
                let can_shorthand = if let Some((prev_cp2x, prev_cp2y)) = last_cubic_cp2 {
                    let reflected_x = 2.0 * cx - prev_cp2x;
                    let reflected_y = 2.0 * cy - prev_cp2y;
                    approx_eq(x1, reflected_x, precision) && approx_eq(y1, reflected_y, precision)
                } else {
                    // No previous cubic: reflection of implicit CP = current point
                    approx_eq(x1, cx, precision) && approx_eq(y1, cy, precision)
                };

                if can_shorthand {
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
                let can_shorthand_q = if let Some((prev_cpx, prev_cpy)) = last_quad_cp {
                    let reflected_x = 2.0 * cx - prev_cpx;
                    let reflected_y = 2.0 * cy - prev_cpy;
                    approx_eq(cpx, reflected_x, precision) && approx_eq(cpy, reflected_y, precision)
                } else {
                    // No previous quad: reflection of implicit CP = current point
                    approx_eq(cpx, cx, precision) && approx_eq(cpy, cy, precision)
                };

                if can_shorthand_q {
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
    let mut sx: f64 = 0.0;
    let mut sy: f64 = 0.0;

    for (i, cmd) in commands.iter().enumerate() {
        match cmd.cmd {
            'L' => {
                let (x, y) = (cmd.args[0], cmd.args[1]);
                // Skip line to same point
                if approx_eq(x, cx, precision) && approx_eq(y, cy, precision) {
                    continue;
                }
                // Skip line to subpath start right before Z (z closes automatically)
                if approx_eq(x, sx, precision)
                    && approx_eq(y, sy, precision)
                    && next_is_close(&commands, i + 1)
                {
                    continue;
                }
                cx = x;
                cy = y;
                result.push(cmd.clone());
            }
            'M' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                sx = cx;
                sy = cy;
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
                cx = sx;
                cy = sy;
                result.push(cmd.clone());
            }
            _ => {
                result.push(cmd.clone());
            }
        }
    }
    result
}

/// Check if the next non-redundant command is a Z/z closepath.
fn next_is_close(commands: &[PathCmd], from: usize) -> bool {
    commands
        .get(from)
        .is_some_and(|c| c.cmd == 'Z' || c.cmd == 'z')
}

/// Parse a path `d` string into a list of commands.
pub(crate) fn parse_path(d: &str) -> Option<Vec<PathCmd>> {
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
fn abs_to_rel(commands: Vec<PathCmd>, precision: u32) -> Vec<PathCmd> {
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
                let abs_str =
                    round_and_format(x, precision).len() + round_and_format(y, precision).len();
                let rel_str =
                    round_and_format(rx, precision).len() + round_and_format(ry, precision).len();

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
                    let abs_h = round_and_format(x, precision);
                    let rel_h = round_and_format(rx, precision);
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
                    let abs_v = round_and_format(y, precision);
                    let rel_v = round_and_format(ry, precision);
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
                    let abs_len =
                        round_and_format(x, precision).len() + round_and_format(y, precision).len();
                    let rel_len = round_and_format(rx, precision).len()
                        + round_and_format(ry, precision).len();
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
                let abs_len: usize = args
                    .iter()
                    .map(|n| round_and_format(*n, precision).len())
                    .sum();
                let rel_len: usize = rx
                    .iter()
                    .map(|n| round_and_format(*n, precision).len())
                    .sum();
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
                let abs_len: usize = args
                    .iter()
                    .map(|n| round_and_format(*n, precision).len())
                    .sum();
                let rel_len: usize = rx
                    .iter()
                    .map(|n| round_and_format(*n, precision).len())
                    .sum();
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
                let abs_len: usize = args
                    .iter()
                    .map(|n| round_and_format(*n, precision).len())
                    .sum();
                let rel_len: usize = rx
                    .iter()
                    .map(|n| round_and_format(*n, precision).len())
                    .sum();
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
                let abs_len =
                    round_and_format(x, precision).len() + round_and_format(y, precision).len();
                let rel_len =
                    round_and_format(rx, precision).len() + round_and_format(ry, precision).len();
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
                let abs_endpoint = round_and_format(args[5], precision).len()
                    + round_and_format(args[6], precision).len();
                let rel_endpoint =
                    round_and_format(rx, precision).len() + round_and_format(ry, precision).len();
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

/// Merge consecutive h+h, v+v, and same-direction l+l commands into single commands.
/// For arc commands where rx == ry (circular arcs), try rounding the radius
/// to lower precision when the arc shape (sagitta) doesn't change significantly.
fn simplify_arc_radii(commands: Vec<PathCmd>, precision: u32) -> Vec<PathCmd> {
    if precision == 0 {
        return commands;
    }
    let error = 10f64.powi(-(precision as i32));

    commands
        .into_iter()
        .map(|mut cmd| {
            if matches!(cmd.cmd, 'A' | 'a') && cmd.args.len() >= 7 {
                let rx = cmd.args[0];
                let ry = cmd.args[1];
                let large_arc = cmd.args[3];

                // Only simplify circular arcs (rx ≈ ry), skip large-arc (sagitta undefined)
                if (rx - ry).abs() < error && large_arc != 1.0 && rx > 0.0 {
                    let dx = cmd.args[5];
                    let dy = cmd.args[6];
                    let chord = (dx * dx + dy * dy).sqrt();

                    // Skip if chord > diameter (invalid arc)
                    if chord <= rx * 2.0 {
                        let sagitta = rx - (rx * rx - 0.25 * chord * chord).max(0.0).sqrt();

                        // Try progressively lower precision
                        for p in (0..precision).rev() {
                            let factor = 10f64.powi(p as i32);
                            let rounded = (rx * factor).round() / factor;
                            if rounded <= 0.0 || chord > rounded * 2.0 {
                                break;
                            }
                            let new_sagitta = rounded
                                - (rounded * rounded - 0.25 * chord * chord).max(0.0).sqrt();
                            if (sagitta - new_sagitta).abs() < error {
                                cmd.args[0] = rounded;
                                cmd.args[1] = rounded;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            cmd
        })
        .collect()
}

fn merge_consecutive(commands: Vec<PathCmd>, precision: u32) -> Vec<PathCmd> {
    if commands.is_empty() {
        return commands;
    }

    let mut result: Vec<PathCmd> = Vec::with_capacity(commands.len());

    for cmd in commands {
        if let Some(prev) = result.last_mut() {
            // Merge consecutive h commands
            if cmd.cmd == 'h' && prev.cmd == 'h' {
                prev.args[0] += cmd.args[0];
                continue;
            }
            if cmd.cmd == 'H' && prev.cmd == 'H' {
                // Absolute H: just keep the last one (it overwrites)
                prev.args[0] = cmd.args[0];
                continue;
            }
            // Merge consecutive v commands
            if cmd.cmd == 'v' && prev.cmd == 'v' {
                prev.args[0] += cmd.args[0];
                continue;
            }
            if cmd.cmd == 'V' && prev.cmd == 'V' {
                prev.args[0] = cmd.args[0];
                continue;
            }
            // Note: do NOT merge consecutive l commands — they represent distinct
            // line segments to different waypoints. Merging skips waypoints.
        }
        result.push(cmd);
    }

    // Remove zero-length h/v that may result from merging
    result.retain(|cmd| {
        if (cmd.cmd == 'h' || cmd.cmd == 'v') && approx_eq(cmd.args[0], 0.0, precision) {
            return false;
        }
        true
    });

    result
}

/// Serialize path commands into an optimized string.
pub(crate) fn serialize_path(commands: &[PathCmd], precision: u32) -> String {
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
                // After command letter — never need separator (letter itself is the delimiter)
                false
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

    // "strongRound": try precision-1 and use it if error is acceptable
    if precision > 0 {
        let error = 10f64.powi(-(precision as i32));
        let factor_low = 10f64.powi((precision - 1) as i32);
        let rounded_low = (val * factor_low).round() / factor_low;
        if (rounded - rounded_low).abs() < error {
            return format_num(rounded_low);
        }
    }

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

    #[test]
    fn cubic_to_quadratic_conversion() {
        // A cubic where both CPs derive the same quadratic CP:
        // P0=(0,0), CP1=(20,0), CP2=(40,60), P3=(60,60) is NOT convertible (different QCPs)
        // A cubic that IS a quadratic: P0=(0,0), QP=(30,30), P3=(60,0)
        // Equivalent cubic: CP1 = P0 + 2/3*(QP-P0) = (20,20), CP2 = P3 + 2/3*(QP-P3) = (40,20)
        let d = "M0 0C20 20 40 20 60 0";
        let result = optimize_path(d, 3).unwrap();
        assert!(
            result.contains('q') || result.contains('Q'),
            "should convert cubic to quadratic: {result}"
        );
        assert!(
            !result.contains('c') && !result.contains('C'),
            "should not have cubic anymore: {result}"
        );
    }

    #[test]
    fn cubic_not_converted_when_not_quadratic() {
        // A cubic that cannot be represented as a quadratic
        let d = "M0 0C10 50 50 50 60 0";
        let result = optimize_path(d, 3).unwrap();
        assert!(
            result.contains('c') || result.contains('C'),
            "should keep cubic: {result}"
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
        // Collinear line segments may be merged for shorter output,
        // but the endpoint must be preserved.
        let d =
            "M0 0L0.4 0.4L0.8 0.8L1.2 1.2L1.6 1.6L2.0 2.0L2.4 2.4L2.8 2.8L3.2 3.2L3.6 3.6L4.0 4.0";
        let result = optimize_path(d, 3).unwrap();
        // Must be valid and reparseable
        let cmds = parse_path(&result).unwrap();
        assert!(!cmds.is_empty(), "should produce valid path: {result}");
        // Endpoint must be preserved (4,4)
        assert!(
            result.contains('4') && result.len() < d.len(),
            "should be shorter than original: {result}"
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
