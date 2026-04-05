# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**svgm** (SVG Minimizer) is a Rust-based SVG optimizer. Like oxlint is to ESLint, svgm is a ground-up Rust rewrite of the SVG optimization pipeline. The key differentiator vs SVGO is fixed-point convergence — passes run in a loop over the in-memory AST until nothing changes, so one invocation is always enough.

**Repository**: https://github.com/madebyfrmwrk/svgm
**License**: MIT + Apache 2.0 dual license

## Build & Development

```bash
source "$HOME/.cargo/env"   # Rust installed via rustup

cargo build                  # build all crates (debug)
cargo build --release        # release binary at target/release/svgm
cargo test --workspace       # all 114 tests (104 unit + 10 integration)
cargo test -p svgm-core      # core library tests only
cargo test -p svgm-core --test integration  # integration tests only
cargo test <test_name>       # run a single test by name
cargo test -- --nocapture    # run tests with stdout visible
cargo clippy --workspace     # lint — MUST pass with zero warnings
cargo fmt                    # format code
```

### Running the CLI

```bash
cargo run --release -- input.svg              # optimize in place (default)
cargo run --release -- input.svg -o out.svg   # output to different file
cargo run --release -- input.svg --stdout     # print to stdout
cargo run --release -- input.svg --dry-run    # preview without writing
cargo run --release -- *.svg                  # multiple files
cargo run --release -- input.svg --quiet      # suppress output
```

When piped (stdout is not a terminal), output goes to stdout automatically.

## Architecture

Cargo workspace with two crates:

### `crates/svgm-core` — Core optimization engine

- **`ast.rs`** — Arena-based document model. All nodes stored in `Vec<Node>` referenced by `NodeId(u32)`. Each node has `parent: Option<NodeId>`, `children: Vec<NodeId>`, `removed: bool`. O(1) parent access, O(1) removal via mark-and-sweep. Key types: `Document`, `Node`, `NodeKind`, `Element`, `Attribute`, `Namespace`.

- **`parser.rs`** — Uses `xmlparser` crate (zero-alloc XML tokenizer) to build the `Document` tree. Decodes XML entities (`&amp;` → `&`, etc.) in text nodes and attribute values. Handles namespaces, CDATA, PIs, comments, doctypes.

- **`serializer.rs`** — Converts `Document` back to minified SVG string. Self-closes void SVG elements (`<path/>`, `<rect/>`, etc.), uses `></tag>` for container elements (`<g>`, `<defs>`, `<svg>`, etc.). Escapes entities in text and attributes.

- **`optimizer.rs`** — Fixed-point convergence loop. Calls all passes sequentially, tracks if any pass returned `Changed`. Repeats until no pass changes anything, max 10 iterations. Public function: `optimize(doc) -> OptimizeResult { iterations }`.

- **`passes/mod.rs`** — Defines the `Pass` trait and `default_passes()` function that returns all passes in execution order. `PassResult` enum: `Changed` | `Unchanged`.

- **`passes/*.rs`** — One file per pass. 23 passes total (22 default + 1 opt-in):

  **Removal passes:**
  - `remove_doctype.rs` — strip DOCTYPE declarations
  - `remove_proc_inst.rs` — strip XML processing instructions
  - `remove_comments.rs` — strip comments
  - `remove_metadata.rs` — strip `<metadata>` elements
  - `remove_editor_data.rs` — strip Inkscape/Illustrator/Sketch/Figma elements, attributes, and namespace declarations. Has lists of editor namespace URIs and prefixes.
  - `remove_empty_attrs.rs` — strip attributes with empty string values
  - `remove_empty_text.rs` — strip `<text>`/`<tspan>`/`<textPath>` with no meaningful text children
  - `remove_empty_containers.rs` — strip `<g>`/`<defs>`/`<symbol>` etc. with no meaningful children (whitespace-only counts as empty). Preserves containers with `id` attribute.
  - `remove_hidden_elems.rs` — strip provably invisible elements: `display="none"`, zero-size shapes (rect/circle/ellipse with zero dimensions), empty/missing path `d`, zero-length lines, leaf shapes with `fill="none"` + `stroke="none"`. Conservative safety: skips elements with `id`, inside `<defs>`/`<symbol>`, animation targets, and elements with effect-bearing attrs (`clip-path`, `mask`, `filter`, `marker-*`). Does NOT remove `visibility="hidden"` or `opacity="0"` (deferred to v2).
  - `remove_unused_namespaces.rs` — strip xmlns declarations whose prefix isn't used by any element or attribute
  - `remove_unknowns_and_defaults.rs` — strip attributes matching SVG spec defaults (`opacity="1"`, `stroke="none"`, `fill="black"`, etc.). Has a conservative `DEFAULT_ATTRS` table. Skips `fill` on `<svg>` element (inherited by children).
  - `remove_desc.rs` — **opt-in, NOT in default preset**. Strips `<desc>` and `<title>` (accessibility concern).

  **Normalization passes:**
  - `cleanup_attrs.rs` — collapse runs of whitespace in attribute values to single space, trim
  - `cleanup_numeric_values.rs` — round numeric attributes to configurable precision (default 3), strip trailing zeros, strip `px` units, clean viewBox. Has `NUMERIC_ATTRS` list.
  - `convert_colors.rs` — normalize all color formats to shortest: `rgb()` → hex, `#rrggbb` → `#rgb`, named ↔ hex (whichever is shorter). Has `COLOR_ATTRS` list, `named_to_hex` and `hex_to_shorter_name` lookup tables.

  **Structural passes:**
  - `collapse_groups.rs` — removes unnecessary `<g>` wrappers. Two cases: (1) groups with no attributes → hoist children to parent, (2) single-child groups → merge group attrs into child if no conflicts. Skips groups with `transform` (transform merging is complex). `GROUP_ONLY_ATTRS` list (`clip-path`, `mask`, `filter`) blocks merging for attributes with different semantics on groups vs elements. Processes bottom-up.

  **Transform passes:**
  - `convert_transform.rs` — simplifies and applies transform attributes. Phase 1: parses transform strings, multiplies consecutive transforms into a single 2D affine matrix, serializes to shortest form (translate/scale/rotate or matrix). Removes identity transforms. Phase 2: applies pure translates directly to element coordinates (rect, circle, ellipse, line, text, use) and to path `d` attributes (translating absolute command coordinates in-place, leaving relative commands and arc radii/flags unchanged). Phase 3: pushes transforms from single-child groups to child element via matrix composition, enabling collapse_groups to remove the wrapper. Skips groups with `clip-path`/`mask`/`filter`. Has its own transform parser and 2D matrix math. `PathCmd`/`parse_path`/`serialize_path` from convert_path_data are `pub(crate)` for cross-pass use. Precision configurable (default 3).

  **Geometry passes:**
  - `convert_path_data.rs` — optimizes SVG path `d` attributes. Has its own path parser and serializer. Pipeline: normalize to absolute (expanding S→C and T→Q), simplify degenerate curves to lines (collinear control point detection), re-detect C→S and Q→T shorthands (reflected control points), remove redundant commands (zero-length lines), absolute-to-relative conversion (per-command, picks shorter), L→H/V shortcuts, leading zero removal (`.5` not `0.5`), implicit command repetition (omit repeated command letters), minimal separator insertion. Precision configurable (default 3). Handles all SVG path commands including arcs.

  **Attribute ordering:**
  - `sort_attrs.rs` — sorts element attributes alphabetically by qualified name for better gzip/brotli compression. Runs as second-to-last pass (after all attribute modifications are complete). No semantic impact — attribute order has no meaning in SVG/XML.

  **Whitespace:**
  - `minify_whitespace.rs` — removes whitespace-only text nodes (formatting indentation). Preserves whitespace inside text content elements (`<text>`, `<tspan>`, `<style>`, `<script>`, `<foreignObject>`, etc.).

