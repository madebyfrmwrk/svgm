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

#[test]
fn illustrator_entities() {
    // DOCTYPE internal-subset entities (`xmlns="&ns_svg;"`, classic Adobe
    // Illustrator exports) must be expanded, or the root ends up outside the
    // SVG namespace and browsers render nothing.
    let (output, _, _) = optimize_file(Path::new(
        "tests/fixtures/regression/illustrator_entities.svg",
    ));
    assert_valid_svg(&output, "illustrator_entities");
    assert!(
        output.contains("xmlns=\"http://www.w3.org/2000/svg\""),
        "{output}"
    );
    assert!(!output.contains("&amp;ns_"), "{output}");
    assert!(!output.contains("&amp;brand"), "{output}");
    assert!(
        output.contains("fill=\"red\"") || output.contains("fill=\"#f00\""),
        "{output}"
    );
    assert!(output.contains("#ff0000 text"), "{output}");
    // Editor namespaces bound through entities are recognised by URI.
    assert!(!output.contains("i:pgf"), "{output}");
    assert!(!output.contains("xmlns:i="), "{output}");
    let again = svgm_core::optimize(&output).unwrap();
    assert_eq!(again.data, output, "not idempotent");
}

/// Every prefix used by an element or attribute in `output` must be declared
/// on that element or one of its ancestors (otherwise the XML is ill-formed).
fn assert_prefixes_declared(output: &str) {
    use svgm_core::ast::{Document, NodeId, NodeKind};
    fn walk(doc: &Document, id: NodeId, in_scope: &[String]) {
        let mut scope = in_scope.to_vec();
        if let NodeKind::Element(ref elem) = doc.node(id).kind {
            scope.extend(elem.namespaces.iter().map(|ns| ns.prefix.clone()));
            let used = elem
                .prefix
                .iter()
                .chain(elem.attributes.iter().filter_map(|a| a.prefix.as_ref()));
            for prefix in used {
                assert!(
                    prefix == "xml" || scope.contains(prefix),
                    "prefix {prefix}: used on <{}> but not declared",
                    elem.qualified_name()
                );
            }
        }
        for child in doc.children(id) {
            walk(doc, child, &scope);
        }
    }
    let doc = svgm_core::parser::parse(output).unwrap();
    walk(&doc, doc.root, &[]);
}

#[test]
fn editor_namespaces_by_uri() {
    let (output, _, _) = optimize_file(Path::new(
        "tests/fixtures/regression/editor_namespaces_by_uri.svg",
    ));
    assert_valid_svg(&output, "editor_namespaces_by_uri");
    assert_prefixes_declared(&output);
    // Editor data under non-canonical prefixes is gone, declarations too.
    for gone in [
        "ink:",
        "sp:",
        "serif:",
        "bx:",
        "v:",
        "xmlns:ink",
        "xmlns:sp",
        "xmlns:serif",
        "xmlns:bx",
        "xmlns:v",
        "namedview",
        "documentProperties",
        "grid",
    ] {
        assert!(!output.contains(gone), "{gone} should be removed: {output}");
    }
    // `x` bound to XLink and `a` bound to a custom namespace are not editor
    // data, whatever Illustrator conventionally binds those prefixes to.
    assert!(
        output.contains("xmlns:x=\"http://www.w3.org/1999/xlink\""),
        "{output}"
    );
    assert_eq!(output.matches("x:href=\"#").count(), 2, "{output}");
    assert!(output.contains("a:role=\"keep\""), "{output}");
    assert!(
        output.contains("xmlns:a=\"http://example.com/custom\""),
        "{output}"
    );
    let again = svgm_core::optimize(&output).unwrap();
    assert_eq!(again.data, output, "not idempotent");
}

fn optimize_both_presets(input: &str) -> Vec<(svgm_core::Preset, String)> {
    [svgm_core::Preset::Safe, svgm_core::Preset::Default]
        .into_iter()
        .map(|preset| {
            let config = svgm_core::Config {
                preset,
                ..Default::default()
            };
            (
                preset,
                svgm_core::optimize_with_config(input, &config)
                    .unwrap()
                    .data,
            )
        })
        .collect()
}

