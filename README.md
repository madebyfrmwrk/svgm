<p align="center">
  <br>
  <a href="https://github.com/madebyfrmwrk/svgm">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/madebyfrmwrk/svgm/main/assets/svgm-dark.svg">
      <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/madebyfrmwrk/svgm/main/assets/svgm-light.svg">
      <img alt="svgm" src="https://raw.githubusercontent.com/madebyfrmwrk/svgm/main/assets/svgm-light.svg" height="42">
    </picture>
  </a>
  <br>
  <br>
</p>

<p align="center">
  SVG optimization, rewritten in Rust.
</p>

<p align="center">
  <a href="#install">Install</a> &middot;
  <a href="#usage">Usage</a> &middot;
  <a href="#benchmarks">Benchmarks</a> &middot;
  <a href="#how-it-works">How it works</a> &middot;
  <a href="#contributing">Contributing</a>
</p>

<div align="center">

[![MIT licensed][license-badge]][license-url]
[![Crate][crates-badge]][crates-url]
[![Build Status][ci-badge]][ci-url]

</div>

---

## About

SVG files exported from tools like Figma, Illustrator, and Inkscape often include metadata, redundant attributes, unnecessary wrapper structure, and verbose path data.

[SVGO](https://github.com/svg/svgo) has been the standard SVG optimizer for years. SVGM takes a different approach: a native Rust optimizer designed around fixed-point convergence, safe defaults, and a modern CLI. Like [oxlint](https://oxc.rs) for ESLint, SVGM targets the same problem with a different architecture.

### Fixed-point convergence

In some optimizers, additional runs can still reduce output further because later passes create opportunities for earlier ones. SVGM is designed to converge in a single invocation by running optimization passes over the in-memory AST until the document stabilizes.

```
$ svgm icon.svg

  icon.svg
  13.5 KiB -> 6.6 KiB (51.1% smaller)  0ms  3 passes
```

No re-parsing between iterations. No manual multipass flag. One invocation, fixed-point optimization.

## Install

### From source (requires [Rust](https://rustup.rs))

```bash
cargo install svgm
```

### Build from repo

```bash
git clone https://github.com/madebyfrmwrk/svgm.git
cd svgm
cargo build --release
# Binary at ./target/release/svgm
```

## Usage

```bash
svgm icon.svg                    # Optimize in place (overwrites the file)
svgm icon.svg -o icon.min.svg    # Write to a different file
svgm icon.svg --stdout           # Print to stdout instead of overwriting
svgm icon.svg --dry-run          # Preview size reduction without writing
svgm icons/*.svg                 # Optimize multiple files in place
svgm -r ./icons                  # Recursively optimize all SVGs in a directory
svgm -r ./icons -o ./icons-min   # Recursive with output directory
svgm icon.svg --quiet            # Suppress all output except errors
```

When piped (e.g. `svgm icon.svg | gzip`), output goes to stdout automatically.

### Presets

```bash
svgm icon.svg --preset safe       # Removal + normalization only (20 passes)
svgm icon.svg --preset default    # Full optimization (34 passes, default)
svgm icon.svg --precision 2       # Override numeric precision on any preset
```

### Config file

Create an `svgm.config.toml` in your project root:

```toml
preset = "default"
precision = 3

[passes]
convertShapeToPath = false  # opt-out: disable a specific pass
```

SVGM auto-discovers the config by walking up from the input file's directory. Use `--config path` to specify explicitly, or `--no-config` to skip. See [`svgm.config.example.toml`](svgm.config.example.toml) for a starter template.

## Benchmarks

100 real-world SVG logos (Figma, Illustrator, Inkscape, svgrepo exports). 902.7 KiB total original size.

| | **SVGM** | **SVGO** |
|:--|:--|:--|
| **Compression** | **18.5%** | 18.2% |
| **Median time** | **347ms** | 11,595ms |
| **Speed** | **33x faster** | baseline |
| **Files won** | **55** | 44 |
| **Invocations to converge** | **1** | 1-3 |

Full benchmark details at [svgm.dev/docs/benchmarks](https://svgm.dev/docs/benchmarks).

SVGM compresses more than SVGO while being 33x faster.

## How it works

### Architecture

```
              parse              optimize              serialize
SVG string ---------> AST tree ---------> AST tree -----------> SVG string
             xmlparser         fixed-point            minified
                                  loop                 output
```

1. **Parse** — `xmlparser` tokenizes the SVG into an arena-based AST with parent pointers
2. **Optimize** — Run all passes in a loop until no pass reports a change (max 10 iterations)
3. **Serialize** — Write the AST back as a minified SVG string

Passes operate directly on the in-memory AST, avoiding repeated serialize/parse cycles between iterations.

### Optimization passes

**Removal** — strip dead weight
- Comments, doctypes, XML processing instructions, deprecated attributes
- Editor metadata (Inkscape, Illustrator, Sketch, Figma) and editor-generated descriptions
- Empty containers, empty attributes, empty text elements, useless defs
- Unused namespace declarations, non-inheritable group attributes
- Attributes matching SVG spec defaults (`opacity="1"`, `stroke="none"`, `version="1.1"`, etc.)
- Useless stroke/fill sub-properties when the primary is invisible

**Normalization** — tighten values
- Collapse whitespace in attributes, minify inline styles
- Round numeric values with smart precision (tries lower precision when error is acceptable)
- Shorten colors: `rgb(255,0,0)` -> `red`, `#aabbcc` -> `#abc` (including in `style=""`)
- Promote common child attributes to parent group
- Sort `<defs>` children for better compression
- Clean up deprecated `enable-background`

**Structural** — simplify the tree
- Collapse useless `<g>` wrappers (no-attribute groups, single-child groups with transform merging)
- Distribute group transforms to children when safe
- Reference safety: groups with `clip-path`, `mask`, or `filter` are never collapsed

**Transform** — simplify and apply transforms
- Merge consecutive transforms into a single equivalent (`translate(10,20) translate(5,5)` -> `translate(15,25)`)
- Decompose matrices to shorter forms (`matrix(a,0,0,a,tx,ty)` -> `translate(tx,ty)scale(a)`)
- Remove identity transforms (`scale(1)`, `translate(0,0)`, `rotate(0)`)
- Apply pure translates directly to element coordinates and path data (with length comparison)
- Push transforms from single-child groups to child, enabling group collapse
- Optimize `gradientTransform` and `patternTransform`

**Geometry** — compress path data
- Shape-to-path conversion (rect, circle, ellipse, line, polyline, polygon → shorter `<path>`)
- Ellipse-to-circle conversion when rx equals ry
- Path merging (adjacent non-overlapping paths with identical attributes, with geometric intersection safety)
- Cubic-to-quadratic Bezier conversion (C → Q when losslessly possible)
- Cubic-to-arc conversion (detects circular arc curves)
- Smart arc radius rounding (sagitta-based geometric validation)
- Absolute-to-relative coordinate conversion where shorter (precision-aware comparison)
- `L` to `H`/`V` shortcut commands, consecutive h/v merging
- `C` to `S` and `Q` to `T` shorthand curves (reflected control points, including after non-curve commands)
- Degenerate curve to line simplification (collinear control points)
- Redundant command removal (zero-length lines, lineto before closepath)
- Strip leading zeros (`.5` instead of `0.5`), no space after command letters
- Implicit command repetition, minimal separator insertion

**IDs** — clean up references
- Remove unused `id` attributes
- Shorten referenced IDs to minimal unique names

### Safety

SVGM is conservative by default:

- `<desc>` and `<title>` are **preserved** (accessibility semantics)
- `<symbol>` and `<defs>` with `id` attributes are **never removed** (may be referenced)
- Animation elements (`<animate>`, `<animateTransform>`, etc.) are **fully preserved**
- `<foreignObject>` content is **never touched**
- `fill="black"` on `<svg>` is **kept** (inherited by children)

## Rust API

```rust
use svgm_core::optimize;

let result = optimize("<svg>...</svg>").unwrap();
println!("{}", result.data);       // optimized SVG string
println!("{}", result.iterations); // convergence iterations
```

With preset/precision control:

```rust
use svgm_core::{optimize_with_config, Config, Preset};

let config = Config {
    preset: Preset::Safe,
    precision: Some(2),
    ..Config::default()
};
let result = optimize_with_config("<svg>...</svg>", &config).unwrap();
```

## Project structure

```
svgm/
├── crates/
│   ├── svgm-core/       # Parser, AST, optimizer, serializer, passes
│   ├── svgm-cli/        # CLI binary (clap + indicatif)
│   ├── svgm-wasm/       # WebAssembly build (npm: svgm-wasm)
│   └── svgm-napi/       # Node.js native addon (npm: svgm-node)
├── LICENSE-MIT
└── LICENSE-APACHE
```

## Roadmap

- [x] Transform merging, application, and push-down (all 3 phases)
- [x] Shape-to-path conversion (rect, circle, ellipse → shorter `<path>`)
- [x] Path merging (adjacent paths with identical attributes)
- [x] ID cleanup (remove unused, shorten used)
- [x] CSS `<style>` inlining and minification
- [x] Recursive directory processing (`-r`)
- [x] Safety presets and config file support
- [x] WASM build for browser usage
- [x] SVGO feature parity (34 passes, compression at par)
- [x] Node.js bindings via napi-rs

## Contributing

SVGM is early, but already usable. Contributions and real-world SVG edge cases are especially helpful.

```bash
git clone https://github.com/madebyfrmwrk/svgm.git
cd svgm
cargo test --workspace
cargo clippy --workspace  # must pass clean
```

If you find an SVG that SVGM corrupts or handles worse than expected, please [open an issue](https://github.com/madebyfrmwrk/svgm/issues) with the SVG attached.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE).

[license-badge]: https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg
[license-url]: https://github.com/madebyfrmwrk/svgm/blob/main/LICENSE-MIT
[crates-badge]: https://img.shields.io/crates/v/svgm.svg
[crates-url]: https://crates.io/crates/svgm
[ci-badge]: https://github.com/madebyfrmwrk/svgm/actions/workflows/ci.yml/badge.svg
[ci-url]: https://github.com/madebyfrmwrk/svgm/actions