- **`lib.rs`** — Public API: `svgm_core::optimize(svg_string) -> Result<OptimizeOutput, ParseError>` where `OptimizeOutput { data: String, iterations: usize }`.

### `crates/svgm-cli` — CLI binary

Uses `clap` (derive API) for arg parsing, `indicatif` for spinner, `console` for colored output. Default behavior: optimize in place (overwrites input file). When stdout is not a terminal (piped), writes to stdout instead. `-o` flag for explicit output path. `--stdout` flag to force stdout. `--dry-run`, `--quiet` flags.

### CI/CD

- `.github/workflows/ci.yml` — runs on every push and PR. Matrix: ubuntu, macos, windows. Steps: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, `cargo build --release`.
- `.github/workflows/release.yml` — triggered on `v*` tag push. Builds release binaries for linux-x64, macos-x64, macos-arm64, windows-x64. Creates GitHub Release with attached archives.

### Test fixtures

- `crates/svgm-core/tests/fixtures/synthetic/` — handcrafted SVGs targeting specific passes (comments+metadata, nested empty groups, colors+numbers, empty text, animation preservation)
- `crates/svgm-core/tests/fixtures/real/` — 8 real editor exports (test-1 through test-6 Figma-style, test-7-transforms with nested transform chains, test-8-figma-transforms with nested groups and mixed transform types)
- `crates/svgm-core/tests/fixtures/regression/` — SVGs reproducing known bugs (symbol_use_ref, foreign_object)
- `crates/svgm-core/tests/fixtures/regression/path_torture/` — 15 SVG files targeting path edge cases: arc flags, relative/absolute mixing, degenerate curves, tiny decimals, negative zero, large coordinates, subpath edges, implicit repeats, and 6 individual SVGO bug reproductions (#2199, #2104, #2093, #1858, #1773, #1676)
- `crates/svgm-core/tests/integration.rs` — 10 integration tests that optimize fixtures and verify: valid SVG output, specific pass effects, convergence (optimizing twice produces identical output), path structural equivalence

### Dependencies

```toml
xmlparser = "0.13"    # zero-alloc XML tokenizer
svgtypes = "0.16"     # SVG path/transform/color type parsing (available but not yet used by convert_path_data — has its own parser)
clap = "4"            # CLI arg parsing (derive API)
indicatif = "0.17"    # terminal spinners
console = "0.15"      # colored terminal output
thiserror = "2"       # error derive macro
```

## Current Benchmark Status

17 real SVG logos (Figma/Illustrator exports), SVGO 4.0.1:
- **svgm**: 40% compression, 62ms total
- **SVGO**: 47% compression, 2,010ms total
- svgm is **32x faster**, compression gap is ~7 points (down from 8.5)

The remaining gap is concentrated in: shape-to-path conversion (#5), path merging (#4), ID cleanup (#7), and CSS handling (#6). The biggest per-file gaps are on path-heavy SVGs (obsidian-icon 13pt, vercel 14pt, google-workspace 11pt).

## Non-Goals for v1

- No public plugin API (internal `Pass` trait only — freeze abstractions after engine is stable)
- No SVGO config file compatibility
- No formatting-preserving output (minified only)

## Code Conventions

- Every pass is one file in `passes/` implementing the `Pass` trait
- Every pass has inline `#[cfg(test)] mod tests` with before/after SVG snippets
- `cargo clippy --workspace` must pass with zero warnings
- `cargo test --workspace` must pass before committing
- Raw string literals `r#""#` cannot contain `#` characters (use escaped strings instead for SVGs with hex colors)
- The `Pass::run` method must return `PassResult::Changed` only if it actually modified the document
