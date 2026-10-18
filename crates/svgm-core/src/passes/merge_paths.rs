use super::convert_path_data::{PathCmd, parse_path_strict};
use super::{Pass, PassResult};
use crate::ast::{Document, NodeId, NodeKind};

pub struct MergePaths;

impl Pass for MergePaths {
    fn name(&self) -> &'static str {
        "mergePaths"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        // Collect parent IDs that have children (deduplicated, in traversal order)
        let mut parents_seen = std::collections::HashSet::new();
        let mut parents: Vec<NodeId> = Vec::new();
        for &id in &ids {
            if let Some(parent_id) = doc.node(id).parent
                && parents_seen.insert(parent_id)
            {
                parents.push(parent_id);
            }
        }

        for parent_id in parents {
            if merge_adjacent_paths(doc, parent_id) {
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

/// Attributes that unconditionally block merging.
const BLOCKING_ATTRS: &[&str] = &[
    "id",
    "marker-start",
    "marker-mid",
    "marker-end",
    "clip-path",
    "mask",
    "mask-image",
];

/// Attributes where a url() reference blocks merging — gradient/pattern bounding
/// box changes when paths are combined.
const URL_BLOCKING_ATTRS: &[&str] = &["fill", "filter", "stroke"];

/// Inheritable presentation attributes that block merging if present on any ancestor.
const INHERITED_BLOCKING_ATTRS: &[&str] = &[
    "marker-start",
    "marker-mid",
    "marker-end",
    "clip-path",
    "mask",
    "mask-image",
];

/// Check if a string contains a `url(` reference.
fn contains_url_ref(value: &str) -> bool {
    value.contains("url(")
}

/// Check if any ancestor of the given node has an inheritable blocking attribute.
fn has_inherited_blocking_attr(doc: &Document, id: NodeId) -> bool {
    let mut current = doc.node(id).parent;
    while let Some(parent_id) = current {
        if let NodeKind::Element(ref elem) = doc.node(parent_id).kind {
            for attr in &elem.attributes {
                if attr.prefix.is_none() && INHERITED_BLOCKING_ATTRS.contains(&attr.name.as_str()) {
                    return true;
                }
            }
        }
        current = doc.node(parent_id).parent;
    }
    false
}

/// Try to merge adjacent <path> siblings under the given parent.
/// Returns true if any merge was performed.
fn merge_adjacent_paths(doc: &mut Document, parent_id: NodeId) -> bool {
    let mut changed = false;

    loop {
        let children: Vec<NodeId> = doc.children(parent_id).collect();
        let mut merged_any = false;

        let mut i = 0;
        while i < children.len() {
            // Find a run of mergeable paths starting at i
            if !is_mergeable_path(doc, children[i]) {
                i += 1;
                continue;
            }

            let run_start = i;
            let mut run_end = i + 1;

            while run_end < children.len() {
                // Skip whitespace-only text nodes between paths
                if is_whitespace_text(doc, children[run_end]) {
                    run_end += 1;
                    continue;
                }

                if is_mergeable_path(doc, children[run_end])
                    && attrs_match(doc, children[run_start], children[run_end])
                {
                    run_end += 1;
                } else {
                    break;
                }
            }

            // Collect the actual path IDs in this run (skip whitespace nodes)
            let path_ids: Vec<NodeId> = children[run_start..run_end]
                .iter()
                .copied()
                .filter(|&id| is_mergeable_path(doc, id))
                .collect();

            if path_ids.len() >= 2 {
                // Try to merge paths, checking intersection for each pair
                let mut accumulated_d = String::new();
                let mut accumulated_points: Option<PathPoints> = None;
                let mut first_id = path_ids[0];
                let mut merged_ids: Vec<NodeId> = Vec::new();

                // Get first path's d
                if let NodeKind::Element(ref elem) = doc.node(first_id).kind
                    && let Some(d) = elem.attr("d")
                {
                    accumulated_d.push_str(d);
                    accumulated_points = gather_points(d);
                }

                for &path_id in &path_ids[1..] {
                    let current_d = if let NodeKind::Element(ref elem) = doc.node(path_id).kind
                        && let Some(d) = elem.attr("d")
                    {
                        d.to_string()
                    } else {
                        continue;
                    };

                    // Check if accumulated path intersects with current path
                    let should_merge = match (&accumulated_points, gather_points(&current_d)) {
                        (Some(acc), Some(cur)) => !intersects(acc, &cur),
                        // If we can't parse either path, don't merge (safe default)
                        _ => false,
                    };

                    if should_merge {
                        // Add separator if needed
                        if !accumulated_d.is_empty()
                            && !accumulated_d.ends_with(' ')
                            && !accumulated_d.ends_with('z')
                            && !accumulated_d.ends_with('Z')
                        {
                            accumulated_d.push(' ');
                        }
                        accumulated_d.push_str(&current_d);

                        // Update accumulated points
                        if let Some(ref cur_pts) = gather_points(&current_d)
                            && let Some(ref mut acc_pts) = accumulated_points
                        {
                            acc_pts.merge(cur_pts);
                        }

                        merged_ids.push(path_id);
                    } else {
                        // Can't merge this path — flush accumulated into first_id
                        // and start a new accumulation from this path
                        if !merged_ids.is_empty() {
                            // Write accumulated d to first path
                            if let NodeKind::Element(ref mut elem) = doc.node_mut(first_id).kind
                                && let Some(d_attr) = elem
                                    .attributes
                                    .iter_mut()
                                    .find(|a| a.name == "d" && a.prefix.is_none())
                            {
                                d_attr.value = accumulated_d.clone();
                            }
                            for &mid in &merged_ids {
                                doc.remove(mid);
                            }
                            merged_any = true;
                            changed = true;
                        }

                        // Start fresh from this path
                        first_id = path_id;
                        accumulated_d = current_d.clone();
                        accumulated_points = gather_points(&current_d);
                        merged_ids.clear();
                    }
                }

                // Flush any remaining accumulated merges
                if !merged_ids.is_empty() {
                    if let NodeKind::Element(ref mut elem) = doc.node_mut(first_id).kind
                        && let Some(d_attr) = elem
                            .attributes
                            .iter_mut()
                            .find(|a| a.name == "d" && a.prefix.is_none())
                    {
                        d_attr.value = accumulated_d;
                    }
                    for &mid in &merged_ids {
                        doc.remove(mid);
                    }

                    // Also remove whitespace text nodes that were between the merged paths
                    for &child_id in &children[run_start..run_end] {
                        if is_whitespace_text(doc, child_id) {
                            doc.remove(child_id);
                        }
                    }

                    merged_any = true;
                    changed = true;
                }
            }

            i = run_end;
        }

        if !merged_any {
            break;
        }
    }

    changed
}

/// Check if a node is a <path> element that can participate in merging.
fn is_mergeable_path(doc: &Document, id: NodeId) -> bool {
    let node = doc.node(id);
    if node.removed {
        return false;
    }

    let elem = match &node.kind {
        NodeKind::Element(e) if e.name == "path" => e,
        _ => return false,
    };

    // Must have a d attribute
    if elem.attr("d").is_none() {
        return false;
    }

    // Block if it has any attributes that make merging unsafe
    for attr in &elem.attributes {
        if attr.prefix.is_none() {
            if BLOCKING_ATTRS.contains(&attr.name.as_str()) {
                return false;
            }
            // Block if fill, stroke, or filter uses url() references
            if URL_BLOCKING_ATTRS.contains(&attr.name.as_str()) && contains_url_ref(&attr.value) {
                return false;
            }
        }
    }

    // Block if any ancestor has an inheritable blocking attribute
    if has_inherited_blocking_attr(doc, id) {
        return false;
    }

    // Block if it has animation children
    for child_id in doc.children(id) {
        if let NodeKind::Element(ref child_elem) = doc.node(child_id).kind {
            match child_elem.name.as_str() {
                "animate" | "animateTransform" | "animateMotion" | "set" => return false,
                _ => {}
            }
        }
    }

    true
}

/// Check if all attributes except `d` are identical between two path elements.
fn attrs_match(doc: &Document, a: NodeId, b: NodeId) -> bool {
    let elem_a = match &doc.node(a).kind {
        NodeKind::Element(e) => e,
        _ => return false,
    };
    let elem_b = match &doc.node(b).kind {
        NodeKind::Element(e) => e,
        _ => return false,
    };

    // Collect non-d attributes from each
    let attrs_a: Vec<(&Option<String>, &str, &str)> = elem_a
        .attributes
        .iter()
        .filter(|a| !(a.name == "d" && a.prefix.is_none()))
        .map(|a| (&a.prefix, a.name.as_str(), a.value.as_str()))
        .collect();

    let attrs_b: Vec<(&Option<String>, &str, &str)> = elem_b
        .attributes
        .iter()
        .filter(|a| !(a.name == "d" && a.prefix.is_none()))
        .map(|a| (&a.prefix, a.name.as_str(), a.value.as_str()))
        .collect();

    if attrs_a.len() != attrs_b.len() {
        return false;
    }

    // Every attr in A must have an exact match in B (order-independent)
    for &(prefix_a, name_a, value_a) in &attrs_a {
        if !attrs_b.iter().any(|&(prefix_b, name_b, value_b)| {
            prefix_a == prefix_b && name_a == name_b && value_a == value_b
        }) {
            return false;
        }
    }

    true
}

/// Check if a node is a whitespace-only text node.
fn is_whitespace_text(doc: &Document, id: NodeId) -> bool {
    if doc.node(id).removed {
        return false;
    }
    matches!(&doc.node(id).kind, NodeKind::Text(t) if t.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Geometric intersection detection (port of SVGO's _path.js)
// ---------------------------------------------------------------------------

/// A subpath's collected sample points with AABB extremes.
#[derive(Debug, Clone)]
struct SubPath {
    points: Vec<[f64; 2]>,
    min_x: usize, // index into points
    max_x: usize,
    min_y: usize,
    max_y: usize,
}

impl SubPath {
    fn new() -> Self {
        SubPath {
            points: Vec::new(),
            min_x: 0,
            max_x: 0,
            min_y: 0,
            max_y: 0,
        }
    }

    fn add_point(&mut self, pt: [f64; 2]) {
        let idx = self.points.len();
        if self.points.is_empty() {
            self.min_x = 0;
            self.max_x = 0;
            self.min_y = 0;
            self.max_y = 0;
        } else {
            if pt[0] < self.points[self.min_x][0] {
                self.min_x = idx;
            }
            if pt[0] > self.points[self.max_x][0] {
                self.max_x = idx;
            }
            if pt[1] < self.points[self.min_y][1] {
                self.min_y = idx;
            }
            if pt[1] > self.points[self.max_y][1] {
                self.max_y = idx;
            }
        }
        self.points.push(pt);
    }
}

/// Collected points from an entire path, grouped by subpath with global AABB.
#[derive(Debug, Clone)]
struct PathPoints {
    subpaths: Vec<SubPath>,
    global_min_x: f64,
    global_max_x: f64,
    global_min_y: f64,
    global_max_y: f64,
}

impl PathPoints {
    fn new() -> Self {
        PathPoints {
            subpaths: Vec::new(),
            global_min_x: f64::INFINITY,
            global_max_x: f64::NEG_INFINITY,
            global_min_y: f64::INFINITY,
            global_max_y: f64::NEG_INFINITY,
        }
    }

    fn add_point(&mut self, pt: [f64; 2]) {
        if let Some(sp) = self.subpaths.last_mut() {
            sp.add_point(pt);
        }
        if pt[0] < self.global_min_x {
            self.global_min_x = pt[0];
        }
        if pt[0] > self.global_max_x {
            self.global_max_x = pt[0];
        }
        if pt[1] < self.global_min_y {
            self.global_min_y = pt[1];
        }
        if pt[1] > self.global_max_y {
            self.global_max_y = pt[1];
        }
    }

    /// Merge another PathPoints' subpaths and AABB into this one.
    fn merge(&mut self, other: &PathPoints) {
        self.subpaths.extend(other.subpaths.iter().cloned());
        if other.global_min_x < self.global_min_x {
            self.global_min_x = other.global_min_x;
        }
        if other.global_max_x > self.global_max_x {
            self.global_max_x = other.global_max_x;
        }
        if other.global_min_y < self.global_min_y {
            self.global_min_y = other.global_min_y;
        }
        if other.global_max_y > self.global_max_y {
            self.global_max_y = other.global_max_y;
        }
    }
}

/// Parse a path `d` string and gather sample points per subpath.
/// Returns None if the path cannot be parsed *in full*: the merged `d` is
/// built from the original strings, so a path with an unparseable tail must
/// never be merged (the garbage would sit mid-path and hide the next shape).
fn gather_points(d: &str) -> Option<PathPoints> {
    let commands = parse_path_strict(d)?;
    if commands.is_empty() {
        return None;
    }

    // Convert relative to absolute
    let commands = to_absolute(&commands);

    let mut points = PathPoints::new();
    let mut cursor = [0.0f64; 2];
    let mut start = [0.0f64; 2];
    let mut prev_cmd: Option<char> = None;
    let mut prev_ctrl_point = [0.0f64; 2]; // For S/T reflected control points

    for cmd in &commands {
        let c = cmd.cmd;
        let a = &cmd.args;

        match c {
            'M' => {
                // Start new subpath
                points.subpaths.push(SubPath::new());
                start = [a[0], a[1]];
                cursor = start;
            }
            'H' => {
                points.add_point([a[0], cursor[1]]);
                cursor[0] = a[0];
            }
            'V' => {
                points.add_point([cursor[0], a[0]]);
                cursor[1] = a[0];
            }
            'C' => {
                // Cubic Bézier: sample midpoints between consecutive control points
                // (same as SVGO's approach for convex hull approximation)
                points.add_point([0.5 * (cursor[0] + a[0]), 0.5 * (cursor[1] + a[1])]);
                points.add_point([0.5 * (a[0] + a[2]), 0.5 * (a[1] + a[3])]);
                points.add_point([0.5 * (a[2] + a[4]), 0.5 * (a[3] + a[5])]);
                prev_ctrl_point = [a[4] - a[2], a[5] - a[3]];
                cursor = [a[4], a[5]];
            }
            'S' => {
                // Smooth cubic: reflect previous control point
                if matches!(prev_cmd, Some('C') | Some('S')) {
                    points.add_point([
                        cursor[0] + 0.5 * prev_ctrl_point[0],
                        cursor[1] + 0.5 * prev_ctrl_point[1],
                    ]);
                    let ctrl = [
                        cursor[0] + prev_ctrl_point[0],
                        cursor[1] + prev_ctrl_point[1],
                    ];
                    points.add_point([0.5 * (ctrl[0] + a[0]), 0.5 * (ctrl[1] + a[1])]);
                }
                points.add_point([0.5 * (a[0] + a[2]), 0.5 * (a[1] + a[3])]);
                prev_ctrl_point = [a[2] - a[0], a[3] - a[1]];
                cursor = [a[2], a[3]];
            }
            'Q' => {
                // Quadratic Bézier: add control point
                points.add_point([a[0], a[1]]);
                prev_ctrl_point = [a[2] - a[0], a[3] - a[1]];
                cursor = [a[2], a[3]];
            }
            'T' => {
                // Smooth quadratic: reflect previous control point
                if matches!(prev_cmd, Some('Q') | Some('T')) {
                    let ctrl = [
                        cursor[0] + prev_ctrl_point[0],
                        cursor[1] + prev_ctrl_point[1],
                    ];
                    points.add_point(ctrl);
                    prev_ctrl_point = [a[0] - ctrl[0], a[1] - ctrl[1]];
                }
                cursor = [a[0], a[1]];
            }
            'A' => {
                // Arc: convert to cubic Bézier curves, then sample those
                let curves = arc_to_cubic(
                    cursor[0],
                    cursor[1],
                    a[0],
                    a[1],
                    a[2],
                    a[3] as i32,
                    a[4] as i32,
                    a[5],
                    a[6],
                );
                let mut base = cursor;
                for chunk in curves.chunks(6) {
                    if chunk.len() == 6 {
                        points.add_point([0.5 * (base[0] + chunk[0]), 0.5 * (base[1] + chunk[1])]);
                        points
                            .add_point([0.5 * (chunk[0] + chunk[2]), 0.5 * (chunk[1] + chunk[3])]);
                        points
                            .add_point([0.5 * (chunk[2] + chunk[4]), 0.5 * (chunk[3] + chunk[5])]);
                        base = [chunk[4], chunk[5]];
                        points.add_point(base);
                    }
                }
                cursor = [a[5], a[6]];
            }
            'Z' => {
                cursor = start;
            }
            _ => {}
        }

        // Add the final endpoint of every command (like SVGO does)
        // This ensures M endpoints, L endpoints, etc. are all captured
        if c != 'Z' && a.len() >= 2 {
            points.add_point([a[a.len() - 2], a[a.len() - 1]]);
        }

        // Track previous command for S/T reflection
        if c != 'Z' {
            prev_cmd = Some(c);
        }
    }

    if points.subpaths.is_empty() || points.subpaths.iter().all(|sp| sp.points.is_empty()) {
        return None;
    }

    Some(points)
}

/// Convert relative path commands to absolute.
fn to_absolute(commands: &[PathCmd]) -> Vec<PathCmd> {
    let mut result = Vec::with_capacity(commands.len());
    let mut cx: f64 = 0.0;
    let mut cy: f64 = 0.0;
    let mut start_x: f64 = 0.0;
    let mut start_y: f64 = 0.0;

    for cmd in commands {
        let c = cmd.cmd;
        let is_relative = c.is_ascii_lowercase();
        let upper = c.to_ascii_uppercase();
        let mut args = cmd.args.clone();

        if is_relative {
            match upper {
                'M' | 'L' | 'T' => {
                    args[0] += cx;
                    args[1] += cy;
                }
                'H' => {
                    args[0] += cx;
                }
                'V' => {
                    args[0] += cy;
                }
                'C' => {
                    args[0] += cx;
                    args[1] += cy;
                    args[2] += cx;
                    args[3] += cy;
                    args[4] += cx;
                    args[5] += cy;
                }
                'S' | 'Q' => {
                    args[0] += cx;
                    args[1] += cy;
                    args[2] += cx;
                    args[3] += cy;
                }
                'A' => {
                    // rx, ry, rotation, flags stay relative; only endpoint is offset
                    args[5] += cx;
                    args[6] += cy;
                }
                _ => {}
            }
        }

        // Update cursor
        match upper {
            'M' => {
                cx = args[0];
                cy = args[1];
                start_x = cx;
                start_y = cy;
            }
            'L' | 'T' => {
                cx = args[0];
                cy = args[1];
            }
            'H' => {
                cx = args[0];
            }
            'V' => {
                cy = args[0];
            }
            'C' => {
                cx = args[4];
                cy = args[5];
            }
            'S' | 'Q' => {
                cx = args[2];
                cy = args[3];
            }
            'A' => {
                cx = args[5];
                cy = args[6];
            }
            'Z' => {
                cx = start_x;
                cy = start_y;
            }
            _ => {}
        }

        result.push(PathCmd { cmd: upper, args });
    }

    result
}

/// Convert an SVG arc to one or more cubic Bézier curves.
/// Returns flat array of control points: [cp1x, cp1y, cp2x, cp2y, x, y, ...]
///
/// Based on Snap.svg's a2c implementation (Apache 2 license).
#[allow(clippy::too_many_arguments)]
fn arc_to_cubic(
    x1: f64,
    y1: f64,
    rx: f64,
    ry: f64,
    angle: f64,
    large_arc_flag: i32,
    sweep_flag: i32,
    x2: f64,
    y2: f64,
) -> Vec<f64> {
    arc_to_cubic_impl(
        x1,
        y1,
        rx,
        ry,
        angle,
        large_arc_flag,
        sweep_flag,
        x2,
        y2,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn arc_to_cubic_impl(
    mut x1: f64,
    mut y1: f64,
    mut rx: f64,
    mut ry: f64,
    angle: f64,
    large_arc_flag: i32,
    sweep_flag: i32,
    mut x2: f64,
    mut y2: f64,
    recursive: Option<[f64; 4]>,
) -> Vec<f64> {
    let pi_120 = std::f64::consts::PI * 120.0 / 180.0;
    let rad = angle.to_radians();

    let rotate_x = |x: f64, y: f64, r: f64| x * r.cos() - y * r.sin();
    let rotate_y = |x: f64, y: f64, r: f64| x * r.sin() + y * r.cos();

    let (f1, f2, cx, cy);

    if let Some(rec) = recursive {
        f1 = rec[0];
        f2 = rec[1];
        cx = rec[2];
        cy = rec[3];
    } else {
        x1 = rotate_x(x1, y1, -rad);
        y1 = rotate_y(x1, y1, -rad);
        x2 = rotate_x(x2, y2, -rad);
        y2 = rotate_y(x2, y2, -rad);

        let x = (x1 - x2) / 2.0;
        let y = (y1 - y2) / 2.0;
        let mut h = (x * x) / (rx * rx) + (y * y) / (ry * ry);

        if h > 1.0 {
            h = h.sqrt();
            rx *= h;
            ry *= h;
        }

        let rx2 = rx * rx;
        let ry2 = ry * ry;
        let denom = rx2 * y * y + ry2 * x * x;
        let sign = if large_arc_flag == sweep_flag {
            -1.0
        } else {
            1.0
        };
        let k = sign * ((rx2 * ry2 - denom).abs() / denom).sqrt();

        cx = (k * rx * y) / ry + (x1 + x2) / 2.0;
        cy = (k * -ry * x) / rx + (y1 + y2) / 2.0;

        f1 = {
            let v = ((y1 - cy) / ry).clamp(-1.0, 1.0).asin();
            if x1 < cx { std::f64::consts::PI - v } else { v }
        };
        f2 = {
            let v = ((y2 - cy) / ry).clamp(-1.0, 1.0).asin();
            if x2 < cx { std::f64::consts::PI - v } else { v }
        };

        let mut f1_adj = f1;
        let mut f2_adj = f2;
        if f1_adj < 0.0 {
            f1_adj += std::f64::consts::TAU;
        }
        if f2_adj < 0.0 {
            f2_adj += std::f64::consts::TAU;
        }
        if sweep_flag != 0 && f1_adj > f2_adj {
            f1_adj -= std::f64::consts::TAU;
        }
        if sweep_flag == 0 && f2_adj > f1_adj {
            f2_adj -= std::f64::consts::TAU;
        }

        // Reassign
        return arc_to_cubic_impl(
            x1,
            y1,
            rx,
            ry,
            angle,
            large_arc_flag,
            sweep_flag,
            x2,
            y2,
            Some([f1_adj, f2_adj, cx, cy]),
        );
    };

    let mut res = Vec::new();
    let df = f2 - f1;

    if df.abs() > pi_120 {
        let f2old = f2;
        let x2old = x2;
        let y2old = y2;
        let f2_new = f1
            + pi_120
                * if sweep_flag != 0 && f2 > f1 {
                    1.0
                } else {
                    -1.0
                };
        let x2_new = cx + rx * f2_new.cos();
        let y2_new = cy + ry * f2_new.sin();
        res = arc_to_cubic_impl(
            x2_new,
            y2_new,
            rx,
            ry,
            angle,
            0,
            sweep_flag,
            x2old,
            y2old,
            Some([f2_new, f2old, cx, cy]),
        );
    }

    let df = f2 - f1;
    let c1 = f1.cos();
    let s1 = f1.sin();
    let c2 = f2.cos();
    let s2 = f2.sin();
    let t = (df / 4.0).tan();
    let hx = (4.0 / 3.0) * rx * t;
    let hy = (4.0 / 3.0) * ry * t;

    let m = [
        -hx * s1,
        hy * c1,
        x2 + hx * s2 - x1,
        y2 - hy * c2 - y1,
        x2 - x1,
        y2 - y1,
    ];

    if recursive.is_some() {
        let mut full = m.to_vec();
        full.extend_from_slice(&res);

        // Rotate back
        let mut rotated = Vec::with_capacity(full.len());
        for i in 0..full.len() {
            if i % 2 == 0 {
                rotated.push(rotate_x(
                    full[i],
                    full.get(i + 1).copied().unwrap_or(0.0),
                    rad,
                ));
            } else {
                rotated.push(rotate_y(full[i - 1], full[i], rad));
            }
        }
        rotated
    } else {
        let mut full = m.to_vec();
        full.extend_from_slice(&res);
        full
    }
}

/// Check if two sets of path points have geometric intersection.
/// Uses AABB fast-reject then per-subpath convex hull GJK.
fn intersects(path1: &PathPoints, path2: &PathPoints) -> bool {
    // Global AABB check
    if path1.global_max_x <= path2.global_min_x
        || path2.global_max_x <= path1.global_min_x
        || path1.global_max_y <= path2.global_min_y
        || path2.global_max_y <= path1.global_min_y
    {
        return false;
    }

    // Per-subpath AABB check
    let all_disjoint = path1.subpaths.iter().all(|sp1| {
        if sp1.points.is_empty() {
            return true;
        }
        path2.subpaths.iter().all(|sp2| {
            if sp2.points.is_empty() {
                return true;
            }
            sp1.points[sp1.max_x][0] <= sp2.points[sp2.min_x][0]
                || sp2.points[sp2.max_x][0] <= sp1.points[sp1.min_x][0]
                || sp1.points[sp1.max_y][1] <= sp2.points[sp2.min_y][1]
                || sp2.points[sp2.max_y][1] <= sp1.points[sp1.min_y][1]
        })
    });

    if all_disjoint {
        return false;
    }

    // Build convex hulls and run GJK
    let hulls1: Vec<ConvexHull> = path1
        .subpaths
        .iter()
        .map(|sp| convex_hull(&sp.points))
        .collect();
    let hulls2: Vec<ConvexHull> = path2
        .subpaths
        .iter()
        .map(|sp| convex_hull(&sp.points))
        .collect();

    hulls1.iter().any(|h1| {
        if h1.points.len() < 3 {
            return false;
        }
        hulls2.iter().any(|h2| {
            if h2.points.len() < 3 {
                return false;
            }
            gjk_intersect(h1, h2)
        })
    })
}

/// A convex hull with tracked extreme point indices for GJK support.
#[derive(Debug, Clone)]
struct ConvexHull {
    points: Vec<[f64; 2]>,
    min_x: usize,
    max_x: usize,
    min_y: usize,
    max_y: usize,
}

/// Compute convex hull using monotone chain algorithm.
fn convex_hull(points: &[[f64; 2]]) -> ConvexHull {
    if points.is_empty() {
        return ConvexHull {
            points: Vec::new(),
            min_x: 0,
            max_x: 0,
            min_y: 0,
            max_y: 0,
        };
    }

    let mut sorted: Vec<[f64; 2]> = points.to_vec();
    sorted.sort_by(|a, b| {
        a[0].partial_cmp(&b[0])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal))
    });

    // Lower hull
    let mut lower: Vec<[f64; 2]> = Vec::new();
    let mut min_y_idx = 0;
    let mut bottom = 0;
    for (i, &pt) in sorted.iter().enumerate() {
        while lower.len() >= 2 && cross2d(lower[lower.len() - 2], lower[lower.len() - 1], pt) <= 0.0
        {
            lower.pop();
        }
        if pt[1] < sorted[min_y_idx][1] {
            min_y_idx = i;
            bottom = lower.len();
        }
        lower.push(pt);
    }

    // Upper hull
    let mut upper: Vec<[f64; 2]> = Vec::new();
    let mut max_y_idx = sorted.len() - 1;
    let mut top = 0;
    for i in (0..sorted.len()).rev() {
        while upper.len() >= 2
            && cross2d(upper[upper.len() - 2], upper[upper.len() - 1], sorted[i]) <= 0.0
        {
            upper.pop();
        }
        if sorted[i][1] > sorted[max_y_idx][1] {
            max_y_idx = i;
            top = upper.len();
        }
        upper.push(sorted[i]);
    }

    // Remove last point of each half (duplicate of first point of other half)
    upper.pop();
    lower.pop();

    let lower_len = lower.len();
    let mut hull_points = lower;
    hull_points.extend(upper);

    let hull_len = hull_points.len();

    ConvexHull {
        points: hull_points,
        min_x: 0,
        max_x: lower_len,
        min_y: bottom,
        max_y: if hull_len > 0 {
            (lower_len + top) % hull_len
        } else {
            0
        },
    }
}

/// 2D cross product of vectors OA and OB where O=o, A=a, B=b.
fn cross2d(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// GJK (Gilbert-Johnson-Keerthi) intersection test between two convex hulls.
fn gjk_intersect(hull1: &ConvexHull, hull2: &ConvexHull) -> bool {
    let initial_dir = [1.0, 0.0];
    let mut simplex = vec![gjk_support(hull1, hull2, initial_dir)];
    let mut direction = negate(simplex[0]);

    let mut iterations = 10_000;

    loop {
        if iterations == 0 {
            // Safety: assume collision if we can't determine
            return true;
        }
        iterations -= 1;

        simplex.push(gjk_support(hull1, hull2, direction));

        // Check if the new point passed the origin
        if dot2d(direction, *simplex.last().unwrap()) <= 0.0 {
            return false;
        }

        if process_simplex(&mut simplex, &mut direction) {
            return true;
        }
    }
}

/// Minkowski difference support function.
fn gjk_support(hull1: &ConvexHull, hull2: &ConvexHull, direction: [f64; 2]) -> [f64; 2] {
    let p1 = support_point(hull1, direction);
    let p2 = support_point(hull2, negate(direction));
    sub2d(p1, p2)
}

/// Find the farthest point in a convex hull along a direction.
fn support_point(hull: &ConvexHull, direction: [f64; 2]) -> [f64; 2] {
    // Start from a quadrant-based initial guess
    let start_idx = if direction[1] >= 0.0 {
        if direction[0] < 0.0 {
            hull.max_y
        } else {
            hull.max_x
        }
    } else if direction[0] < 0.0 {
        hull.min_x
    } else {
        hull.min_y
    };

    let n = hull.points.len();
    if n == 0 {
        return [0.0, 0.0];
    }

    let mut index = start_idx % n;
    let mut max_dot = dot2d(hull.points[index], direction);

    // Walk forward along hull to find maximum
    loop {
        let next = (index + 1) % n;
        let d = dot2d(hull.points[next], direction);
        if d > max_dot {
            max_dot = d;
            index = next;
        } else {
            break;
        }
    }

    hull.points[index]
}

/// Process the GJK simplex, updating direction and simplex.
/// Returns true if the origin is contained.
fn process_simplex(simplex: &mut Vec<[f64; 2]>, direction: &mut [f64; 2]) -> bool {
    if simplex.len() == 2 {
        // Line case
        let a = simplex[1];
        let b = simplex[0];
        let ao = negate(a);
        let ab = sub2d(b, a);

        if dot2d(ao, ab) > 0.0 {
            let perp = orth2d(ab, a);
            direction[0] = perp[0];
            direction[1] = perp[1];
        } else {
            direction[0] = ao[0];
            direction[1] = ao[1];
            simplex.remove(0); // Keep only A
        }
    } else {
        // Triangle case
        let a = simplex[2];
        let b = simplex[1];
        let c = simplex[0];
        let ab = sub2d(b, a);
        let ac = sub2d(c, a);
        let ao = negate(a);
        let acb = orth2d(ab, ac); // perpendicular to AB facing away from C
        let abc = orth2d(ac, ab); // perpendicular to AC facing away from B

        if dot2d(acb, ao) > 0.0 {
            if dot2d(ab, ao) > 0.0 {
                // Region 4: between A and B
                simplex.remove(0); // Remove C
                let perp = orth2d(ab, a);
                direction[0] = perp[0];
                direction[1] = perp[1];
            } else if dot2d(ac, ao) > 0.0 {
                // Region 6: between A and C
                simplex.remove(1); // Remove B
                let perp = orth2d(ac, a);
                direction[0] = perp[0];
                direction[1] = perp[1];
            } else {
                // Region 5: near A
                *simplex = vec![a];
                direction[0] = ao[0];
                direction[1] = ao[1];
            }
        } else if dot2d(abc, ao) > 0.0 {
            if dot2d(ac, ao) > 0.0 {
                // Region 6
                simplex.remove(1);
                let perp = orth2d(ac, a);
                direction[0] = perp[0];
                direction[1] = perp[1];
            } else {
                // Region 5
                *simplex = vec![a];
                direction[0] = ao[0];
                direction[1] = ao[1];
            }
        } else {
            // Origin is inside the triangle
            return true;
        }
    }
    false
}

// --- 2D vector helpers ---

fn dot2d(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn sub2d(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn negate(v: [f64; 2]) -> [f64; 2] {
    [-v[0], -v[1]]
}

/// Perpendicular to `v` that faces away from `from`.
fn orth2d(v: [f64; 2], from: [f64; 2]) -> [f64; 2] {
    let o = [-v[1], v[0]];
    if dot2d(o, negate(from)) < 0.0 {
        negate(o)
    } else {
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::passes::PassResult;
    use crate::serializer::serialize;

    fn run_pass(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = MergePaths.run(&mut doc);
        (result, serialize(&doc))
    }

    // --- Basic merging ---

    #[test]
    fn merges_adjacent_paths_same_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        // Should be one path with concatenated d
        assert_eq!(output.matches("<path").count(), 1);
        assert!(
            output.contains("M0 0L10 10") && output.contains("M20 20L30 30"),
            "expected merged d, got: {output}"
        );
    }

    #[test]
    fn merges_three_adjacent_paths() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"red\"/><path d=\"M40 40L50 50\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("<path").count(), 1);
    }

    #[test]
    fn no_merge_different_attrs() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"blue\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_single_path() {
        let input =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Blocking attributes ---

    #[test]
    fn no_merge_with_id() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\" id=\"a\"/><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_markers() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\" marker-end=\"url(#arrow)\"/><path d=\"M20 20L30 30\" fill=\"red\" marker-end=\"url(#arrow)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_animation_child() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"><animate attributeName=\"d\" to=\"M0 0L20 20\" dur=\"1s\"/></path><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Whitespace handling ---

    #[test]
    fn merges_paths_with_whitespace_between() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/>  \n  <path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("<path").count(), 1);
    }

    // --- Non-adjacent paths ---

    #[test]
    fn no_merge_non_adjacent() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><rect width=\"10\" height=\"10\"/><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Attribute order independence ---

    #[test]
    fn merges_with_different_attr_order() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\" stroke=\"black\"/><path d=\"M20 20L30 30\" stroke=\"black\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("<path").count(), 1);
    }

    // --- Intersection-based blocking ---

    #[test]
    fn no_merge_overlapping_paths() {
        // Two paths whose bounding boxes clearly overlap
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L20 20L40 0Z\" fill=\"red\"/><path d=\"M10 0L30 20L50 0Z\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(
            result,
            PassResult::Unchanged,
            "overlapping paths should not merge, got: {output}"
        );
    }

    #[test]
    fn merges_non_overlapping_paths() {
        // Two paths far apart
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 0L10 10L0 10Z\" fill=\"red\"/><path d=\"M100 100L110 100L110 110L100 110Z\" fill=\"red\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(
            result,
            PassResult::Changed,
            "non-overlapping paths should merge, got: {output}"
        );
        assert_eq!(output.matches("<path").count(), 1);
    }

    // --- URL reference blocking ---

    #[test]
    fn no_merge_with_url_fill() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"url(#grad)\"/><path d=\"M20 20L30 30\" fill=\"url(#grad)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_url_stroke() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" stroke=\"url(#grad)\"/><path d=\"M20 20L30 30\" stroke=\"url(#grad)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_url_filter() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" filter=\"url(#blur)\"/><path d=\"M20 20L30 30\" filter=\"url(#blur)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Inherited blocking ---

    #[test]
    fn no_merge_with_inherited_clip_path() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g clip-path=\"url(#c)\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"red\"/></g></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_inherited_mask() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g mask=\"url(#m)\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"red\"/></g></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- clip-path / mask on element ---

    #[test]
    fn no_merge_with_clip_path_attr() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\" clip-path=\"url(#c)\"/><path d=\"M20 20L30 30\" fill=\"red\" clip-path=\"url(#c)\"/></svg>";
        let (result, _) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged);
    }

    // --- Integration ---

    #[test]
    fn full_optimizer_convergence() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L10 10\" fill=\"red\"/><path d=\"M20 20L30 30\" fill=\"red\"/></svg>";
        let result1 = crate::optimize(input).unwrap();
        let result2 = crate::optimize(&result1.data).unwrap();
        assert_eq!(result1.data, result2.data, "should converge");
    }