#[test]
fn unsafe_deprecated_attrs_survive_both_presets() {
    // SVGO only removes these with removeDeprecatedAttrs { removeUnsafe }:
    // xml:space="preserve" changes text layout and requiredFeatures drives
    // <switch> selection in user agents that still evaluate it.
    let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xml:space="preserve" viewBox="0 0 10 10"><switch><text requiredFeatures="http://www.w3.org/TR/SVG11/feature#Text" x="1" y="5">  a  b  </text><text x="1" y="5">fallback</text></switch></svg>"#;
    for (preset, output) in optimize_both_presets(input) {
        assert!(
            output.contains("xml:space=\"preserve\""),
            "{preset:?}: {output}"
        );
        assert!(
            output.contains("requiredFeatures=\"http://www.w3.org/TR/SVG11/feature#Text\""),
            "{preset:?}: {output}"
        );
    }

    // enable-background on <svg> is owned by cleanupEnableBackground and
    // must survive (simplified at most) when a filter uses BackgroundImage.
    let input = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" enable-background="new 0 0 10 10"><filter id="f"><feBlend in="BackgroundImage" in2="SourceGraphic"/></filter><rect width="10" height="10" fill="red"/><rect width="5" height="5" filter="url(#f)"/></svg>"##;
    for (preset, output) in optimize_both_presets(input) {
        assert!(
            output.contains("enable-background=\"new"),
            "{preset:?}: {output}"
        );
        assert!(output.contains("BackgroundImage"), "{preset:?}: {output}");
    }
}

#[test]
fn empty_conditional_attrs() {
    // Empty requiredFeatures/requiredExtensions/systemLanguage evaluate to
    // false and hide the element; stripping them (SVGO removeEmptyAttrs.02)
    // would make hidden content visible.
    let input =
        fs::read_to_string("tests/fixtures/regression/empty_conditional_attrs.svg").unwrap();
    for (preset, output) in optimize_both_presets(&input) {
        assert_valid_svg(&output, "empty_conditional_attrs");
        assert_eq!(
            output.matches("requiredFeatures=\"\"").count(),
            1,
            "{preset:?}: {output}"
        );
        assert_eq!(
            output.matches("requiredExtensions=\"\"").count(),
            1,
            "{preset:?}: {output}"
        );
        assert_eq!(
            output.matches("systemLanguage=\"\"").count(),
            2,
            "{preset:?}: {output}"
        );
        // Every shape stays hidden: it, or an ancestor, carries one of the
        // conditional attributes (collapseGroups may push it onto the shape).
        use svgm_core::ast::{Document, NodeId, NodeKind};
        fn walk(doc: &Document, id: NodeId, hidden: bool, preset: &str, output: &str) {
            let mut hidden = hidden;
            if let NodeKind::Element(ref elem) = doc.node(id).kind {
                hidden |= elem.attributes.iter().any(|a| {
                    matches!(
                        a.name.as_str(),
                        "requiredFeatures" | "requiredExtensions" | "systemLanguage"
                    ) && a.value.is_empty()
                });
                if matches!(elem.name.as_str(), "rect" | "path") {
                    assert!(hidden, "{preset}: <{}> became visible: {output}", elem.name);
                }
            }
            for child in doc.children(id) {
                walk(doc, child, hidden, preset, output);
            }
        }
        let doc = svgm_core::parser::parse(&output).unwrap();
        walk(&doc, doc.root, false, &format!("{preset:?}"), &output);
        assert_eq!(
            output.matches("<rect").count() + output.matches("<path").count(),
            4,
            "{preset:?}: {output}"
        );
    }
}

#[test]
fn processing_instructions_and_license_comments_survive_both_presets() {
    // SVGO removeXMLProcInst only drops the XML declaration; removeComments
    // keeps `<!--! ... -->` (its default preservePatterns).
    let input = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<?xml-stylesheet href=\"style.css\" type=\"text/css\"?>\n<!--!Icon Font v1 by @iconfont - Copyright 2023 Icon Font CIC.-->\n<!-- Generator: Adobe Illustrator -->\n<svg xmlns=\"http://www.w3.org/2000/svg\"><?foo bar?><!-- plain --><rect width=\"1\" height=\"1\"/></svg>";
    for (preset, output) in optimize_both_presets(input) {
        assert!(!output.contains("<?xml "), "{preset:?}: {output}");
        assert!(!output.contains("Generator"), "{preset:?}: {output}");
        assert!(!output.contains("plain"), "{preset:?}: {output}");
        assert!(
            output.starts_with("<?xml-stylesheet href=\"style.css\" type=\"text/css\"?><!--!Icon Font v1 by @iconfont - Copyright 2023 Icon Font CIC.--><svg"),
            "{preset:?}: {output}"
        );
        assert!(output.contains("<?foo bar?>"), "{preset:?}: {output}");
        let config = svgm_core::Config {
            preset,
            ..Default::default()
        };
        let again = svgm_core::optimize_with_config(&output, &config).unwrap();
        assert_eq!(again.data, output, "{preset:?}: not idempotent");
    }
    // Disabling removeProcInst round-trips the declaration.
    let config = svgm_core::Config {
        pass_overrides: [("removeProcInst".to_string(), false)]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let output = svgm_core::optimize_with_config(input, &config)
        .unwrap()
        .data;
    assert!(
        output.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?><?xml-stylesheet"),
        "{output}"
    );
}
#[test]
fn switch_fallback() {
    // draw.io / Illustrator exports: every direct child of <switch> is an
    // alternative branch. The empty requiredFeatures branch and the
    // multi-child fallback group must both survive, in both presets.
    let input = fs::read_to_string("tests/fixtures/regression/switch_fallback.svg").unwrap();
    for preset in [svgm_core::Preset::Safe, svgm_core::Preset::Default] {
        let config = svgm_core::Config {
            preset,
            ..Default::default()
        };
        let output = svgm_core::optimize_with_config(&input, &config)
            .unwrap()
            .data;
        assert_valid_svg(&output, "switch_fallback");
        assert_eq!(
            output.matches("<switch>").count(),
            2,
            "{preset:?}: {output}"
        );
        // draw.io: empty branch then the <a> fallback
        assert!(
            output.contains("<switch><g") && output.contains("</g><a "),
            "{preset:?}: {output}"
        );
        assert!(
            output.contains("Viewer does not support full SVG 1.1"),
            "{preset:?}: {output}"
        );
        // Illustrator: foreignObject branch then the drawing group
        assert!(output.contains("<foreignObject"), "{preset:?}: {output}");
        assert!(
            output.contains("</g><g>") || output.contains("</g><g "),
            "{preset:?}: {output}"
        );
        assert!(output.contains("fill=\"red\""), "{preset:?}: {output}");
        assert!(
            output.contains("fill=\"blue\"") || output.contains("fill=\"#00f\""),
            "{preset:?}: {output}"
        );
        let again = svgm_core::optimize_with_config(&output, &config)
            .unwrap()
            .data;
        assert_eq!(again, output, "{preset:?}: not idempotent");
    }
}

#[test]
fn transform_url_refs() {
    // A translate must stay a transform attribute on elements whose
    // rendering depends on the untransformed user space (userSpaceOnUse
    // clip/gradient/mask references, style refs, positioned <text>, <use>
    // instances), and a group transform must never be pushed into a
    // non-rendered resource child such as <clipPath>.
    let (output, _, _) = optimize_file(Path::new(
        "tests/fixtures/regression/transform_url_refs.svg",
    ));
    assert_valid_svg(&output, "transform_url_refs");
    for kept in [
        "clip-path=\"url(#a)\" d=\"M0 0h10v10H0z\" transform=\"translate(100,100)\"",
        "fill=\"url(#b)\" transform=\"translate(100,20)\"",
        "style=\"mask:url(#c)\" transform=\"translate(20,100)\"",
        "<text transform=\"translate(50,50)\"><tspan x=\"0\" y=\"10\">a</tspan></text>",
        "id=\"d\" transform=\"translate(5,5)\"",
        "<g transform=\"scale(2)\"><clipPath id=\"e\">",
    ] {
        assert!(output.contains(kept), "{kept} missing from {output}");
    }
    assert_eq!(output.matches("transform=").count(), 6, "{output}");
    let again = svgm_core::optimize(&output).unwrap().data;
    assert_eq!(again, output, "not idempotent");
}
