use super::convert_path_data::{PathCmd, parse_path_strict, serialize_path};
use super::{
    Computed, Pass, PassResult, computed_style_with, parse_svg_number, style_declaration,
    stylesheet_declarations,
};
use crate::ast::{Document, NodeId, NodeKind};

pub struct MergePaths {
    /// Numeric precision used when re-serializing the merged `d` (SVGO
    /// mergePaths `floatPrecision`, default 3).
    pub precision: u32,
}

impl Default for MergePaths {
    fn default() -> Self {
        Self { precision: 3 }
    }
}

impl Pass for MergePaths {
    fn name(&self) -> &'static str {
        "mergePaths"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        // A stylesheet rule could give any path a marker, clip, mask or
        // url() paint; without selector matching, refuse to merge anything.
        if stylesheet_blocks_merge(doc) {
            return PassResult::Unchanged;
        }

        let sheet = SheetStroke::of(doc);
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
            if merge_adjacent_paths(doc, parent_id, self.precision, sheet) {
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

/// Properties that block merging when set to anything but `none`: markers
/// are placed at every vertex of *each* path, and clip/mask geometry in
/// objectBoundingBox units changes when the bounding box grows (SVGO
/// mergePaths checks the computed style of each).
const GEOMETRY_PROPS: &[&str] = &[
    "marker-start",
    "marker-mid",
    "marker-end",
    "marker",
    "clip-path",
    "mask",
    "mask-image",
];

/// Properties that block merging when they reference a paint server or
/// filter via `url()`: objectBoundingBox gradients, patterns and filter
/// regions change when paths are combined.
const URL_PROPS: &[&str] = &["fill", "filter", "stroke"];

/// Check if a string contains a `url(` reference.
fn contains_url_ref(value: &str) -> bool {
    value.contains("url(")
}

/// A value that activates a marker/clip/mask property.
fn is_set(value: &str) -> bool {
    let v = value.trim();
    !v.is_empty() && v != "none"
}

/// True when the path's computed style (its `style` attribute, its
/// presentation attributes, and for inheritable properties its ancestors')
/// makes merging unsafe. An ancestor's `clip-path`/`mask` is not inherited
/// in CSS terms, but stays blocking as before: the group's clip applies to
/// everything rendered inside it, and an objectBoundingBox clip on a group
/// is uncommon enough to refuse conservatively.
fn element_blocks_merge(doc: &Document, id: NodeId) -> bool {
    // A CSS `d` overrides the attribute the merge concatenates.
    if let NodeKind::Element(ref elem) = doc.node(id).kind
        && style_declaration(elem, "d").is_some()
    {
        return true;
    }
    for prop in GEOMETRY_PROPS {
        if let Computed::Static(v) = computed_style_with(doc, id, prop, false)
            && is_set(&v)
        {
            return true;
        }
    }
    for prop in URL_PROPS {
        if let Computed::Static(v) = computed_style_with(doc, id, prop, false)
            && contains_url_ref(&v)
        {
            return true;
        }
    }
    let mut current = doc.node(id).parent;
    while let Some(parent_id) = current {
        if let NodeKind::Element(ref elem) = doc.node(parent_id).kind {
            for prop in ["clip-path", "mask", "mask-image"] {
                let value =
                    style_declaration(elem, prop).or_else(|| elem.attr(prop).map(str::to_string));
                if value.is_some_and(|v| is_set(&v)) {
                    return true;
                }
            }
        }
        current = doc.node(parent_id).parent;
    }
    false
}

/// True when a remaining stylesheet rule could set a blocking property on
/// some path (selectors are not resolved here, so any such declaration
/// disables merging for the whole document — SVGO resolves them per
/// element via computeStyle).
fn stylesheet_blocks_merge(doc: &Document) -> bool {
    stylesheet_declarations(doc).iter().any(|(prop, value)| {
        prop.eq_ignore_ascii_case("d")
            || (GEOMETRY_PROPS.contains(&prop.as_str()) && is_set(value))
            || (URL_PROPS.contains(&prop.as_str()) && contains_url_ref(value))
    })
}

/// Stroke properties a remaining stylesheet may set on some path. Selectors
/// are not resolved, so a declaration applies to every path conservatively.
#[derive(Clone, Copy, Default)]
struct SheetStroke {
    /// A `stroke` declaration other than `none`: any path may be stroked.
    paints: bool,
    /// A declaration changing how far a stroke reaches: the margin is unknown.
    geometry: bool,
}

impl SheetStroke {
    fn of(doc: &Document) -> Self {
        let mut sheet = SheetStroke::default();
        for (prop, value) in stylesheet_declarations(doc) {
            if prop.eq_ignore_ascii_case("stroke") {
                sheet.paints |= !value.trim().eq_ignore_ascii_case("none");
            } else if [
                "stroke-width",
                "stroke-linejoin",
                "stroke-miterlimit",
                "stroke-linecap",
                "vector-effect",
            ]
            .iter()
            .any(|p| prop.eq_ignore_ascii_case(p))
            {
                sheet.geometry = true;
            }
        }
        sheet
    }
}

/// How far a path's stroke can reach beyond the points gathered from its
/// outline, per axis.
#[derive(Debug, Clone, Copy, PartialEq)]
enum StrokeExtent {
    /// No visible stroke: only the fill geometry matters.
    NoStroke,
    Margin(f64),
    /// The stroke reach cannot be bounded statically; never merge.
    Unknown,
}

/// A `stroke-width` in user units: a plain number or `px`. Percentages,
/// font-relative and other units, `calc()` and negatives give `None`.
fn parse_stroke_width(value: &str) -> Option<f64> {
    let v = value.trim();
    let v = if v.len() > 2 && v[v.len() - 2..].eq_ignore_ascii_case("px") {
        &v[..v.len() - 2]
    } else {
        v
    };
    parse_svg_number(v).filter(|w| *w >= 0.0)
}

/// Stroke extent of path `id`. Merging puts both fills under both strokes,
/// so where one path's stroke reaches into its neighbour the paint order
/// changes (SVGO ignores the stroke here and renders such merges
/// differently). The stroke stays within `width / 2` of the outline, except
/// at miter joins (up to `miterlimit * width / 2` from the vertex) and
/// square caps (a corner at `sqrt(2) * width / 2` on one axis).
fn stroke_extent(doc: &Document, id: NodeId, sheet: SheetStroke) -> StrokeExtent {
    let static_value = |prop: &str| match computed_style_with(doc, id, prop, false) {
        Computed::Static(v) => Some(v.trim().to_ascii_lowercase()),
        _ => None,
    };
    let stroked = static_value("stroke").is_some_and(|v| v != "none") || sheet.paints;
    if !stroked {
        return StrokeExtent::NoStroke;
    }
    if sheet.geometry {
        return StrokeExtent::Unknown;
    }
    let width = match static_value("stroke-width") {
        None => 1.0,
        Some(v) => match parse_stroke_width(&v) {
            Some(w) => w,
            None => return StrokeExtent::Unknown,
        },
    };
    if width == 0.0 {
        return StrokeExtent::NoStroke;
    }
    // A non-scaling stroke's width is in screen units.
    if static_value("vector-effect").is_some_and(|v| v != "none") {
        return StrokeExtent::Unknown;
    }
    let join = match static_value("stroke-linejoin").as_deref() {
        Some("round" | "bevel") => 1.0,
        // miter (the initial value), miter-clip and arcs are all cut off at
        // the miter limit.
        _ => match static_value("stroke-miterlimit") {
            None => 4.0,
            Some(v) => match parse_svg_number(&v) {
                Some(limit) if limit >= 1.0 => limit,
                _ => return StrokeExtent::Unknown,
            },
        },
    };
    let cap = match static_value("stroke-linecap").as_deref() {
        None | Some("butt" | "round") => 1.0,
        _ => std::f64::consts::SQRT_2,
    };
    let margin = width / 2.0 * f64::max(join, cap);
    if margin.is_finite() {
        StrokeExtent::Margin(margin)
    } else {
        StrokeExtent::Unknown
    }
}

/// Strictly parsed commands and sample points of a path element's `d`.
/// `None` when the attribute is missing or any part of it fails to parse
/// (such a path never takes part in a merge, so a truncated tail can never
/// be copied along).
fn parsed_path(doc: &Document, id: NodeId) -> Option<(Vec<PathCmd>, PathPoints)> {
    let NodeKind::Element(ref elem) = doc.node(id).kind else {
        return None;
    };
    let commands = parse_path_strict(elem.attr("d")?)?;
    let points = gather_points_from(&commands)?;
    Some((commands, points))
}

/// Overwrite the unprefixed `d` attribute of element `id`.
fn set_d(doc: &mut Document, id: NodeId, d: String) {
    if let NodeKind::Element(ref mut elem) = doc.node_mut(id).kind
        && let Some(d_attr) = elem
            .attributes
            .iter_mut()
            .find(|a| a.name == "d" && a.prefix.is_none())
    {
        d_attr.value = d;
    }
}

/// Try to merge adjacent <path> siblings under the given parent.
/// Returns true if any merge was performed.
fn merge_adjacent_paths(
    doc: &mut Document,
    parent_id: NodeId,
    precision: u32,
    sheet: SheetStroke,
) -> bool {
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

            // Every path in the run has the same parent and identical
            // non-`d` attributes, so they all share one stroke extent.
            if path_ids.len() >= 2
                && let Some(margin) = match stroke_extent(doc, path_ids[0], sheet) {
                    StrokeExtent::NoStroke => Some(0.0),
                    StrokeExtent::Margin(m) => Some(m),
                    StrokeExtent::Unknown => None,
                }
            {
                // Slack for the arc approximation and re-serialization.
                let tolerance = 10f64.powi(-(precision as i32));
                // Accumulate *parsed* commands, never raw strings: the parser
                // turns a leading relative `m` into `M` (so the second shape
                // can't be shifted by the first one's end point), and the
                // merged `d` is re-serialized at `precision` like SVGO's
                // js2path. A path that does not parse strictly is never
                // merged into or onto.
                let mut acc: Option<(Vec<PathCmd>, PathPoints)> = parsed_path(doc, path_ids[0]);
                let mut first_id = path_ids[0];
                let mut merged_ids: Vec<NodeId> = Vec::new();

                for &path_id in &path_ids[1..] {
                    let current = parsed_path(doc, path_id);

                    // Merge only when both parse and their geometry is disjoint
                    if let (Some((acc_cmds, acc_pts)), Some((cur_cmds, cur_pts))) =
                        (acc.as_mut(), current.as_ref())
                        && !intersects_with_margin(acc_pts, cur_pts, margin, tolerance)
                    {
                        acc_cmds.extend(cur_cmds.iter().cloned());
                        acc_pts.merge(cur_pts);
                        merged_ids.push(path_id);
                        continue;
                    }

                    // Can't merge this path — flush accumulated into first_id
                    // and start a new accumulation from this path
                    if !merged_ids.is_empty() {
                        if let Some((acc_cmds, _)) = &acc {
                            set_d(doc, first_id, serialize_path(acc_cmds, precision));
                        }
                        for &mid in &merged_ids {
                            doc.remove(mid);
                        }
                        merged_any = true;
                        changed = true;
                    }

                    first_id = path_id;
                    acc = current;
                    merged_ids.clear();
                }

                // Flush any remaining accumulated merges
                if !merged_ids.is_empty() {
                    if let Some((acc_cmds, _)) = &acc {
                        set_d(doc, first_id, serialize_path(acc_cmds, precision));
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

    // Must have a d attribute; an id may be referenced (use/CSS/script)
    if elem.attr("d").is_none() || elem.attr("id").is_some() {
        return false;
    }

    // Markers, clip/mask, url() paints — from attributes, style="" or
    // inherited from ancestors
    if element_blocks_merge(doc, id) {
        return false;
    }

    // SVGO merges only childless paths: <title>/<desc> are accessibility
    // content, animation elements target their parent, and anything else
    // would be silently dropped with the element. Whitespace-only text
    // (formatting) is the one exception.
    for child_id in doc.children(id) {
        let child = doc.node(child_id);
        if child.removed {
            continue;
        }
        if !matches!(&child.kind, NodeKind::Text(t) if t.trim().is_empty()) {
            return false;
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
#[cfg(test)]
fn gather_points(d: &str) -> Option<PathPoints> {
    gather_points_from(&parse_path_strict(d)?)
}

/// Sample points of every subpath (end points, curve control-polygon
/// midpoints, arcs via their cubic approximation) for the intersection test.
fn gather_points_from(commands: &[PathCmd]) -> Option<PathPoints> {
    if commands.is_empty() {
        return None;
    }

    // Convert relative to absolute
    let commands = to_absolute(commands);

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
                // a2c returns each cubic relative to the previous one's end
                // point (SVGO maps every chunk through `toAbsolute` with the
                // running base point); sampling the raw relative values put
                // every arc near the origin and broke the intersection test.
                let mut base = cursor;
                for chunk in curves.chunks(6) {
                    if chunk.len() == 6 {
                        let c: Vec<f64> = chunk
                            .iter()
                            .enumerate()
                            .map(|(i, v)| v + base[i % 2])
                            .collect();
                        points.add_point([0.5 * (base[0] + c[0]), 0.5 * (base[1] + c[1])]);
                        points.add_point([0.5 * (c[0] + c[2]), 0.5 * (c[1] + c[3])]);
                        points.add_point([0.5 * (c[2] + c[4]), 0.5 * (c[3] + c[5])]);
                        base = [c[4], c[5]];
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

    let (f1, mut f2, cx, cy);

    if let Some(rec) = recursive {
        f1 = rec[0];
        f2 = rec[1];
        cx = rec[2];
        cy = rec[3];
    } else {
        // Rotate both end points into the ellipse's frame (each pair from
        // its original coordinates, not from a half-rotated one).
        (x1, y1) = (rotate_x(x1, y1, -rad), rotate_y(x1, y1, -rad));
        (x2, y2) = (rotate_x(x2, y2, -rad), rotate_y(x2, y2, -rad));

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
            // Already in the ellipse's rotated frame: the caller rotates the
            // whole result back once, so the split must not rotate again.
            0.0,
            0,
            sweep_flag,
            x2old,
            y2old,
            Some([f2_new, f2old, cx, cy]),
        );
        // Snap's a2c reassigns f2/x2/y2 here so the chunk below spans only
        // the first 120 degrees; the remainder is in `res`.
        f2 = f2_new;
        x2 = x2_new;
        y2 = y2_new;
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

/// [`intersects`] for paths whose strokes reach `margin` past their
/// gathered points. With a margin, pieces merge only when their bounding
/// boxes are more than `2 * margin + tolerance` apart on some axis, globally
/// or for every pair of subpaths; touching counts as overlapping, and the
/// hull test is skipped because it only knows the unstroked outline (and
/// never sees degenerate hulls such as straight lines).
fn intersects_with_margin(p1: &PathPoints, p2: &PathPoints, margin: f64, tolerance: f64) -> bool {
    if margin <= 0.0 {
        return intersects(p1, p2);
    }
    let reach = 2.0 * margin + tolerance;
    // `!(gap <= reach)` would let NaN through; write it so NaN overlaps.
    let apart = |a_min: f64, a_max: f64, b_min: f64, b_max: f64| {
        b_min - a_max > reach || a_min - b_max > reach
    };
    if apart(
        p1.global_min_x,
        p1.global_max_x,
        p2.global_min_x,
        p2.global_max_x,
    ) || apart(
        p1.global_min_y,
        p1.global_max_y,
        p2.global_min_y,
        p2.global_max_y,
    ) {
        return false;
    }
    let non_empty = |sp: &&SubPath| !sp.points.is_empty();
    p1.subpaths.iter().filter(non_empty).any(|a| {
        p2.subpaths.iter().filter(non_empty).any(|b| {
            !(apart(
                a.points[a.min_x][0],
                a.points[a.max_x][0],
                b.points[b.min_x][0],
                b.points[b.max_x][0],
            ) || apart(
                a.points[a.min_y][1],
                a.points[a.max_y][1],
                b.points[b.min_y][1],
                b.points[b.max_y][1],
            ))
        })
    })
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
        let result = MergePaths::default().run(&mut doc);
        (result, serialize(&doc))
    }

    #[test]
    fn css_d_property_blocks_merging() {
        // `d` in CSS overrides the attribute the merge would concatenate.
        for input in [
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0h5\" style=\"d:path('M20 20h5')\"/><path d=\"M10 0h5\" style=\"d:path('M20 20h5')\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>.a:hover{d:path('M0 0')}</style><path d=\"M0 0h5\"/><path d=\"M10 0h5\"/></svg>",
        ] {
            let (result, output) = run_pass(input);
            assert_eq!(result, PassResult::Unchanged, "{input}");
            assert_eq!(output, input);
        }
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
            output.contains("d=\"M0 0 10 10M20 20 30 30\""),
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
    fn merged_d_is_reserialized_with_absolute_first_moveto() {
        // A leading relative `m` is absolute per spec; copying it verbatim
        // after the first path made the second shape start at (55,55).
        let (result, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"m5 5h10v10H5z\"/><path d=\"m50 50h10v10H50z\"/></svg>",
        );
        assert_eq!(result, PassResult::Changed);
        assert!(
            output.contains("d=\"M5 5h10v10H5zM50 50h10v10H50z\""),
            "{output}"
        );

        // Implicit pairs after the first moveto stay relative linetos; no
        // separator is inserted before the next piece's `M`.
        let (_, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"m5 5 10 10z\"/><path d=\"M50 50 60 60 50 60z\"/></svg>",
        );
        assert!(
            output.contains("d=\"M5 5l10 10zM50 50 60 60 50 60z\""),
            "{output}"
        );

        // Pieces are re-serialized at the pass precision.
        let mut doc = parse(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M5.00004 5h10v10H5z\"/><path d=\"M50 50h10.0001v10H50z\"/></svg>",
        )
        .unwrap();
        MergePaths { precision: 3 }.run(&mut doc);
        let output = serialize(&doc);
        assert!(
            output.contains("d=\"M5 5h10v10H5zM50 50h10v10H50z\""),
            "{output}"
        );
    }

    #[test]
    fn arc_points_are_absolute_in_intersection_test() {
        // Half disc (100,0)->(100,100) bulging to x=150, evenodd rect
        // inside it: merging would punch a hole.
        let (result, _) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path fill-rule=\"evenodd\" d=\"M100 0a50 50 0 0 1 0 100z\"/><path fill-rule=\"evenodd\" d=\"M110 40h10v10h-10z\"/></svg>",
        );
        assert_eq!(result, PassResult::Unchanged);
        // A rect on the concave side (left of the chord) does not intersect.
        let (result, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M100 0a50 50 0 0 1 0 100\"/><path d=\"M40 40h10v10H40z\"/></svg>",
        );
        assert_eq!(result, PassResult::Changed, "{output}");
        assert!(
            output.contains("d=\"M100 0a50 50 0 0 1 0 100M40 40h10v10H40z\""),
            "{output}"
        );
        // Two circles 200 units apart (relative arcs used to be sampled as
        // absolute points near the origin and "intersect").
        let (result, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M50 50m-25 0a25 25 0 1 0 50 0a25 25 0 1 0-50 0\"/><path d=\"M250 50m-25 0a25 25 0 1 0 50 0a25 25 0 1 0-50 0\"/></svg>",
        );
        assert_eq!(result, PassResult::Changed, "{output}");
        // Overlapping circles still refuse.
        let (result, _) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M50 50m-25 0a25 25 0 1 0 50 0a25 25 0 1 0-50 0\"/><path d=\"M60 50m-25 0a25 25 0 1 0 50 0a25 25 0 1 0-50 0\"/></svg>",
        );
        assert_eq!(result, PassResult::Unchanged);
        // Rotated ellipse arc over 120 degrees: the split chunks must not be
        // rotated twice. Major axis vertical (rotation 90), bulge to x<=20;
        // a rect at x 30..40 is clear of it.
        let (result, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0A100 20 90 0 1 0 200\"/><path d=\"M30 90h10v20H30z\"/></svg>",
        );
        assert_eq!(result, PassResult::Changed, "{output}");
        let (result, _) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0A100 20 90 0 1 0 200z\"/><path d=\"M5 90h10v20H5z\"/></svg>",
        );
        assert_eq!(result, PassResult::Unchanged);
    }

    #[test]
    fn no_merge_with_title_or_desc_children() {
        // SVGO merges only childless paths; a11y content must not vanish.
        for child in ["<title>Left</title>", "<desc>Left shape</desc>", "<!--c-->"] {
            let input = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0h10v10H0z\">{child}</path><path d=\"M50 50h10v10H50z\"/></svg>"
            );
            let (result, output) = run_pass(&input);
            assert_eq!(result, PassResult::Unchanged, "{output}");
            assert_eq!(output.matches("<path").count(), 2, "{output}");
        }
        // Whitespace-only text (formatting) is not content.
        let (result, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0h10v10H0z\">  \n </path><path d=\"M50 50h10v10H50z\"/></svg>",
        );
        assert_eq!(result, PassResult::Changed, "{output}");
        assert_eq!(output.matches("<path").count(), 1, "{output}");
    }

    #[test]
    fn no_merge_with_style_or_inherited_url_paint() {
        let cases = [
            // style="" on the paths themselves
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"fill:url(#g)\" d=\"M0 0h10v10H0z\"/><path style=\"fill:url(#g)\" d=\"M50 50h10v10H50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"stroke:url(#g)\" d=\"M0 0h10v10H0z\"/><path style=\"stroke:url(#g)\" d=\"M50 50h10v10H50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"filter:url(#f)\" d=\"M0 0h10v10H0z\"/><path style=\"filter:url(#f)\" d=\"M50 50h10v10H50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"clip-path:url(#c)\" d=\"M0 0h10v10H0z\"/><path style=\"clip-path:url(#c)\" d=\"M50 50h10v10H50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"clip-path:circle(25%)\" d=\"M0 0h10v10H0z\"/><path style=\"clip-path:circle(25%)\" d=\"M50 50h10v10H50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"mask:url(#m)\" d=\"M0 0h10v10H0z\"/><path style=\"mask:url(#m)\" d=\"M50 50h10v10H50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"mask-image: linear-gradient(to left top,black, transparent)\" d=\"M0 0h10v10H0z\"/><path style=\"mask-image: linear-gradient(to left top,black, transparent)\" d=\"M50 50h10v10H50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"marker-mid:url(#m)\" d=\"M0 0h10v10H0z\"/><path style=\"marker-mid:url(#m)\" d=\"M50 50h10v10H50z\"/></svg>",
            // inherited from an ancestor, attribute and style forms
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g fill=\"url(#g)\"><path d=\"M0 0h10v10H0z\"/><path d=\"M50 50h10v10H50z\"/></g></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g style=\"fill:url(#g)\"><path d=\"M0 0h10v10H0z\"/><path d=\"M50 50h10v10H50z\"/></g></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g style=\"stroke:url(#g)\"><g><path d=\"M0 0h10v10H0z\"/><path d=\"M50 50h10v10H50z\"/></g></g></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g marker-start=\"url(#m)\"><path d=\"M0 0h10v10H0z\"/><path d=\"M50 50h10v10H50z\"/></g></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g style=\"clip-path:url(#c)\"><path d=\"M0 0h10v10H0z\"/><path d=\"M50 50h10v10H50z\"/></g></svg>",
        ];
        for input in cases {
            let (result, output) = run_pass(input);
            assert_eq!(result, PassResult::Unchanged, "{output}");
        }
        // `none` and a plain colour do not block; a solid fill on a group
        // is inherited identically by the merged path.
        for input in [
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"clip-path:none;fill:red\" d=\"M0 0h10v10H0z\"/><path style=\"clip-path:none;fill:red\" d=\"M50 50h10v10H50z\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g fill=\"red\" style=\"stroke:blue\"><path d=\"M0 0h10v10H0z\"/><path d=\"M50 50h10v10H50z\"/></g></svg>",
        ] {
            let (result, output) = run_pass(input);
            assert_eq!(result, PassResult::Changed, "{output}");
            assert_eq!(output.matches("<path").count(), 1, "{output}");
        }
    }

    #[test]
    fn stylesheet_declaring_blocking_props_disables_merging() {
        // SVGO mergePaths.08-12: rules in a remaining <style> may give any
        // path a url() paint, filter, clip-path or mask. Without selector
        // matching, nothing is merged in that document.
        let rules = [
            "path.lg{fill:url(#gradient);}",
            "path.lg{stroke:url(#gradient);}",
            "path.lg{filter:url(#blurMe);}",
            "path.lg{clip-path:url(#myClip);}",
            "path.lg{mask:url(#mask);}",
            "@media print{.x{marker-mid:url(#m)}}",
        ];
        for rule in rules {
            let input = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>{rule}</style><path class=\"lg\" d=\"M0 0h10v10H0z\"/><path class=\"lg\" d=\"M50 50h10v10H50z\"/><path d=\"M100 0h10v10h-10z\"/><path d=\"M150 50h10v10h-10z\"/></svg>"
            );
            let (result, output) = run_pass(&input);
            assert_eq!(result, PassResult::Unchanged, "{rule}: {output}");
            assert_eq!(output.matches("<path").count(), 4, "{rule}: {output}");
        }
        // A stylesheet that declares none of those still allows merging.
        let (result, output) = run_pass(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>path{stroke-width:2;fill:red;clip-path:none}</style><path d=\"M0 0h10v10H0z\"/><path d=\"M50 50h10v10H50z\"/></svg>",
        );
        assert_eq!(result, PassResult::Changed, "{output}");
        assert_eq!(output.matches("<path").count(), 1, "{output}");
    }

    #[test]
    fn svgo_merge_paths_fixtures_08_to_12_stay_unmerged() {
        // Full optimizer run on the SVGO fixture shapes: after inlineStyles
        // the class rules become attributes and still block.
        let fixture = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 300 300\">\
            <style>path.lg{fill:url(#gradient);}</style>\
            <linearGradient id=\"gradient\"><stop offset=\"0\" stop-color=\"#ff0000\"/><stop offset=\"1\" stop-color=\"#0000ff\"/></linearGradient>\
            <path fill=\"url(#gradient)\" d=\"M 0 0 H 100 V 80 H 0 z\"/>\
            <path fill=\"url(#gradient)\" d=\"M 200 0 H 300 V 80 H 200 z\"/>\
            <path style=\"fill:url(#gradient)\" d=\"M 0 100 h 100 v 80 H 0 z\"/>\
            <path style=\"fill:url(#gradient)\" d=\"M 200 100 H 300 v 80 H 200 z\"/>\
            <path class=\"lg\" d=\"M 0 200 h 100 v 80 H 0 z\"/>\
            <path class=\"lg\" d=\"M 200 200 H 300 v 80 H 200 z\"/>\
            </svg>";
        let out = crate::optimize(fixture).unwrap().data;
        assert_eq!(out.matches("<path").count(), 6, "{out}");
        let fixture = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-5 -5 400 400\">\
            <style>path.lg{mask:url(#mask);}</style>\
            <mask id=\"mask\" maskContentUnits=\"objectBoundingBox\"><rect fill=\"white\" x=\"0\" y=\"0\" width=\"100%\" height=\"100%\"/><circle fill=\"black\" cx=\".5\" cy=\".5\" r=\".5\"/></mask>\
            <path mask=\"url(#mask)\" fill=\"red\" d=\"M 0 0 H 100 V 80 H 0 z\"/>\
            <path mask=\"url(#mask)\" fill=\"red\" d=\"M 200 0 H 300 V 80 H 200 z\"/>\
            <path style=\"mask:url(#mask)\" fill=\"red\" d=\"M 0 100 h 100 v 80 H 0 z\"/>\
            <path style=\"mask:url(#mask)\" fill=\"red\" d=\"M 200 100 H 300 v 80 H 200 z\"/>\
            <path class=\"lg\" fill=\"red\" d=\"M 0 200 h 100 v 80 H 0 z\"/>\
            <path class=\"lg\" fill=\"red\" d=\"M 200 200 H 300 v 80 H 200 z\"/>\
            <path style=\"mask-image: linear-gradient(to left top,black, transparent)\" fill=\"red\" d=\"M 0 300 h 100 v 80 H 0 z\"/>\
            <path style=\"mask-image: linear-gradient(to left top,black, transparent)\" fill=\"red\" d=\"M 200 300 H 300 v 80 H 200 z\"/>\
            </svg>";
        let out = crate::optimize(fixture).unwrap().data;
        assert_eq!(out.matches("<path").count(), 8, "{out}");
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
            let result = MergePaths::default().run(&mut doc);
            assert_eq!(result, PassResult::Unchanged, "{bad}");
            assert_eq!(crate::serializer::serialize(&doc), input);
        }
    }

    // --- Stroke extent (#49) ---

    /// Two paths sharing `attrs`, wrapped in an `<svg>`.
    fn pair(d1: &str, d2: &str, attrs: &str) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"{d1}\" {attrs}/><path d=\"{d2}\" {attrs}/></svg>"
        )
    }

    /// A 10x10 square at `x` and one at `x2`.
    fn squares(x2: f64, attrs: &str) -> String {
        pair("M0 0h10v10H0z", &format!("M{x2} 0h10v10H{x2}z"), attrs)
    }

    fn assert_merged(input: &str) {
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed, "{input}");
        assert_eq!(output.matches("<path").count(), 1, "{output}");
    }

    fn assert_kept(input: &str) {
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Unchanged, "{input}");
        assert_eq!(output, input);
    }

    const TOUCHING_D1: &str = "M25.5 25.5a8 8 0 0 1 8 8v2h-16v-2a8 8 0 0 1 8-8z";
    const TOUCHING_D2: &str = "M25.5 25.5a8 8 0 0 0 8-8v-2h-16v2a8 8 0 0 0 8 8z";

    #[test]
    fn stroked_shapes_that_touch_are_not_merged() {
        // Issue #49 repro: the shapes touch at (25.5, 25.5); merged, both
        // fills are painted before both strokes, which changes the render.
        assert_kept(&pair(
            TOUCHING_D1,
            TOUCHING_D2,
            "fill=\"red\" stroke=\"gray\" fill-rule=\"evenodd\"",
        ));
    }

    #[test]
    fn unstroked_or_zero_width_touching_shapes_still_merge() {
        for attrs in [
            "fill=\"red\" fill-rule=\"evenodd\"",
            "fill=\"red\" stroke=\"gray\" stroke-width=\"0\"",
            "fill=\"red\" stroke=\"none\" stroke-width=\"5\"",
            "fill=\"red\" stroke=\"NONE\"",
        ] {
            assert_merged(&pair(TOUCHING_D1, TOUCHING_D2, attrs));
        }
    }

    #[test]
    fn far_apart_stroked_shapes_still_merge() {
        for attrs in [
            "fill=\"red\" stroke=\"blue\" stroke-width=\"2\"",
            "fill=\"red\" stroke=\"blue\" stroke-width=\"2px\"",
            "fill=\"red\" style=\"stroke:blue;stroke-width:2PX\"",
        ] {
            assert_merged(&squares(110.0, attrs));
        }
    }

    #[test]
    fn default_miter_join_reaches_four_half_widths() {
        // width 4, miterlimit 4: margin 8 per side, so a 1-unit gap overlaps.
        assert_kept(&squares(
            11.0,
            "fill=\"red\" stroke=\"blue\" stroke-width=\"4\"",
        ));
        // A gap of 17 > 2 * 8 is safe.
        assert_merged(&squares(
            27.0,
            "fill=\"red\" stroke=\"blue\" stroke-width=\"4\"",
        ));
    }

    #[test]
    fn round_and_bevel_joins_reach_half_the_width() {
        for join in ["round", "bevel"] {
            let attrs = format!(
                "fill=\"red\" stroke=\"blue\" stroke-width=\"4\" stroke-linejoin=\"{join}\""
            );
            // Each stroke reaches 2 past its square: gap 5 is clear, 4 touches,
            // 3 overlaps.
            assert_merged(&squares(15.0, &attrs));
            assert_kept(&squares(14.0, &attrs));
            assert_kept(&squares(13.0, &attrs));
        }
    }

    #[test]
    fn miterlimit_scales_the_margin() {
        let base = "fill=\"red\" stroke=\"blue\" stroke-width=\"2\"";
        assert_kept(&squares(13.0, base));
        assert_merged(&squares(13.0, &format!("{base} stroke-miterlimit=\"1\"")));
        for bad in [".5", "x"] {
            assert_kept(&squares(
                13.0,
                &format!("{base} stroke-miterlimit=\"{bad}\""),
            ));
        }
    }

    #[test]
    fn overlapping_collinear_stroked_lines_are_not_merged() {
        // Zero-height boxes and two-point hulls hid this overlap entirely.
        assert_kept(&pair(
            "M0 0H10",
            "M5 0H15",
            "stroke=\"red\" stroke-opacity=\".5\"",
        ));
    }

    #[test]
    fn square_caps_reach_further_than_round_joins() {
        let attrs = |cap: &str| {
            format!(
                "fill=\"none\" stroke=\"red\" stroke-opacity=\".5\" stroke-width=\"2\" stroke-linejoin=\"round\" stroke-linecap=\"{cap}\""
            )
        };
        // Boxes 2.5 apart: butt caps reach 1 each, square caps sqrt(2) each.
        assert_kept(&pair("M0 0L10 10", "M22.5 0L12.5 10", &attrs("square")));
        assert_merged(&pair("M0 0L10 10", "M22.5 0L12.5 10", &attrs("butt")));
    }

    #[test]
    fn unknown_stroke_widths_are_not_merged() {
        for width in ["1em", "50%", "calc(1px + 1px)", "-1"] {
            assert_kept(&squares(
                110.0,
                &format!("fill=\"red\" stroke=\"blue\" stroke-width=\"{width}\""),
            ));
        }
    }

    #[test]
    fn inherited_stroke_is_honoured() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g stroke=\"gray\" stroke-width=\"4\" stroke-linejoin=\"round\"><path d=\"M0 0h10v10H0z\" fill=\"red\"/><path d=\"M13 0h10v10H13z\" fill=\"red\"/></g></svg>";
        assert_kept(input);
        let input = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g style=\"stroke:gray\"><path d=\"{TOUCHING_D1}\" fill=\"red\"/><path d=\"{TOUCHING_D2}\" fill=\"red\"/></g></svg>"
        );
        assert_kept(&input);
    }

