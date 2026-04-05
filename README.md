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
  Fast, safe SVG optimizer written in Rust.
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

## Why svgm?

SVG files from editors like Figma, Illustrator, and Inkscape are bloated with metadata, redundant attributes, and unoptimized paths.

[SVGO](https://github.com/svg/svgo) is the standard optimizer, but it has problems:

```
$ svgo icon.svg                    $ svgo icon.svg                    $ svgo icon.svg
13.5 KiB - 50.8% = 6.6 KiB        6.6 KiB - 0.2% = 6.6 KiB         6.6 KiB - 0% = 6.6 KiB
```

**Why do you have to run it three times?** SVGO runs plugins in a fixed order. When a later plugin creates an opportunity for an earlier one, that opportunity is missed until the next run. Nested 3 levels deep = 3 runs to converge.

**svgm fixes this.** One command. Fully optimized. Every time.

```
$ svgm icon.svg

  icon.svg
  13.5 KiB -> 6.6 KiB (51.1% smaller)  0ms  3 passes
```

svgm runs all optimization passes in a loop over the in-memory AST until nothing changes. No re-parsing between iterations. No manual multipass flag. One invocation, guaranteed convergence.

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

### Optimize in place (default)

```bash
svgm icon.svg
```

### Output to a different file

```bash
svgm icon.svg -o icon.min.svg
```

### Pipe to stdout

```bash
svgm icon.svg --stdout | gzip > icon.svgz
```

### Dry run

```bash
svgm icon.svg --dry-run
```

### Multiple files

```bash
svgm icons/*.svg
```

### Quiet mode

```bash
svgm icon.svg --quiet
```

## Benchmarks

Tested on 17 real SVG logos (Figma/Illustrator exports). Same files, same machine.

| | **svgm** | **SVGO** |
|:--|:--|:--|
| **Compression** | 38.4% | 46.9% |
| **Time** | 941ms | 2,174ms |
| **Speed** | **2.3x faster** | baseline |
| **Runs needed** | **1** | 1-3 |
| **Binary size** | 1.3 MB | Node.js runtime |

<details>
<summary><b>Full per-file results</b></summary>

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

svgm is closing the compression gap while staying significantly faster and running as a single native binary with zero dependencies.

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

The key insight: **passes operate on the in-memory AST, not strings.** SVGO re-parses from string between each multipass iteration. svgm mutates the tree directly, which is why it converges faster and uses less memory.

### Optimization passes

16 passes across 4 categories:

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

### Safety model

svgm is conservative by default:

- `<desc>` and `<title>` are **preserved** (accessibility semantics) unless you opt in
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

svgm is early-stage and contributions are welcome.

```bash
git clone https://github.com/builtbyfrmwrk/svgm.git
cd svgm
cargo test --workspace    # 68 tests
cargo clippy --workspace  # must pass clean
```

If you find an SVG that svgm corrupts or handles worse than expected, please [open an issue](https://github.com/builtbyfrmwrk/svgm/issues) with the SVG attached.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE).

[license-badge]: https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg
[license-url]: https://github.com/builtbyfrmwrk/svgm/blob/main/LICENSE-MIT
