pub mod cleanup_attrs;
pub mod cleanup_enable_background;
pub mod cleanup_ids;
pub mod cleanup_numeric_values;
pub mod collapse_groups;
pub mod convert_colors;
pub mod convert_ellipse_to_circle;
pub mod convert_path_data;
pub mod convert_shape_to_path;
pub mod convert_transform;
pub mod inline_styles;
pub mod merge_paths;
pub mod minify_styles;
pub mod minify_whitespace;
pub mod move_elems_attrs_to_group;
pub mod move_group_attrs_to_elems;
pub mod remove_comments;
pub mod remove_deprecated_attrs;
pub mod remove_desc;
pub mod remove_doctype;
pub mod remove_editor_data;
pub mod remove_empty_attrs;
pub mod remove_empty_containers;
pub mod remove_empty_text;
pub mod remove_hidden_elems;
pub mod remove_metadata;
pub mod remove_non_inheritable_group_attrs;
pub mod remove_proc_inst;
pub mod remove_unknowns_and_defaults;
pub mod remove_unused_namespaces;
pub mod remove_useless_defs;
pub mod remove_useless_stroke_and_fill;
pub mod sort_attrs;
pub mod sort_defs_children;

mod util;
pub(crate) use util::*;

use crate::ast::Document;

/// Result of running a single optimization pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassResult {
    Changed,
    Unchanged,
}

impl PassResult {
    pub fn changed(self) -> bool {
        self == PassResult::Changed
    }
}

/// An optimization pass that transforms a Document in place.
pub trait Pass {
    fn name(&self) -> &'static str;
    fn run(&self, doc: &mut Document) -> PassResult;
}

/// Parse a number using the SVG `<number>` grammar only.
///
/// Accepts an optional sign, digits with an optional fraction (or a bare
/// fraction like `.5`), and an optional exponent. Rejects anything Rust's
/// `f64::from_str` would accept beyond that (`inf`, `nan`, `infinity`, hex,
/// surrounding garbage) and rejects values that overflow to infinity
/// (`1e999`), so callers never see non-finite numbers.
pub(crate) fn parse_svg_number(s: &str) -> Option<f64> {
    // No trimming: callers pass already-trimmed values, and `5 px` with the
    // unit stripped must stay invalid rather than being "repaired" to 5.
    let t = s;
    let b = t.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut i = 0;
    if b[i] == b'+' || b[i] == b'-' {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut has_digits = i > int_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        has_digits |= i > frac_start;
    }
    if !has_digits {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Largest magnitude the rounding passes will rewrite. Rust formats floats
/// positionally, so `1e300` would become a 300-digit expansion; real SVG
/// coordinates never get anywhere near this.
const MAX_MAGNITUDE: f64 = 1e15;

/// True when `v` can be rounded to `precision` decimals and printed
/// compactly: finite, below [`MAX_MAGNITUDE`], and `v * 10^precision` does
/// not overflow. Values that fail this must be left untouched by every pass.
pub(crate) fn representable(v: f64, precision: u32) -> bool {
    v.is_finite() && v.abs() < MAX_MAGNITUDE && (v * 10f64.powi(precision as i32)).is_finite()
}

/// Returns the default set of passes in execution order (Default preset).
pub fn default_passes() -> Vec<Box<dyn Pass>> {
    crate::config::passes_for_config(&crate::config::Config::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_svg_number_follows_svg_grammar() {
        for (input, expected) in [
            ("1", 1.0),
            (".5", 0.5),
            ("-.5", -0.5),
            ("1.", 1.0),
            ("+3", 3.0),
            ("-1.5e3", -1500.0),
            ("1E-2", 0.01),
            ("1e308", 1e308),
        ] {
            assert_eq!(parse_svg_number(input), Some(expected), "{input:?}");
        }
        for input in [
            "", " ", " 2 ", "5 ", "1e999", "-1e999", "inf", "-inf", "infinity", "nan", "NaN", "1e",
            "1e+", "e5", "1.2.3", "0x10", "1px", "1 2", "--1", ".", "-", "+", "1,5", "١",
        ] {
            assert_eq!(parse_svg_number(input), None, "{input:?}");
        }
    }

    #[test]
    fn representable_rejects_rounding_overflow() {
        assert!(representable(1.0, 3));
        assert!(representable(123456789.123, 3));
        assert!(!representable(1e15, 3));
        assert!(!representable(1e300, 3));
        assert!(!representable(1e308, 3));
        assert!(!representable(1e300, 20));
        assert!(!representable(f64::INFINITY, 0));
        assert!(!representable(f64::NAN, 0));
    }
}
