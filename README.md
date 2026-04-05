<p align="center">
  <br>
  <a href="https://github.com/builtbyfrmwrk/svgm">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="assets/svgm-light.svg">
      <source media="(prefers-color-scheme: light)" srcset="assets/svgm-dark.svg">
      <img alt="svgm" src="assets/svgm-dark.svg" height="48">
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

</div>

---

## About

SVG files exported from tools like Figma, Illustrator, and Inkscape often include metadata, redundant attributes, unnecessary wrapper structure, and verbose path data.

[SVGO](https://github.com/svg/svgo) has been the standard SVG optimizer for years. svgm takes a different approach: a native Rust optimizer designed around fixed-point convergence, safe defaults, and a modern CLI. Like [oxlint](https://oxc.rs) for ESLint, svgm targets the same problem with a different architecture.

### Fixed-point convergence

In some optimizers, additional runs can still reduce output further because later passes create opportunities for earlier ones. svgm is designed to converge in a single invocation by running optimization passes over the in-memory AST until the document stabilizes.

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
git clone https://github.com/builtbyfrmwrk/svgm.git
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
svgm icon.svg --quiet            # Suppress all output except errors
```

When piped (e.g. `svgm icon.svg | gzip`), output goes to stdout automatically.

## Benchmarks

17 real SVG logos (Figma/Illustrator exports). Same files, same machine.

| | **svgm** | **SVGO** |
|:--|:--|:--|
| **Compression** | 38.4% | 46.9% |
| **Total time** | 941ms | 2,174ms |
| **Speed** | **2.3x faster** | baseline |
| **Invocations to converge** | **1** | 1-3 |

<details>
<summary><b>Per-file breakdown</b></summary>

```
FILE                          SVGM         SVGO
anthropic-icon.svg           48.1%        74.8%
apidog.svg                   46.8%        55.9%
astro.svg                    47.2%        50.9%
claude.svg                   49.2%        53.1%
google-play-console.svg      51.1%        60.9%
google-workspace.svg         49.6%        63.1%
incident.svg                 49.8%        54.7%
moonshot-ai.svg              55.5%        61.6%
obsidian-icon.svg            32.5%        45.2%
obsidian.svg                 48.3%        53.8%
oxc-dark.svg                 16.5%        25.5%
oxc-icon.svg                 15.7%        25.1%
oxc.svg                      16.4%        25.4%
perplexity.svg               52.8%        59.4%
vercel.svg                   46.9%        63.5%
vite.svg                     15.2%        25.7%
xcode.svg                    36.7%        43.8%
```

</details>

svgm is closing the compression gap while staying significantly faster and shipping as a single native binary. Path, transform, and CSS optimizations are still being developed.

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
- Comments, doctypes, XML processing instructions
- Editor metadata (Inkscape, Illustrator, Sketch, Figma)
- Empty containers, empty attributes, empty text elements
- Unused namespace declarations
- Attributes matching SVG spec defaults (`opacity="1"`, `stroke="none"`, etc.)

**Normalization** — tighten values
- Collapse whitespace in attributes
- Round numeric values, strip trailing zeros and default `px` units
- Shorten colors: `rgb(255,0,0)` -> `red`, `#aabbcc` -> `#abc`

**Structural** — simplify the tree
- Collapse useless `<g>` wrappers (no-attribute groups, single-child groups)

**Geometry** — compress path data
- Absolute-to-relative coordinate conversion where shorter
- `L` to `H`/`V` shortcut commands
- Strip leading zeros (`.5` instead of `0.5`)
- Implicit command repetition
- Minimal separator insertion

### Safety

svgm is conservative by default:

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

## Project structure

```
svgm/
├── crates/
│   ├── svgm-core/       # Parser, AST, optimizer, serializer, passes
│   └── svgm-cli/        # CLI binary (clap + indicatif)
├── LICENSE-MIT
└── LICENSE-APACHE
```

## Roadmap

- [ ] Transform merging and application
- [ ] Path merging (adjacent paths with identical attributes)
- [ ] Shape-to-path conversion
- [ ] `<use>` dereferencing
- [ ] CSS `<style>` minification
- [ ] Recursive directory processing (`-r`)
- [ ] WASM build for browser usage
- [ ] Node.js bindings via napi-rs

## Contributing

svgm is early, but already usable. Contributions and real-world SVG edge cases are especially helpful.

```bash
git clone https://github.com/builtbyfrmwrk/svgm.git
cd svgm
cargo test --workspace
cargo clippy --workspace  # must pass clean
```

If you find an SVG that svgm corrupts or handles worse than expected, please [open an issue](https://github.com/builtbyfrmwrk/svgm/issues) with the SVG attached.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE).

[license-badge]: https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg
[license-url]: https://github.com/builtbyfrmwrk/svgm/blob/main/LICENSE-MIT