    // --- Intersection detection unit tests ---

    #[test]
    fn test_gather_points_simple() {
        let pts = gather_points("M0 0L10 0L10 10L0 10Z").unwrap();
        assert_eq!(pts.subpaths.len(), 1);
        assert!(pts.subpaths[0].points.len() >= 3);
    }

    #[test]
    fn test_non_overlapping_rects() {
        let p1 = gather_points("M0 0L10 0L10 10L0 10Z").unwrap();
        let p2 = gather_points("M20 20L30 20L30 30L20 30Z").unwrap();
        assert!(!intersects(&p1, &p2));
    }

    #[test]
    fn test_overlapping_rects() {
        let p1 = gather_points("M0 0L10 0L10 10L0 10Z").unwrap();
        let p2 = gather_points("M5 5L15 5L15 15L5 15Z").unwrap();
        assert!(intersects(&p1, &p2));
    }

    #[test]
    fn test_mcdonalds_overlapping_paths() {
        // The first two paths from the McDonald's SVG — they clearly overlap
        let d1 = "M48.134 39.985c-.212-10.311-1.873-20.235-4.679-27.943C40.627 4.277 37.097 0 33.518 0c-2.224 0-4.313 1.618-6.212 4.812-1.176 1.981-2.272 4.582-3.225 7.643-.956-3.061-2.05-5.662-3.228-7.643C18.956 1.618 16.866 0 14.642 0c-3.582 0-7.11 4.277-9.937 12.042C1.9 19.75.238 29.674.026 39.984L0 41.194h7.187l.014-1.168c.279-21.542 5.41-35.205 7.5-35.205 1.408 0 5.006 9.461 5.552 32.885l.028 1.154h7.598l.027-1.154C28.451 14.282 32.05 4.82 33.458 4.82c2.091 0 7.22 13.663 7.5 35.205l.015 1.169h7.184z";
        let d2 = "M47.59 39.998c-.21-10.253-1.862-20.11-4.647-27.76C40.243 4.813 36.807.555 33.518.555c-3.6 0-6.99 5.002-9.437 13.819C21.633 5.558 18.242.556 14.64.556c-3.287 0-6.724 4.258-9.426 11.681C2.43 19.887.78 29.745.57 39.998l-.013.639H6.65l.007-.618c.283-21.831 5.441-35.755 8.044-35.755 2.23 0 5.589 11.616 6.097 33.427l.016.613h6.533l.014-.613c.508-21.81 3.869-33.427 6.097-33.427 2.604 0 7.762 13.924 8.044 35.755l.008.618h6.093z";
        let p1 = gather_points(d1).unwrap();
        let p2 = gather_points(d2).unwrap();
        assert!(
            intersects(&p1, &p2),
            "McDonald's arch paths should be detected as overlapping"
        );
    }

    #[test]
    fn truncated_paths_are_never_merged() {
        // The merged `d` is built from the original strings, so a path whose
        // tail could not be parsed must stay separate: `L10 M50 50` would
        // hide the second shape in every browser.
        for bad in [
            "M0 0 L5 5 L10",
            "M0 0 L5 5 e",
            "M0 0 L5 5 L1e300 1e300 L10",
            "M1 1l5",
        ] {
            let input = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"{bad}\"/><path d=\"M50 50 L60 60 L50 60z\"/></svg>"
            );
            let mut doc = crate::parser::parse(&input).unwrap();
            let result = MergePaths.run(&mut doc);
            assert_eq!(result, PassResult::Unchanged, "{bad}");
            assert_eq!(crate::serializer::serialize(&doc), input);
        }
    }
}
