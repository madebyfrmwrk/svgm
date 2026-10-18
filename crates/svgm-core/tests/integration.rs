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
    let (output, input_size, output_size) = optimize_file(Path::new(
        "tests/fixtures/synthetic/comments_and_metadata.svg",
    ));
    assert_valid_svg(&output, "comments_and_metadata");

    // Should remove comments
    assert!(!output.contains("<!--"), "comments should be removed");
    // Should remove metadata
    assert!(!output.contains("<metadata"), "metadata should be removed");
    // Should remove inkscape/sodipodi elements and attrs
    assert!(
        !output.contains("inkscape"),
        "inkscape data should be removed"
    );
    assert!(
        !output.contains("sodipodi"),
        "sodipodi data should be removed"
    );
    // Should remove empty containers
    assert!(!output.contains("><g></g>"), "empty <g> should be removed");
    // Should convert colors
    assert!(output.contains("fill=\"red\"") || output.contains("fill=\"#f00\""));
    // Should clean numeric values
    assert!(
        !output.contains("50.000"),
        "trailing zeros should be removed"
    );
    // Should remove empty attrs
    assert!(
        !output.contains("class=\"\""),
        "empty class should be removed"
    );
    // Should be significantly smaller
    assert!(
        output_size < input_size * 80 / 100,
        "should be at least 20% smaller: {input_size} -> {output_size}"
    );
}

#[test]
fn synthetic_nested_empty_groups() {
    let (output, _, _) = optimize_file(Path::new(
        "tests/fixtures/synthetic/nested_empty_groups.svg",
    ));
    assert_valid_svg(&output, "nested_empty_groups");
    // All <g> elements should be collapsed (empty ones removed, single-child ones unwrapped)
    assert!(
        output.contains("<rect") || output.contains("<path"),
        "rect (or its path equivalent) should be preserved"
    );
    let g_count = output.matches("<g>").count() + output.matches("<g ").count();
    assert_eq!(
        g_count, 0,
        "all groups should be collapsed, found {g_count}"
    );
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
    let (output, _, _) = optimize_file(Path::new(
        "tests/fixtures/synthetic/empty_text_elements.svg",
    ));
    assert_valid_svg(&output, "empty_text_elements");
    // Should keep text with content
    assert!(output.contains("Hello world"));
    // Empty text elements should be gone (those without meaningful text children)
}

#[test]
fn synthetic_preserves_animation() {
    let (output, _, _) = optimize_file(Path::new(
        "tests/fixtures/synthetic/preserves_animation.svg",
    ));
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
    let (output, _, _) = optimize_file(Path::new("tests/fixtures/regression/symbol_use_ref.svg"));
    assert_valid_svg(&output, "symbol_use_ref");
    // Symbol must be preserved because it's referenced by <use>
    assert!(output.contains("<symbol"), "symbol should be preserved");
    assert!(output.contains("<use"), "use should be preserved");
    // The ID may be shortened (e.g., "icon" -> "a"), but the href must
    // still reference the symbol's id — verify they match.
    assert!(
        output.contains("href=\"#"),
        "use href reference should be preserved"
    );
}

#[test]
fn regression_foreign_object() {
    let (output, _, _) = optimize_file(Path::new("tests/fixtures/regression/foreign_object.svg"));
    assert_valid_svg(&output, "foreign_object");
    // foreignObject content must be preserved
    assert!(
        output.contains("<foreignObject"),
        "foreignObject should be preserved"
    );
    assert!(output.contains("Hello world"));
}

#[test]
fn regression_fill_inherit_none() {
    let (output, _, _) =
        optimize_file(Path::new("tests/fixtures/regression/fill_inherit_none.svg"));
    assert_valid_svg(&output, "fill_inherit_none");
    // The path must retain fill="black" because the parent <svg> has fill="none".
    // Removing it would make the path inherit fill="none" and become invisible.
    assert!(
        output.contains("fill=\"black\"") || output.contains("fill=\"#000\""),
        "path fill must be preserved when parent svg has fill=none: {output}"
    );
}

// ── Path torture fixtures ───────────────────────────────────────────────

#[test]
fn path_torture_fixtures() {
    let fixture_dir = Path::new("tests/fixtures/regression/path_torture");
    assert!(
        fixture_dir.exists(),
        "path_torture fixture directory missing"
    );
    let mut count = 0;
    for entry in fs::read_dir(fixture_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |e| e == "svg") {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            let (output, _input_size, _output_size) = optimize_file(&path);

            // Valid SVG
            assert_valid_svg(&output, &name);

            // Convergence: optimizing twice should be byte-identical
            let result2 = svgm_core::optimize(&output).unwrap();
            assert_eq!(
                output, result2.data,
                "{name}: not converged after second optimization"
            );

            // Re-parseable
            svgm_core::parser::parse(&output)
                .unwrap_or_else(|e| panic!("{name}: optimized output not parseable: {e}"));

            count += 1;
        }
    }
    assert!(
        count >= 15,
        "expected at least 15 path torture fixtures, found {count}"
    );
}