    #[test]
    fn non_scaling_stroke_is_not_merged() {
        assert_kept(&squares(
            110.0,
            "fill=\"red\" stroke=\"blue\" vector-effect=\"non-scaling-stroke\"",
        ));
        // Without a stroke the vector effect changes nothing.
        assert_merged(&squares(
            110.0,
            "fill=\"red\" vector-effect=\"non-scaling-stroke\"",
        ));
    }

    #[test]
    fn stylesheet_stroke_is_honoured() {
        let sheet = |css: &str, d1: &str, d2: &str, attrs: &str| {
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>{css}</style><path d=\"{d1}\" {attrs}/><path d=\"{d2}\" {attrs}/></svg>"
            )
        };
        assert_kept(&sheet(
            ".a{stroke:gray}",
            TOUCHING_D1,
            TOUCHING_D2,
            "class=\"a\" fill=\"red\"",
        ));
        assert_merged(&sheet(
            ".a{stroke:gray}",
            "M0 0h10v10H0z",
            "M110 0h10v10h-10z",
            "class=\"a\" fill=\"red\"",
        ));
        // The sheet may change the width of any path: margin unknown.
        assert_kept(&sheet(
            ".x{stroke-width:2}",
            "M0 0h10v10H0z",
            "M110 0h10v10h-10z",
            "fill=\"red\" stroke=\"blue\"",
        ));
    }

    #[test]
    fn accumulated_path_is_tested_with_the_margin() {
        // margin 4: the first two merge (90 apart); the third is 4 from the
        // first square, inside the accumulated path's reach.
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0h10v10H0z\" fill=\"red\" stroke=\"blue\" stroke-width=\"2\"/><path d=\"M100 0h10v10h-10z\" fill=\"red\" stroke=\"blue\" stroke-width=\"2\"/><path d=\"M14 0h10v10H14z\" fill=\"red\" stroke=\"blue\" stroke-width=\"2\"/></svg>";
        let (result, output) = run_pass(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(output.matches("<path").count(), 2, "{output}");
    }

    #[test]
    fn intersects_with_margin_bounds() {
        let a = gather_points("M0 0h10v10H0z").unwrap();
        let b = gather_points("M13 0h10v10H13z").unwrap();
        // Gap 3: touching at margin 1.5 counts as overlapping.
        assert!(intersects_with_margin(&a, &b, 1.5, 1e-3));
        assert!(!intersects_with_margin(&a, &b, 1.4, 1e-3));
        assert_eq!(
            intersects_with_margin(&a, &b, 0.0, 1e-3),
            intersects(&a, &b)
        );
        assert!(!intersects(&a, &b));
    }
}
