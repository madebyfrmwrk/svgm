# Changelog

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