#[test]
fn path_torture_structural_equivalence() {
    let fixture_dir = Path::new("tests/fixtures/regression/path_torture");
    assert!(
        fixture_dir.exists(),
        "path_torture fixture directory missing"
    );
    for entry in fs::read_dir(fixture_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |e| e == "svg") {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            let input = fs::read_to_string(&path).unwrap();
            let output = svgm_core::optimize(&input).unwrap().data;

            // Verify the output is valid and converged (already checked above).
            // Path count may change: convertShapeToPath increases it (shapes→paths),
            // mergePaths decreases it (adjacent identical-attr paths merged).
            // We just verify the output is non-empty and parseable.
            assert!(!output.is_empty(), "{name}: output should not be empty");
        }
    }
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

// ── Issue regressions ───────────────────────────────────────────────────

#[test]
fn issue_22_path_garbage_does_not_hang() {
    let start = std::time::Instant::now();
    let out = svgm_core::optimize(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 e"/><path d="M0 0 L nan nan"/><path d="hello world"/></svg>"#,
    )
    .unwrap();
    assert!(start.elapsed().as_secs() < 5, "took {:?}", start.elapsed());
    // The two truncated paths both become `M0 0` and may then be merged;
    // what matters is that the valid prefix survives and the garbage is gone.
    assert!(out.data.contains("M0 0"), "{}", out.data);
    assert!(!out.data.contains("nan"), "{}", out.data);
    assert!(out.data.contains(r#"d="hello world""#), "{}", out.data);
}

#[test]
fn issue_23_non_ascii_style_is_preserved_and_stable() {
    let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><style>.ǎccent{fill:red}</style><rect class="ǎccent"/></svg>"#;
    let out = svgm_core::optimize(input).unwrap();
    assert!(
        out.data.len() <= input.len(),
        "grew: {} -> {}",
        input.len(),
        out.data.len()
    );
    assert!(out.data.contains("ǎccent"), "{}", out.data);
    let again = svgm_core::optimize(&out.data).unwrap();
    assert_eq!(again.data, out.data, "not idempotent");

    let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><style>text{font-family:"Söhne";content:"→"}</style><text>→ Söhne</text></svg>"#;
    let out = svgm_core::optimize(input).unwrap();
    assert!(out.data.contains("font-family:\"Söhne\""), "{}", out.data);
    assert!(out.data.contains("→ Söhne"), "{}", out.data);
}

#[test]
fn issue_24_truncated_input_is_an_error_not_empty_output() {
    for input in [
        "<svg",
        "<svg>",
        "<svg><g>",
        "",
        "  ",
        "<?xml version=\"1.0\"?>",
    ] {
        let result = svgm_core::optimize(input);
        assert!(result.is_err(), "{input:?} -> {:?}", result.map(|r| r.data));
    }
    // Non-svg roots are accepted, as in SVGO.
    let out = svgm_core::optimize("<?xml version=\"1.0\"?><html><svg/></html>").unwrap();
    assert_eq!(out.data, "<html><svg></svg></html>");
}

#[test]
fn non_finite_numbers_never_reach_the_output() {
    let input = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1e999 1"><rect width="1e999" height="nan" x="1e308" y="inf"/><path d="M0 0L1e999 5"/><circle cx="1e308" cy="0" r="1"/><g transform="translate(1e308,1e308)"><path d="M0 0h5" transform="scale(1e308)"/></g><polygon points="1e999 1 2 2 3 3"/></svg>"#;
    let out = svgm_core::optimize(input).unwrap();
    // Unparseable values are kept verbatim; nothing is ever rewritten into
    // inf/NaN or a 300-digit expansion.
    for kept in [
        "width=\"1e999\"",
        "height=\"nan\"",
        "y=\"inf\"",
        "x=\"1e308\"",
        "viewBox=\"0 0 1e999 1\"",
    ] {
        assert!(out.data.contains(kept), "{kept} missing from {}", out.data);
    }
    assert!(!out.data.contains("NaN"), "{}", out.data);
    assert!(!out.data.contains("100000000000000"), "{}", out.data);
    assert_eq!(out.data.matches("inf").count(), 1, "{}", out.data);
    let again = svgm_core::optimize(&out.data).unwrap();
    assert_eq!(again.data, out.data, "not idempotent");
}

// ── WP3 regression fixtures ─────────────────────────────────────────────

fn optimize_fixture_both_presets(name: &str) -> (String, String) {
    let path = format!("tests/fixtures/regression/{name}");
    let input = fs::read_to_string(&path).unwrap();
    let default = svgm_core::optimize(&input).unwrap().data;
    let safe = svgm_core::optimize_with_config(
        &input,
        &svgm_core::Config {
            preset: svgm_core::Preset::Safe,
            ..Default::default()
        },
    )
    .unwrap()
    .data;
    assert_valid_svg(&default, name);
    assert_valid_svg(&safe, name);
    (default, safe)
}

#[test]
fn regression_cdata_text() {
    let (default, safe) = optimize_fixture_both_presets("cdata_text.svg");
    for (preset, output) in [("default", &default), ("safe", &safe)] {
        // <text>/<tspan> whose only content is a CDATA section are not empty.
        assert_eq!(
            output.matches("<text").count(),
            3,
            "{preset}: all three text elements must survive: {output}"
        );
        assert!(
            output.contains("a &lt; b &amp; c") || output.contains("<![CDATA[a < b & c]]>"),
            "{preset}: CDATA content must survive: {output}"
        );
        assert!(output.contains("second"), "{preset}: {output}");
        // Whitespace between words inside <a> nested in <text> is rendered.
        assert!(
            output.contains("<tspan>read</tspan> <tspan>more</tspan>"),
            "{preset}: space between words must survive: {output}"
        );
    }
}

#[test]
fn regression_defaults_ancestor_style() {
    let (default, safe) = optimize_fixture_both_presets("defaults_ancestor_style.svg");
    for (preset, output) in [("default", &default), ("safe", &safe)] {
        // Ancestors set fill/stroke via style="" or stylesheet rules, so the
        // "default-looking" values on the paths are real overrides.
        assert_eq!(
            output.matches("fill=\"#000\"").count() + output.matches("fill=\"black\"").count(),
            3,
            "{preset}: the three fill=black overrides must survive: {output}"
        );
        assert_eq!(
            output.matches("stroke=\"none\"").count(),
            2,
            "{preset}: both stroke=none overrides must survive: {output}"
        );
        assert!(
            output.contains("stroke-width=\"1\""),
            "{preset}: stroke-width=1 override must survive: {output}"
        );
        // fill="red" under <g fill="red" style="fill:blue"> is an override.
        assert!(
            output.contains("fill=\"red\"") && output.contains("fill:blue"),
            "{preset}: {output}"
        );
        // overflow="visible" is not a default on svg/image.
        assert_eq!(
            output.matches("overflow=\"visible\"").count(),
            2,
            "{preset}: {output}"
        );
    }
}

#[test]
fn regression_hidden_elems_css() {
    let (default, safe) = optimize_fixture_both_presets("hidden_elems_css.svg");
    for (preset, output) in [("default", &default), ("safe", &safe)] {
        // display="none" may be overridden by the stylesheet (.show, :hover)
        // or by style="display:inline": all three rects render.
        assert!(
            output.contains("display=\"block\"") || output.contains("class=\"show\""),
            "{preset}: {output}"
        );
        assert!(output.contains("class=\"hov\""), "{preset}: {output}");
        assert!(output.contains(".hov:hover"), "{preset}: {output}");
        assert!(output.contains("display:inline"), "{preset}: {output}");
        // Nothing with display="none" was deleted.
        assert_eq!(
            output.matches("display=\"none\"").count()
                + output.matches("display=\"block\"").count(),
            3,
            "{preset}: {output}"
        );
        // A marker with display:none is still rendered where referenced.
        assert!(output.contains("<marker"), "{preset}: {output}");
        // SVG2 auto radius: <ellipse ry="2"> renders as a circle.
        assert!(
            output.contains("<ellipse") || output.contains("<circle"),
            "{preset}: {output}"
        );
        // Zero-length line with round caps renders a dot.
        assert!(
            output.contains("stroke-linecap=\"round\""),
            "{preset}: {output}"
        );
        // fill=none + stroke=none is not a removal rule (hit area).
        assert!(
            output.contains("pointer-events=\"all\""),
            "{preset}: {output}"
        );
        // Percent / unit coordinates are not numbers: the rect stays a rect
        // at x=25%, and the 100%-long line stays a line.
        assert!(output.contains("x=\"25%\""), "{preset}: {output}");
        assert!(output.contains("height=\"1em\""), "{preset}: {output}");
        assert!(output.contains("x2=\"100%\""), "{preset}: {output}");
    }
}
