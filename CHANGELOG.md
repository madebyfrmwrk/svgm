# Changelog

## 0.4.0

A correctness release. Every optimisation that SVGO 4 refuses as unsafe is now refused by svgm too, after an audit of all 34 default passes against SVGO's source and fixtures. On the 100-logo benchmark corpus compression goes from 18.5% (0.3.9) to 18.1% (SVGO 4.0.1: 18.2%). Nearly all of that is path data: 0.3.9 rounded many coordinates past the configured precision, which the corrected strongRound rule no longer does. In exchange, none of the cases below change how an SVG renders any more, and a 414-file pixel comparison of svgm output against SVGO output shows no difference. 0 hangs, 0 panics and 0 non-idempotent files on the 433-input robustness corpus. Test count 356 → 670.

### Fixed — parsing and namespaces

- **DOCTYPE internal-subset entities are expanded.** Illustrator exports write `xmlns="&ns_svg;"`; the root element used to leave the SVG namespace and render blank. `<!ENTITY>` values are substituted (depth-capped, byte-budgeted against billion-laughs input), and the raw DOCTYPE text round-trips when `removeDoctype` is disabled.
- **Editor data is removed by bound namespace URI, not by prefix.** `xmlns:x="…/xlink"` + `x:href`, or `xmlns:a`/`xmlns:i`/`xmlns:dc` bound to non-editor URIs, are preserved; editor data under non-canonical prefixes (`xmlns:ink`, `serif:`, `bx:`, `v:`, `krita:`) is removed together with its declaration instead of leaving an undeclared prefix. Nine editor namespaces SVGO knows were added.
- **Namespace declarations on a collapsed `<g>` move to the child** instead of being dropped (browsers rejected the output as ill-formed XML).
- `<?xml-stylesheet?>` and other processing instructions, and `<!--! … -->` license comments, are kept (only the XML declaration and plain comments are removed).
- Empty `requiredFeatures`/`requiredExtensions`/`systemLanguage` are kept (an empty value means "never render").

### Fixed — things that were deleted or altered although they rendered

