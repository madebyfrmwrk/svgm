use super::{
    Computed, Pass, PassResult, computed_style_with, representable, strong_round,
    stylesheet_declarations,
};
use crate::ast::{Document, NodeId, NodeKind};

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

        // A stylesheet rule could attach markers to any path, so when one
        // declares a marker property every path is treated as marked
        // (SVGO's `hasMarkerMid` is derived from the computed style).
        let markers_in_css = stylesheet_declarations(doc)
            .iter()
            .any(|(prop, _)| is_marker_prop(prop));

        for id in ids {
            let has_d = matches!(&doc.node(id).kind, NodeKind::Element(e) if e.attr("d").is_some());
            if !has_d {
                continue;
            }
            let has_markers = markers_in_css || has_marker_style(doc, id);
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind
                && let Some(d_attr) = elem
                    .attributes
                    .iter_mut()
                    .find(|a| a.name == "d" && a.prefix.is_none())
                && let Some(optimized) =
                    optimize_path_with(&d_attr.value, self.precision, has_markers)
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

fn is_marker_prop(prop: &str) -> bool {
    matches!(
        prop,
        "marker" | "marker-start" | "marker-mid" | "marker-end"
    )
}

/// True when `marker-start` or `marker-mid` (or the `marker` shorthand) is
/// set on the element, its `style` attribute, or inherited from an ancestor.
/// Such a path must keep every moveto, since each vertex places a marker.
fn has_marker_style(doc: &Document, id: NodeId) -> bool {
    ["marker-start", "marker-mid", "marker"].iter().any(|prop| {
        match computed_style_with(doc, id, prop, false) {
            Computed::Static(v) => {
                let v = v.trim();
                !v.is_empty() && v != "none"
            }
            Computed::Dynamic => true,
            Computed::Unset => false,
        }
    })
}

/// Optimize a path `d` attribute string (no markers assumed).
#[cfg(test)]
fn optimize_path(d: &str, precision: u32) -> Option<String> {
    optimize_path_with(d, precision, false)
}

/// Optimize a path `d` attribute string. `has_markers` keeps every moveto
/// (a redundant `M` immediately followed by another `M` is otherwise
/// dropped, which would move a `marker-start`/`marker-mid` glyph).
fn optimize_path_with(d: &str, precision: u32, has_markers: bool) -> Option<String> {
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
    let commands = remove_redundant(commands, precision, has_markers);

    // Phase 3: Merge runs of same-direction horizontal/vertical lines
    // (h+h→h, v+v→v) while still absolute, so the abs/rel choice below sees
    // the merged segment (SVGO collapses before picking the shorter form).
    let commands = merge_collinear_lines(commands);

    // Phase 4: Pick shorter abs/rel per command
    let commands = abs_to_rel(commands, precision);

    // Phase 5: Simplify arc radii (round equal rx/ry to lower precision when safe)
    let commands = simplify_arc_radii(commands, precision);

    // Never emit `inf`/`NaN` or 300-digit expansions into path data
    // (arithmetic on huge coordinates can overflow even though the parser
    // rejects non-finite input, and rounding overflows before that).
    if commands
        .iter()
        .any(|c| c.args.iter().any(|&a| !representable(a, precision)))
    {
        return None;
    }

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

/// Check if control point (px, py) lies on the chord from (x0, y0) to
/// (x1, y1), i.e. its perpendicular distance to the chord is within
/// `10^-precision` (SVGO isCurveStraightLine). A curve whose end point equals
/// its start has no chord and is never considered straight — flattening a
/// closed single-cubic loop would delete it. The raw cross product is not
/// used because it scales with the chord length: a 0.07-unit bulge on a
/// 0.003-long chord looks tiny, while a 1e-8 bulge on a 7-unit chord looks
/// large.
fn is_collinear(x0: f64, y0: f64, px: f64, py: f64, x1: f64, y1: f64, precision: u32) -> bool {
    let a = -(y1 - y0);
    let b = x1 - x0;
    let len_sq = a * a + b * b;
    if len_sq == 0.0 || !len_sq.is_finite() {
        return false;
    }
    let error = 10f64.powi(-(precision as i32));
    let proj = a * (px - x0) + b * (py - y0);
    (proj * proj / len_sq).sqrt() <= error
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
fn remove_redundant(commands: Vec<PathCmd>, precision: u32, has_markers: bool) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0;
    let mut cy: f64 = 0.0;
    let mut sx: f64 = 0.0;
    let mut sy: f64 = 0.0;

    for (i, cmd) in commands.iter().enumerate() {
        match cmd.cmd {
            // A moveto immediately followed by another moveto draws nothing:
            // only the last one positions the pen (SVGO collapses `m`+`m`).
            // Kept when markers are present, since marker-start/marker-mid
            // are placed at every vertex including empty subpaths.
            'M' if !has_markers && commands.get(i + 1).is_some_and(|n| n.cmd == 'M') => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                sx = cx;
                sy = cy;
            }
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
///
/// Error recovery follows browsers and SVGO: parsing stops at the first
/// token that cannot be consumed (a stray letter, a malformed number, an
/// incomplete argument group, a bad arc flag) and the valid prefix is
/// returned. Returns `None` when there is no leading command letter or when
/// nothing could be parsed before the bad token, so callers leave the
/// attribute untouched. Numbers follow the SVG grammar: `1e` / `1e+` with no
/// exponent digits are malformed (as in browsers), whereas SVGO's
/// `parseFloat` would read them as `1`.
///
/// Use [`parse_path_strict`] when the caller re-emits the *original* string
/// rather than the parsed commands, so a truncated tail can never be copied
/// along.
pub(crate) fn parse_path(d: &str) -> Option<Vec<PathCmd>> {
    parse_path_inner(d).map(|(commands, _)| commands)
}

/// Like [`parse_path`] but returns `None` if any part of the input could not
/// be parsed, instead of returning the valid prefix.
pub(crate) fn parse_path_strict(d: &str) -> Option<Vec<PathCmd>> {
    match parse_path_inner(d) {
        Some((commands, false)) => Some(commands),
        _ => None,
    }
}

/// Returns the parsed commands and whether parsing stopped early.
fn parse_path_inner(d: &str) -> Option<(Vec<PathCmd>, bool)> {
    let mut commands = Vec::new();
    let mut chars = d.chars().peekable();
    let mut current_cmd: Option<char> = None;
    // Set when parsing stopped early because of an unconsumable token.
    let mut truncated = false;

    'outer: while chars.peek().is_some() {
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

        let Some(mut cmd) = current_cmd else {
            // A number (or garbage) with no command in effect: either the path
            // does not start with a command letter, or junk follows a Z.
            if commands.is_empty() {
                return None;
            }
            truncated = true;
            break 'outer;
        };
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
                    // Invalid arc flag: stop parsing, keep the valid prefix.
                    truncated = true;
                    break 'outer;
                }
                if let Some(n) = parse_number(&mut chars) {
                    args.push(n);
                } else {
                    // Not a number, not a command letter and not EOF (both were
                    // checked above): the char would never be consumed, so stop
                    // here instead of looping forever (issue #22). An
                    // incomplete argument group is dropped the same way.
                    truncated = true;
                    break 'outer;
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

    if truncated && commands.is_empty() {
        // Nothing usable before the bad token: leave the attribute alone.
        return None;
    }

    // The first moveto is absolute per spec even when written `m` (SVGO's
    // path2js does the same), so every consumer can treat `commands[0]` as
    // `M` and shift/serialize it accordingly. Implicit pairs after it were
    // already split into `l` above and stay relative.
    if let Some(first) = commands.first_mut()
        && first.cmd == 'm'
    {
        first.cmd = 'M';
    }

    Some((commands, truncated))
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
    let mut result = Vec::with_capacity(commands.len());
    let (mut cx, mut cy) = (0.0f64, 0.0f64);
    let (mut sx, mut sy) = (0.0f64, 0.0f64);

    for mut cmd in commands {
        if matches!(cmd.cmd, 'A' | 'a') && cmd.args.len() >= 7 {
            // The sagitta needs the chord *vector*; for an absolute `A` the
            // end point must be taken relative to the current point (SVGO's
            // calculateSagitta only ever sees relative data).
            let (dx, dy) = if cmd.cmd == 'A' {
                (cmd.args[5] - cx, cmd.args[6] - cy)
            } else {
                (cmd.args[5], cmd.args[6])
            };
            round_arc_radius(&mut cmd, dx, dy, precision, error);
        }
        advance_point(&cmd, &mut cx, &mut cy, &mut sx, &mut sy);
        result.push(cmd);
    }
    result
}

/// Round a circular arc's radius to the lowest precision whose sagitta stays
/// within `error` of the original (SVGO smartArcRounding). `(dx, dy)` is the
/// chord vector from the arc's start point to its end point.
fn round_arc_radius(cmd: &mut PathCmd, dx: f64, dy: f64, precision: u32, error: f64) {
    let rx = cmd.args[0];
    let ry = cmd.args[1];
    let large_arc = cmd.args[3];

    // Only simplify circular arcs (rx ≈ ry), skip large-arc (sagitta undefined)
    if (rx - ry).abs() >= error || large_arc == 1.0 || rx <= 0.0 {
        return;
    }
    let chord = (dx * dx + dy * dy).sqrt();
    // Skip if chord > diameter (invalid arc)
    if chord > rx * 2.0 {
        return;
    }
    let sagitta = rx - (rx * rx - 0.25 * chord * chord).max(0.0).sqrt();

    // Try progressively lower precision
    for p in (0..precision).rev() {
        let factor = 10f64.powi(p as i32);
        let rounded = (rx * factor).round() / factor;
        if rounded <= 0.0 || chord > rounded * 2.0 {
            break;
        }
        let new_sagitta = rounded - (rounded * rounded - 0.25 * chord * chord).max(0.0).sqrt();
        if (sagitta - new_sagitta).abs() < error {
            cmd.args[0] = rounded;
            cmd.args[1] = rounded;
        } else {
            break;
        }
    }
}

/// Advance the current point `(cx, cy)` (and subpath start `(sx, sy)`) past
/// `cmd`, which may be absolute or relative.
fn advance_point(cmd: &PathCmd, cx: &mut f64, cy: &mut f64, sx: &mut f64, sy: &mut f64) {
    let a = &cmd.args;
    match cmd.cmd {
        'M' => {
            *cx = a[0];
            *cy = a[1];
            *sx = *cx;
            *sy = *cy;
        }
        'm' => {
            *cx += a[0];
            *cy += a[1];
            *sx = *cx;
            *sy = *cy;
        }
        'L' | 'T' => {
            *cx = a[0];
            *cy = a[1];
        }
        'l' | 't' => {
            *cx += a[0];
            *cy += a[1];
        }
        'H' => *cx = a[0],
        'h' => *cx += a[0],
        'V' => *cy = a[0],
        'v' => *cy += a[0],
        'C' => {
            *cx = a[4];
            *cy = a[5];
        }
        'c' => {
            *cx += a[4];
            *cy += a[5];
        }
        'S' | 'Q' => {
            *cx = a[2];
            *cy = a[3];
        }
        's' | 'q' => {
            *cx += a[2];
            *cy += a[3];
        }
        'A' => {
            *cx = a[5];
            *cy = a[6];
        }
        'a' => {
            *cx += a[5];
            *cy += a[6];
        }
        'Z' | 'z' => {
            *cx = *sx;
            *cy = *sy;
        }
        _ => {}
    }
}

/// Merge a horizontal (or vertical) absolute `L` into the previous `L` when
/// both lie on the same axis line and point the same way — the `h+h→h` /
/// `v+v→v` collapse of SVGO's collapseRepeated. Opposite directions are
/// kept: `h30h-30` is a visible stroked line drawn there and back, not
/// nothing. Other commands are never merged (consecutive `l` segments go
/// to distinct waypoints).
fn merge_collinear_lines(commands: Vec<PathCmd>) -> Vec<PathCmd> {
    let mut result: Vec<PathCmd> = Vec::with_capacity(commands.len());
    let (mut cx, mut cy) = (0.0f64, 0.0f64); // current point
    let (mut sx, mut sy) = (0.0f64, 0.0f64); // subpath start
    // Start point of the last pushed command when it is an `L`.
    let mut line_from: Option<(f64, f64)> = None;

    for cmd in commands {
        match cmd.cmd {
            'L' => {
                let (x, y) = (cmd.args[0], cmd.args[1]);
                if let Some((fx, fy)) = line_from {
                    let same_sign = |a: f64, b: f64| (a >= 0.0) == (b >= 0.0);
                    let horizontal = fy == cy && cy == y && same_sign(cx - fx, x - cx);
                    let vertical = fx == cx && cx == x && same_sign(cy - fy, y - cy);
                    if (horizontal || vertical)
                        && let Some(prev) = result.last_mut()
                    {
                        prev.args = vec![x, y];
                        cx = x;
                        cy = y;
                        continue;
                    }
                }
                line_from = Some((cx, cy));
                cx = x;
                cy = y;
                result.push(cmd);
            }
            'M' => {
                cx = cmd.args[0];
                cy = cmd.args[1];
                sx = cx;
                sy = cy;
                line_from = None;
                result.push(cmd);
            }
            'Z' | 'z' => {
                cx = sx;
                cy = sy;
                line_from = None;
                result.push(cmd);
            }
            _ => {
                if cmd.args.len() >= 2 {
                    cx = cmd.args[cmd.args.len() - 2];
                    cy = cmd.args[cmd.args.len() - 1];
                }
                line_from = None;
                result.push(cmd);
            }
        }
    }

    result
}

/// Serialize path commands into an optimized string.
pub(crate) fn serialize_path(commands: &[PathCmd], precision: u32) -> String {
    let mut out = String::new();
    let mut prev_cmd: Option<char> = None;

    for cmd in commands {
        let c = cmd.cmd;

        // Omit repeated command letters (implicit repeat). Never for M/m: a
        // repeated moveto's letter is mandatory, since `M1 1 50 50` means
        // "move, then *line to*" (consecutive movetos used to be serialized
        // as implicit linetos and drew a stray segment).
        let emit_cmd = if c == 'M' || c == 'm' {
            true
        } else if prev_cmd == Some(c) {
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
    // strongRound; a value whose `val * 10^precision` overflows comes back
    // unrounded rather than as `inf`.
    format_num(strong_round(val, precision))
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

    // Reject overflow (`1e999` parses as inf) so no non-finite value enters
    // the pipeline.
    s.parse::<f64>().ok().filter(|v| v.is_finite())
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

    #[test]
    fn issue_22_unconsumable_token_keeps_valid_prefix() {
        // Used to loop forever allocating (issue #22).
        let cmds = parse_path("M0 0 e").unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(optimize_path("M0 0 e", 3).as_deref(), Some("M0 0"));

        // Valid prefix kept, garbage and everything after it dropped.
        let cmds = parse_path("M0 0 10 20 e 5 5").unwrap();
        assert_eq!(cmds.iter().map(|c| c.cmd).collect::<String>(), "ML");
        assert_eq!(cmds[1].args, vec![10.0, 20.0]);

        // Trailing dash, incomplete group, bad arc flag, stray symbol all stop cleanly.
        assert_eq!(parse_path("M0 0 -").unwrap().len(), 1);
        assert_eq!(parse_path("M0 0L10").unwrap().len(), 1);
        assert_eq!(parse_path("M0 0a1 1 0 2 0 5 5").unwrap().len(), 1);
        assert_eq!(parse_path("M0 0c0,0,0$0,0,0").unwrap().len(), 1);
        assert_eq!(parse_path("M0 0 L nan nan").unwrap().len(), 1);

        // Nothing usable before the garbage → None (attribute left alone).
        assert!(parse_path("e").is_none());
        assert!(parse_path("hello world").is_none());
        assert!(parse_path("10 20").is_none());

        // End to end through the pass.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0 e\"/><path d=\"hello world\"/></svg>";
        let mut doc = parse_svg(input).unwrap();
        ConvertPathData::default().run(&mut doc);
        let out = serialize(&doc);
        assert!(out.contains("d=\"M0 0\""), "{out}");
        assert!(out.contains("d=\"hello world\""), "{out}");
    }

    #[test]
    fn parse_path_terminates_on_every_byte() {
        for b in 0x20u8..=0x7Eu8 {
            let c = b as char;
            let _ = parse_path(&format!("M0 0 {c}"));
            let _ = parse_path(&format!("M0 0 L5 5 {c} 1 2"));
            let _ = optimize_path(&format!("M0 0 L5 5{c}1 2 L9 9"), 3);
        }
        for c in ['٣', '５', 'ǎ', '€', '\u{feff}'] {
            let _ = parse_path(&format!("M0 0 {c}"));
            let _ = parse_path(&format!("M0 0 L{c}5 5"));
        }
    }

    #[test]
    fn non_finite_and_huge_numbers_never_serialized() {
        // Overflowing literals are rejected by the number parser.
        assert_eq!(parse_path("M0 0L1e999 5").unwrap().len(), 1);
        assert!(parse_path("M1e999 0").is_none());
        // Finite but unroundable values leave the path untouched.
        assert!(optimize_path("M0 0L1e308 5", 3).is_none());
        for d in ["M0 0L1e999 5", "M0 0L1e308 5", "M1e300 1e300l1e300 1e300"] {
            if let Some(out) = optimize_path(d, 3) {
                assert!(!out.contains("inf") && !out.contains("NaN"), "{d} -> {out}");
                assert!(!out.contains("100000000000"), "{d} -> {out}");
            }
        }
    }

    #[test]
    fn consecutive_movetos_keep_their_letter() {
        // `M1 1 50 50` means move then LINE to (50,50): a repeated M must
        // never be collapsed into an implicit lineto.
        let cmds = parse_path("M1 1M50 50L20 20").unwrap();
        let out = serialize_path(&cmds, 3);
        assert!(out.starts_with("M1 1M50 50"), "{out}");
        // The leading `m` is absolute per spec and is parsed as `M`.
        let cmds = parse_path("m1 1m50 50l20 20").unwrap();
        let out = serialize_path(&cmds, 3);
        assert!(out.starts_with("M1 1m50 50"), "{out}");
        // Round trip: re-parsing the output yields the same command sequence.
        let reparsed = parse_path(&out).unwrap();
        assert_eq!(reparsed.len(), 3);
        assert_eq!(reparsed[1].cmd, 'm');
    }

    #[test]
    fn leading_relative_moveto_is_parsed_as_absolute() {
        // SVGO path2js: the first moveto is absolute even when written `m`.
        let cmds = parse_path("m5 5 10 0 0 10z").unwrap();
        assert_eq!(cmds.iter().map(|c| c.cmd).collect::<String>(), "Mllz");
        let cmds = parse_path_strict("m5 5h10zm20 0h5z").unwrap();
        assert_eq!(cmds.iter().map(|c| c.cmd).collect::<String>(), "Mhzmhz");
        // Later `m` subpaths stay relative; the serialized form is unchanged.
        assert_eq!(serialize_path(&cmds, 3), "M5 5h10zm20 0h5z");
    }

    #[test]
    fn strong_round_compares_candidate_with_original() {
        // SVGO strongRound: drop a decimal only when that loses < 10^-p
        // against the *original* value.
        for (v, expected) in [
            (5.0009, "5"),
            (1.0014, "1.001"),
            (1.00149, "1.001"),
            (7.0006, "7"),
            (3.0014, "3.001"),
            (5.0005, "5"),
            (2.3491, "2.35"),
            (18.079, "18.079"),
            (-18.079, "-18.079"),
            (-0.1505, "-.15"),
        ] {
            assert_eq!(round_and_format(v, 3), expected, "{v}");
        }
        assert_eq!(round_and_format(1.96545901, 8), "1.96545901");
        assert_eq!(round_and_format(5.01352941, 8), "5.01352941");
        assert_eq!(round_and_format(1.5, 0), "2");
        // SVGO convertPathData.14 row 1: the kappa circle quadrant's radius
        // comes out as 5.0005 and must print as 5 for the arc to be worth it.
        assert_eq!(
            optimize_path("M0 0c2.761 0 5 2.239 5 5", 3).unwrap(),
            "M0 0A5 5 0 0 1 5 5"
        );
    }

    #[test]
    fn arc_radius_rounding_uses_relative_chord() {
        // Absolute `A`: the chord is end - current point, not the raw end
        // point. Rounding 50.4 -> 50 would change the sagitta by 0.06.
        let out = optimize_path("M-50 0A50.4 50.4 0 0 1 .5 .5", 3).unwrap();
        assert!(out.contains("50.4 50.4"), "{out}");
        // SVGO convertPathData.15 at precision 2 keeps 6.44.
        let out = optimize_path("M.05 6.45A6.44 6.44 0 0 1 6.48 .02", 2).unwrap();
        assert!(out.contains("6.44 6.44"), "{out}");
        // SVGO convertPathData.33: with the right (unit) chord the radius
        // rounds all the way to 11 (SVGO: `M1 1a11 11 0 0 0 1 0`).
        let out = optimize_path("m 1 1 a 10.567 10.567 0 0 0 1 0", 3).unwrap();
        assert!(out.contains("11 11 0 0 0"), "{out}");
        assert!(!out.contains("10.6"), "{out}");
        // A relative `a` is unaffected.
        let out = optimize_path("M100 100a100.4 100.4 0 0 1 1 .5", 3).unwrap();
        assert!(out.contains("a100 100"), "{out}");
    }

    #[test]
    fn straight_curve_detection_uses_perpendicular_distance() {
        // A cubic that ends where it starts has no chord: never flattened
        // (the old cross-product test saw 0 and erased the whole loop).
        let out = optimize_path("M5 5C9 1 9 9 5 5", 3).unwrap();
        assert!(out.contains('c') || out.contains('C'), "{out}");
        let out = optimize_path("M5 5C9 1 9 9 5.0001 5", 3).unwrap();
        assert!(out.contains('c') || out.contains('C'), "{out}");
        // Short chord, visible bulge (~0.07 units): stays a curve (SVGO:
        // `M0 0q.15-.15.002.002`).
        let out = optimize_path("M0 0C.1-.1 .1-.1 .002 .002", 3).unwrap();
        assert_eq!(out, "M0 0Q.15-.15.002.002");
        // SVGO convertPathData.17 at precision 8: a long near-straight curve
        // (control point 7e-9 off the chord) becomes a line.
        let out = optimize_path(
            "M33.027833,1.96545901 C33.097408,2.03503401 38.0413624,6.97898843 38.0413624,6.97898842",
            8,
        )
        .unwrap();
        assert_eq!(out, "M33.027833 1.96545901l5.0135294 5.01352941");
        // Genuinely straight control points still collapse at p=3.
        assert_eq!(optimize_path("M0 0C1 1 2 2 3 3", 3).unwrap(), "M0 0 3 3");
    }

    #[test]
    fn opposite_direction_lines_are_not_merged_away() {
        // SVGO convertPathData.12: h/h and v/v collapse only when
        // they point the same way; a stroked back-and-forth line must stay.
        // (SVGO additionally rewrites a line back to the start as `z`.)
        for (input, expected) in [
            ("M10 50h30h-30", "M10 50h30H10"),
            ("M10 50h-30h30", "M10 50h-30 30"),
            ("M10 50h-30h-50", "M10 50h-80"),
            ("M10 50h30h50", "M10 50h80"),
            ("M10 50v30v-30", "M10 50v30V50"),
            ("M10 50v-30v30", "M10 50V20v30"),
            ("M10 50v-30v-50", "M10 50v-80"),
            ("M10 50v30v50", "M10 50v80"),
            ("M10 50L10 80L10 0", "M10 50v30V0"),
            ("M10 50L10 10L10 80", "M10 50V10v70"),
            ("M10 50L80 50L0 50", "M10 50h70H0"),
            ("M10 50L0 50L80 50", "M10 50H0h80"),
            // SVGO convertPathData.28 (SVGO additionally emits the last
            // segment as `z`): the V/v pair must survive.
            ("M6 32.845 6 14.766 6 32.845", "M6 32.845V14.766v18.079"),
        ] {
            let out = optimize_path(input, 3).unwrap_or_else(|| input.to_string());
            assert_eq!(out, expected, "{input}");
        }
    }

    #[test]
    fn moveto_before_moveto_is_dropped() {
        // SVGO collapses `m`+`m`: only the last moveto positions the pen.
        assert_eq!(
            optimize_path("M1 1M50 50L20 20L20 50z", 3).unwrap(),
            "M50 50 20 20v30z"
        );
        let out = optimize_path("M10 10 M20 20 L30 30", 3).unwrap();
        assert!(!out.contains("M10 10 20 20"), "{out}");
        assert_eq!(out, "M20 20 30 30");
        // A merged path with stacked movetos (mergePaths output).
        let out = optimize_path("M 50 50 M1.25.75 M 50 50 20 20", 3).unwrap();
        assert_eq!(out, "M50 50 20 20");
        let reparsed = parse_path(&out).unwrap();
        assert_eq!(reparsed.iter().map(|c| c.cmd).collect::<String>(), "ML");
        // SVGO convertPathData.12, last row (SVGO: `M10 50H0m30-40L10 80`;
        // svgm prefers the absolute form on equal length).
        assert_eq!(
            optimize_path("M10 50L0 50M80 50M30 10L10 80", 3).unwrap(),
            "M10 50H0M30 10 10 80"
        );
    }

    #[test]
    fn moveto_before_moveto_kept_with_markers() {
        // marker-start / marker-mid place a glyph at every vertex, so a
        // moveto that draws nothing still matters. Attribute, style
        // attribute, inherited and stylesheet forms are all honoured.
        for input in [
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path marker-mid=\"url(#m)\" d=\"M1 1M50 50L20 20L20 50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"marker-start:url(#m)\" d=\"M1 1M50 50L20 20L20 50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g marker-start=\"url(#m)\"><path d=\"M1 1M50 50L20 20L20 50z\"/></g></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>@media print{path{marker-mid:url(#m)}}</style><path d=\"M1 1M50 50L20 20L20 50z\"/></svg>",
        ] {
            let mut doc = parse_svg(input).unwrap();
            ConvertPathData::default().run(&mut doc);
            let out = serialize(&doc);
            assert!(out.contains("M1 1M50 50"), "{out}");
        }
        // marker-end does not involve the leading vertex; `none` is no marker.
        for input in [
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path marker-end=\"url(#m)\" d=\"M1 1M50 50L20 20L20 50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path marker-mid=\"none\" d=\"M1 1M50 50L20 20L20 50z\"/></svg>",
        ] {
            let mut doc = parse_svg(input).unwrap();
            ConvertPathData::default().run(&mut doc);
            let out = serialize(&doc);
            assert!(out.contains("d=\"M50 50 20 20v30z\""), "{out}");
        }
    }

    #[test]
    fn garbage_after_closepath_keeps_prefix_and_strict_rejects_it() {
        let cmds = parse_path("M0 0 L5 5 z xyz").unwrap();
        assert_eq!(cmds.iter().map(|c| c.cmd).collect::<String>(), "MLz");
        assert_eq!(parse_path("M0 0 L5 5 Z 5 5").unwrap().len(), 3);
        assert!(parse_path_strict("M0 0 L5 5 z xyz").is_none());
        assert!(parse_path_strict("M0 0 e").is_none());
        assert!(parse_path_strict("M0 0 L10").is_none());
        assert_eq!(parse_path_strict("M0 0 L5 5z").unwrap().len(), 3);
        // Exponent without digits is malformed (SVG grammar), parsing stops there.
        assert_eq!(parse_path("M0 0 L5 1e").unwrap().len(), 1);
    }
}
