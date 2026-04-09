# Changelog

## 0.3.0

### Added

- **9 new optimization passes** matching SVGO's default plugin set (25 → 34 passes):
  - `removeDeprecatedAttrs` — strips deprecated SVG attributes (`xml:space`, `requiredFeatures`, etc.)
  - `removeUselessDefs` — removes unreferenced elements inside `<defs>`
  - `removeNonInheritableGroupAttrs` — strips non-inheritable presentation attributes from `<g>` elements
  - `removeUselessStrokeAndFill` — removes stroke/fill sub-properties when the primary is invisible
  - `convertEllipseToCircle` — converts `<ellipse>` to `<circle>` when rx equals ry
  - `cleanupEnableBackground` — removes deprecated `enable-background` attribute and style property
  - `moveElemsAttrsToGroup` — promotes common child attributes to parent group
  - `moveGroupAttrsToElems` — distributes group transforms to children when safe
  - `sortDefsChildren` — sorts `<defs>` children for better gzip compression
- **Cubic-to-quadratic Bezier conversion** — losslessly converts C commands to shorter Q commands
- **Cubic-to-arc conversion** — detects circular arc cubics and converts to SVG arc commands
- **Smart arc radius rounding** — uses sagitta comparison to aggressively round arc radii without visual distortion
- **Consecutive h/v merging** — combines adjacent horizontal/vertical line commands
- **Redundant lineto-before-closepath removal** — strips final line segment when closepath returns to same point
- **S/s shorthand detection after non-curve commands** — detects additional smooth cubic shorthand opportunities
- **Colors in `style=""` attributes** — shortens hex colors inside inline style declarations
- **Hex color lowercasing** — normalizes `#C3002F` to `#c3002f`
- **Inline style minification** — strips spaces around colons/semicolons in `style` attributes
- **Percentage precision truncation** — rounds percentage values in gradient attributes
- **European decimal comma normalization** — handles `translate(0,7282, 0,9693)` from non-English exports
- **Transform decomposition** — decomposes `matrix(a,0,0,a,tx,ty)` to shorter `translate()scale()` form
- **gradientTransform/patternTransform** optimization — simplifies gradient and pattern transforms
- **Additional default attributes** — removes `version="1.1"`, `mode="normal"`, `stop-color="#000"`, `x="0"`/`y="0"` on `<svg>`, and other SVG spec defaults

### Changed

- **Presets simplified**: `Balanced` and `Aggressive` replaced with `Default`. Two presets: `safe` and `default`. `balanced` and `aggressive` accepted as backward-compatible aliases.
- **`removeDesc` moved to Default preset** — now only removes editor-generated descriptions ("Created with...", "Generator:..."), preserves custom descriptions for accessibility.
- **Default precision** is always 3 (no more Aggressive=2 special case).
- **collapseGroups** now merges transforms during single-child group collapse (composes group + child transforms).
- **convertTransform** compares translate-applied vs translate-kept length, keeping the shorter form.
- **StrongRound** — path data and numeric attributes try precision-1 when error is acceptable.
- **Precision-aware abs/rel selection** — all path command comparisons use rounded lengths for accuracy.

### Benchmark

100 real-world SVG logos, 902.7 KiB total:
- **svgm 0.3.0**: 18.5% compression, 347ms median, **33x faster** than SVGO
- **SVGO 4.0.1**: 18.2% compression, 11,595ms median
- svgm wins 55 of 100 files, SVGO wins 44, 1 tie

## 0.2.2

### Fixed

- **Path merging no longer breaks overlapping paths.** Previously, `mergePaths` would merge adjacent paths based solely on matching attributes, which broke SVGs with overlapping paths and `fill-rule="evenodd"` (e.g. the McDonald's logo). Now uses geometric intersection detection (AABB + convex hull GJK, ported from SVGO) to skip merging when paths overlap.

### Improved

- `mergePaths` now blocks merging when `clip-path`, `mask`, or `mask-image` is present on the element or inherited from ancestors.
- `mergePaths` now blocks merging when `fill`, `stroke`, or `filter` contains a `url()` reference (gradient/pattern bounding box changes when paths are combined).
- Ancestor attribute inheritance check for all blocking properties (markers, clip-path, mask).

## 0.2.1

### Added

- 18 CLI integration tests covering file operations, stdout/pipe behavior, recursive mode, error cases, file filtering, and config discovery.
- Example config file (`svgm.config.example.toml`) as a starter template.

### Improved

- Polished `--help` text: clearer flag descriptions, preset breakdown in long help, pipe behavior and config hints in after-help.
- Fixed "single-pass" → "fixed-point convergence" in CLI about text.

## 0.2.0

### Added

- **Safety presets** (`--preset safe|balanced|aggressive`): control optimization aggressiveness. `safe` runs only removal/normalization passes (17 passes). `balanced` runs the full pass set (24 passes, default — same as prior behavior). `aggressive` uses the full pass set with lower numeric precision (2 instead of 3).
- **Config file support** (`svgm.config.toml`): per-project configuration with `preset`, `precision`, and per-pass `[passes]` overrides. Auto-discovered by walking up from the input file's directory.
- `--precision N` flag to override numeric rounding precision on any preset.
- `--config PATH` flag to specify an explicit config file path.
- `--no-config` flag to skip config file auto-discovery.
- New public API: `svgm_core::optimize_with_config(input, &config)` for programmatic preset/precision control.
- `svgm_core::Config` and `svgm_core::Preset` types exported from the core crate.

### Changed

- `removeDesc` remains opt-in only — not included in any preset. Enable via config file `removeDesc = true`.
- Default behavior is unchanged (`balanced` preset matches the prior full pass set at precision 3).

## 0.1.3

### Added

- Recursive directory processing with `-r` flag: `svgm -r ./icons` optimizes all SVGs in place recursively.
- Output directory support: `svgm -r ./icons -o ./icons-min` writes optimized files preserving directory structure.
- Progress bar and aggregate summary for directory mode.

### Changed

- `-r` requires exactly one directory input — mixed input (`svgm -r dir file.svg`) is not allowed.
- Output directory overlapping source directory is now an error to prevent recursive write loops.

## 0.1.2

### Fixed

- Multi-file input (`svgm *.svg`) in non-terminal contexts (scripts, CI, piped commands) now correctly optimizes files in place instead of silently writing concatenated output to stdout.
- `--stdout` with multiple input files now returns an explicit error instead of producing malformed output.

## 0.1.1

### Fixed

- README images now use absolute GitHub URLs so they render correctly on crates.io.

## 0.1.0

Initial release.

- 25 optimization passes (24 default + 1 opt-in)
- Fixed-point convergence — one invocation is always enough
- Arena-based AST with O(1) parent access and mark-and-sweep removal
- Path data optimization (absolute/relative, curve shorthands, implicit repeats)
- Transform merging, application, and push-down
- CSS `<style>` inlining and minification
- Conservative defaults — preserves `<desc>`, `<title>`, animations, `<foreignObject>`
- CLI with in-place editing, `--stdout`, `--dry-run`, `--quiet`