- `removeDeprecatedAttrs` no longer strips SVGO's *unsafe* set by default: `xml:space="preserve"`, `xml:base`, `requiredFeatures`, `attributeType`, `clip`, `color-profile`, `glyph-orientation-*` survive both presets (`remove_unsafe` field for opt-in).
- Attributes referenced by CSS attribute selectors (`svg[version]`, `rect[clip]`, `[fill=black]`, also inside `@media`) are never removed as defaults or deprecated (SVGO #2144).
- Direct children of `<switch>` are never collapsed or removed, so draw.io / Illustrator fallback branches keep rendering the intended alternative.
- An empty `<g>` carrying a `filter` (attribute, `style`, or class rule) is kept; its filter and defs are no longer cascade-deleted.
- `overflow="visible"` is not a default (the UA default is `hidden`); it is no longer stripped from `svg`/`image`/`foreignObject`/`symbol`/`marker`/`pattern`.
- Leaf shapes with `fill="none" stroke="none"` are no longer deleted (pointer-events hit areas, clipPath/mask/marker children, CSS-restyled shapes). SVGO only does this as the opt-in `removeNone`.
- Zero-length `<line>` elements and zero-length first path segments are kept when a round or square `stroke-linecap` draws a dot; `h`/`v` runs are no longer merged under `marker-mid`; markers-only paths keep their last vertex.
- `display="none"` removal honours `style=""` overrides and bails while a stylesheet or script remains; `style="display:none"` is removed too. `removeHiddenElems` now runs after `minifyStyles`, as in SVGO.
- `<ellipse>` with one missing radius (SVG 2 `auto`) is kept and converted to `<circle>` instead of being deleted.
- `removeUnknownsAndDefaults` and `removeUselessStrokeAndFill` resolve inherited values through ancestor `style=""` declarations and treat the document as dynamic while a `<style>` or script exists, so `fill="black"` under `<g style="fill:red">` and `stroke-width` under `<g stroke="#000">` survive. `removeUselessStrokeAndFill` skips subtrees with an `id` (`<use>` targets), respects `marker-end`, and ports SVGO's normalisation.
- `convertShapeToPath` aborts on unit or percentage coordinates (`x="25%"`, `rx="1em"`) instead of treating them as 0.
- `<text>`/`<tspan>` whose only content is CDATA are not "empty"; whitespace-only `<tspan>` separating words is kept (`read more` no longer becomes `readmore`); whitespace inside `<a>`/`<tref>`/`<altGlyph>` nested in `<text>` is preserved.
- `removeDesc` only strips `Created with…`/`Created using…` boilerplate, not `Generator:`/`Made with`/`Produced by` descriptions.

### Fixed — path data and transforms

- Opposite-direction `h`/`v` pairs (`h30h-30`) are no longer merged into nothing.
- Straight-curve detection is normalised by chord length (closed single-cubic loops are no longer flattened; long near-straight curves now simplify).
- Arc radius rounding uses the real chord for absolute `A` commands; strongRound follows SVGO's candidate-vs-original rule (kappa cubics become arcs again, `-18.079` no longer over-rounds).
- Redundant movetos (`M…M…`) are dropped unless the path has markers; a leading relative `m` is treated as absolute (SVGO `path2js`) so baking a `translate` no longer displaces the path.
- `mergePaths` re-serialises parsed commands instead of concatenating raw strings (a leading `m` in the second path shifted the shape), converts arc control points to absolute before the intersection test, refuses paths with `<title>`/`<desc>`/animation children, and resolves blocking `fill`/`stroke`/`marker-*`/`clip-path`/`mask` through `style` and inheritance. Three bugs in the arc-to-cubic port were fixed along the way.
- `convertTransform` tries the raw transform string before the European-decimal heuristic (`translate(10,20), scale(2,2)` was mangled into `translate(10.2)scale(2.2)`), and no longer bakes translates into elements with an `id`, a `style` attribute, `url()` references, animations, a CSS `transform` rule, or `<text>` with positioned children. Group transforms are only pushed into renderable children without ids or animations, never into `clipPath`/`mask`/`symbol`/gradients. `clip-path`/`mask`/`filter` set in `style` count as group-only properties.
- `collapseGroups` refuses to merge a group `style=""` onto a child whose presentation attribute conflicts, and never collapses into children with ids or animated attributes.

### Fixed — ids and CSS

- `cleanupIds` bails when scripts, event handlers, `javascript:` links or a `:target` rule are present, and on defs-only documents (sprite sheets); it rewrites `#id` only inside `<style>` (never in visible text), no longer mistakes hex colours for id references, accepts quoted / percent-encoded `url('#id')`, scans identifiers as CSS idents (non-ASCII ids work), reserves ids inside `<foreignObject>`, and assigns names in first-reference order so output is idempotent after `sortDefsChildren`.
- `removeUselessDefs` recurses like SVGO's `collectUsefulNodes`: referenced gradients/clipPaths nested in id-less `<g>` or nested `<defs>` wrappers are hoisted instead of deleted with the wrapper.
- `inlineStyles`: stylesheets containing only at-rules (`@keyframes`, `@font-face`, `@media`) are preserved in source order; compound selectors parse correctly (`.a.b` no longer matches `.a`, `#x.a` is id + class); non-ASCII class and id selectors work; rules are applied by exact specificity and source order, and inlined into `style=""` rather than presentation attributes when a stylesheet remains; classes are removed per element only for fully consumed rules (never with scripts, at-rules or `[class]` selectors); `<style media="…">` is wrapped as `@media`; `<style>` inside `<foreignObject>` is ignored; declarations are split quote- and paren-aware (`url(data:…;base64,…)`, `font-family:"a; b"`).
- While a stylesheet with selectors svgm cannot inline remains, `collapseGroups`, `convertShapeToPath`, `convertEllipseToCircle` and `moveGroupAttrsToElems` leave the structure alone so those selectors keep matching.
- `minifyStyles` no longer lengthens colours (`green` → `#008000`) or skips the declaration after a pseudo-class colon, and only shortens colour properties.

### Changed

- New shared helpers in `svgm-core` (`passes/util.rs`): computed style resolution, script/stylesheet detection, CSS declaration splitting, ancestor and animation helpers. Internal API only.
- Many of the fixes above make svgm more conservative than before while a `<style>` or `<script>` element remains in the document. If you rely on maximum compression for such files, inline the CSS first.
- Known byte-only regressions tracked for 0.4.1: the absolute/relative segment choice in `convertPathData` sometimes picks the longer form; `cleanupIds` skips defs-only documents entirely, so unreferenced `<linearGradient id>`/`<filter id>` in sprite sheets are no longer removed; the structural-CSS guard treats any remaining stylesheet as structural.

## 0.3.9

### Fixed

- **Path data with an unparseable token no longer hangs the optimizer** (#22). `parse_path` looped forever on inputs like `<path d="M0 0 e"/>`, pinning a CPU core and allocating until killed. Parsing now stops at the first token it cannot consume and keeps the valid prefix, matching browser and SVGO error recovery; a path with nothing usable before the bad token is left untouched. The same parser is shared by `convertTransform` and `mergePaths`, so all three are covered.
- **Non-ASCII text in `<style>` is no longer corrupted and blown up** (#23). `minifyStyles` rebuilt stylesheet text by casting each byte to `char`, which re-encoded every multi-byte UTF-8 character as Latin-1, doubling it on every pass and growing a 100-byte file to 2 MB in a single `optimize()` call. All byte-level rewriters (`minifyStyles`, `convertTransform`'s European-decimal normalizer) now copy non-ASCII bytes verbatim.
- **Truncated or root-less input is now a parse error instead of empty output** (#24). `optimize("<svg")` returned `Ok("")`, and `<svg>` returned an auto-closed document, so a write-back pipeline could silently replace a file with nothing. The parser now reports `unexpected end of input` for an unfinished start tag, `unclosed tag` for unclosed elements, and `document has no root element` for empty, whitespace-only, declaration-only or comment-only input. Non-`<svg>` roots are still accepted, as in SVGO. A UTF-8 byte-order mark is stripped before parsing.
- **Non-finite and huge numbers can no longer be written into the output.** `1e999`, `inf`, `nan`, `1e308` and the like (in coordinates, path data, transforms, `viewBox`, `points`) were parsed by Rust's float parser, became `inf`/`NaN` or 300-digit expansions, and re-triggered the #22 hang on the next iteration. Every numeric site now uses the SVG number grammar, rejects non-finite values, and leaves values that cannot be rounded compactly untouched.
- **`--precision` is now bounded to 0–20.** Values of 309 and above produced `NaN` everywhere and very large values panicked. The CLI rejects out-of-range values; the library, WASM and Node bindings clamp or reject them.
- **`#800080` was rewritten to `plum`** (`#dda0dd`) instead of `purple` by `convertColors`. Fixed, with a table self-consistency test so a wrong mapping cannot ship again.
- **Malformed hex colors no longer panic or get rewritten into nonsense.** `#aǎ` aborted the process (a char vector indexed by byte length); `#GGG` became `#gggggg`. Only `#rgb`/`#rrggbb` made of ASCII hex digits are treated as colors.
- **The fixed-point loop rolls back an iteration that grows the document.** Defence in depth: a misbehaving pass can no longer be amplified ten times over. The CLI also isolates panics per file so one bad input cannot abort a recursive batch, and the Node binding turns panics into JS exceptions.
- **Consecutive movetos are serialized with their letter.** `convertPathData` emitted `M1 1M50 50` as `M1 1 50 50`, which is a moveto followed by a *lineto* and drew a stray segment (exposed by `mergePaths` on single-moveto paths).
- **`mergePaths` refuses to merge a path whose `d` could not be parsed in full.** The merged attribute is built from the original strings, so an unparseable tail would otherwise sit mid-path and hide every shape after it.
- **A bare `&` no longer swallows the rest of a text node or grows a `;`**, and numeric references to characters XML forbids (`&#0;`, `&#1;`) are kept as literal text instead of emitting raw control bytes that svgm itself could not re-parse.
- **`cleanupNumericValues` never lengthens a value** (`1e14` used to expand to 15 digits) and no longer "repairs" invalid lengths like `5 px`.
- `inlineStyles` no longer reports a change when it rewrites a presentation attribute to its current value, which made some documents run all 10 iterations.
- `svgm.config.toml` rejects `precision` above 20 with a clear error instead of silently clamping.

### Changed

- Release workflow: npm publish steps are skipped when the version already exists on the registry, a verify job fails fast when the tag does not match `Cargo.toml` / `svgm-napi/package.json`, superseded runs for a re-pushed tag are cancelled, and the deprecated Node 20 actions were upgraded (`upload-artifact@v7`, `download-artifact@v8`, `checkout@v7`, `setup-node@v7`, `action-gh-release@v3`).

## 0.3.8

### Fixed

- **Fixed invisible SVG elements when ancestor overrides `fill` or `stroke`.** The `removeUnknownsAndDefaults` pass now walks the ancestor chain before removing attributes that match SVG spec defaults. Previously, `fill="black"` was removed even when a parent had `fill="none"`, causing elements to inherit the wrong value and become invisible. Now matches SVGO's behavior: Check A only removes defaults when no ancestor has the property; Check B removes useless overrides (child value matches inherited value). Elements with `id` are skipped (may be CSS/JS targets).

## 0.3.7

### Fixed

- Fixed napi npm publish: bundle all platform `.node` binaries directly in the `svgm-node` package instead of publishing separate platform packages. One package, trusted publishing, green tick.

## 0.3.6

### Fixed

- Fixed napi npm publish to use OIDC trusted publishing (`--provenance`) for platform packages instead of token auth.

## 0.3.5

### Fixed

- Fixed release workflow to properly publish platform-specific npm packages (`svgm-node-darwin-arm64`, etc.) as optional dependencies. Previously only the main `svgm-node` package was published without the native binaries.

## 0.3.4

### Fixed

- Fixed npm publish: `index.js` and `index.d.ts` (generated JS loader and types) are now committed to the repo so they're included in the published package.
- Fixed release workflow artifact download path for napi platform binaries.
- Fixed CLI bin script to use `createRequire` for CJS/ESM compatibility.

## 0.3.3

### Added

- **CLI in npm package** — `npm install svgm-node` now provides a `svgm` command. Supports input files, `-o`, `--stdout`, `--preset`, `--precision`, `--dry-run`, `--quiet`, and pipe detection. Zero dependencies (uses Node's built-in `parseArgs`). For the full CLI (recursive mode, config files, progress bars), use `cargo install svgm`.

## 0.3.2

### Added

- **Node.js native bindings** via napi-rs, published as `svgm-node` on npm (`npm install svgm-node`). Prebuilt binaries for Linux (x64 glibc/musl), macOS (x64/arm64), and Windows (x64). Same API surface as `svgm-wasm` — `optimize(svg, options?)` and `version()`.
- CI now builds and tests the napi crate alongside WASM.
- Release workflow publishes `svgm-node` to npm with provenance on version tags.

## 0.3.1

### Changed

- **WASM package renamed** from `@frmwrksoftware/wasm` to `svgm-wasm` (`npm install svgm-wasm`).
- npm publishes now include provenance attestation.

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
