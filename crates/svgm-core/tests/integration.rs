use std::fs;
use std::path::Path;

fn optimize_file(path: &Path) -> (String, usize, usize) {
    let input = fs::read_to_string(path).unwrap();
    let input_size = input.len();
    let result = svgm_core::optimize(&input).unwrap();
    let output_size = result.data.len();
    (result.data, input_size, output_size)
}

fn assert_valid_svg(output: &str, source: &str) {
    // Must start with <svg
    assert!(
        output.trim_start().starts_with("<svg"),
        "{source}: output does not start with <svg"
    );
    // Must end with </svg>
    assert!(
        output.trim_end().ends_with("</svg>"),
        "{source}: output does not end with </svg>"
    );
    // Must be parseable
    svgm_core::parser::parse(output)
        .unwrap_or_else(|e| panic!("{source}: output is not valid XML: {e}"));
}

// ── Synthetic fixtures ──────────────────────────────────────────────────

#[test]
fn synthetic_comments_and_metadata() {
    let (output, input_size, output_size) =
        optimize_file(Path::new("tests/fixtures/synthetic/comments_and_metadata.svg"));
    assert_valid_svg(&output, "comments_and_metadata");

    // Should remove comments
    assert!(!output.contains("<!--"), "comments should be removed");
    // Should remove metadata
    assert!(!output.contains("<metadata"), "metadata should be removed");
    // Should remove inkscape/sodipodi elements and attrs
    assert!(!output.contains("inkscape"), "inkscape data should be removed");
    assert!(!output.contains("sodipodi"), "sodipodi data should be removed");
    // Should remove empty containers
    assert!(
        !output.contains("><g></g>"),
        "empty <g> should be removed"
    );
    // Should convert colors
    assert!(output.contains("fill=\"red\"") || output.contains("fill=\"#f00\""));
    // Should clean numeric values
    assert!(!output.contains("50.000"), "trailing zeros should be removed");
    // Should remove empty attrs
    assert!(!output.contains("class=\"\""), "empty class should be removed");
    // Should be significantly smaller
    assert!(
        output_size < input_size * 80 / 100,
        "should be at least 20% smaller: {input_size} -> {output_size}"
    );
}

#[test]
fn synthetic_nested_empty_groups() {
    let (output, _, _) =
        optimize_file(Path::new("tests/fixtures/synthetic/nested_empty_groups.svg"));
    assert_valid_svg(&output, "nested_empty_groups");
    // All <g> elements should be collapsed (empty ones removed, single-child ones unwrapped)
    assert!(output.contains("<rect"), "rect should be preserved");
    let g_count = output.matches("<g>").count() + output.matches("<g ").count();
    assert_eq!(g_count, 0, "all groups should be collapsed, found {g_count}");
}

#[test]
fn synthetic_colors_and_numbers() {
    let (output, _, _) =
        optimize_file(Path::new("tests/fixtures/synthetic/colors_and_numbers.svg"));
    assert_valid_svg(&output, "colors_and_numbers");

    // px units should be removed from numeric attrs
    assert!(!output.contains("500.000px"), "should strip px units");
    assert!(!output.contains("500.000"), "should strip trailing zeros");
    // rgb() should be converted to hex
    assert!(!output.contains("rgb("), "rgb() should be converted");
    // #ffffff should be shortened
    assert!(output.contains("#fff") || output.contains("white"));
    // #000000 should be shortened
    assert!(output.contains("#000") || output.contains("black"));
}

#[test]
fn synthetic_empty_text_elements() {
    let (output, _, _) =
        optimize_file(Path::new("tests/fixtures/synthetic/empty_text_elements.svg"));
    assert_valid_svg(&output, "empty_text_elements");
    // Should keep text with content
    assert!(output.contains("Hello world"));
    // Empty text elements should be gone (those without meaningful text children)
}

#[test]
fn synthetic_preserves_animation() {
    let (output, _, _) =
        optimize_file(Path::new("tests/fixtures/synthetic/preserves_animation.svg"));
    assert_valid_svg(&output, "preserves_animation");
    // Animation elements must be preserved
    assert!(output.contains("<animate"), "animate should be preserved");
    assert!(
        output.contains("<animateTransform"),
        "animateTransform should be preserved"
    );
}

// ── Regression fixtures ─────────────────────────────────────────────────

#[test]
fn regression_symbol_use_ref() {
    let (output, _, _) =
        optimize_file(Path::new("tests/fixtures/regression/symbol_use_ref.svg"));
    assert_valid_svg(&output, "symbol_use_ref");
    // Symbol must be preserved because it's referenced by <use>
    assert!(output.contains("<symbol"), "symbol should be preserved");
    assert!(output.contains("<use"), "use should be preserved");
    assert!(output.contains("#icon"), "icon reference should be preserved");
}

#[test]
fn regression_foreign_object() {
    let (output, _, _) =
        optimize_file(Path::new("tests/fixtures/regression/foreign_object.svg"));
    assert_valid_svg(&output, "foreign_object");
    // foreignObject content must be preserved
    assert!(
        output.contains("<foreignObject"),
        "foreignObject should be preserved"
    );
    assert!(output.contains("Hello world"));
}

// ── Real editor exports ─────────────────────────────────────────────────

#[test]
fn real_svgs_parse_and_optimize() {
    let fixture_dir = Path::new("tests/fixtures/real");
    if !fixture_dir.exists() {
        return;
    }
    for entry in fs::read_dir(fixture_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |e| e == "svg") {
            let (output, input_size, output_size) = optimize_file(&path);
            let name = path.file_name().unwrap().to_str().unwrap();
            assert_valid_svg(&output, name);
            // Output should not be larger than input
            assert!(
                output_size <= input_size,
                "{name}: output ({output_size}) larger than input ({input_size})"
            );
            // Roundtrip: optimizing the output again should produce identical result
            let result2 = svgm_core::optimize(&output).unwrap();
            assert_eq!(
                output, result2.data,
                "{name}: second optimization produced different output (not converged)"
            );
        }
    }
}
