<p align="center">
  <br>
  <a href="https://github.com/madebyfrmwrk/svgm">
    <img alt="svgm" src="https://raw.githubusercontent.com/madebyfrmwrk/svgm/main/assets/svgm-light.svg" height="72">
  </a>
  <br>
  <br>
</p>

<p align="center">
  SVG optimization for Node.js — native addon powered by <a href="https://github.com/madebyfrmwrk/svgm">svgm</a>.
</p>

<div align="center">

[![npm version][npm-badge]][npm-url]
[![MIT licensed][license-badge]][license-url]
[![Build Status][ci-badge]][ci-url]

</div>

---

33x faster than SVGO with better compression. Native Rust performance via [napi-rs](https://napi.rs), no WASM overhead.

## Install

```bash
npm install svgm-node        # as a project dependency
npm install -g svgm-node     # global install for CLI usage
```

Prebuilt binaries are available for:

| Platform | Architecture |
|:--|:--|
| Linux | x64 (glibc), x64 (musl) |
| macOS | x64, arm64 (Apple Silicon) |
| Windows | x64 |

## CLI

Installing the package also provides a `svgm` command:

```bash
svgm icon.svg                    # Optimize in place
svgm icon.svg -o icon.min.svg    # Write to different file
svgm icon.svg --stdout           # Print to stdout
svgm icon.svg --dry-run          # Preview without writing
svgm icon.svg --preset safe      # Safe preset (20 passes)
svgm icon.svg --precision 2      # Override numeric precision
svgm *.svg                       # Multiple files
```

When piped, output goes to stdout automatically.

> For the full-featured CLI (recursive directory mode, config files, progress bars), install via Rust: `cargo install svgm`

## JavaScript API

```js
const { optimize, version } = require('svgm-node');

const result = optimize('<svg xmlns="http://www.w3.org/2000/svg">...</svg>');
console.log(result.data);       // optimized SVG string
console.log(result.iterations); // convergence count
console.log(version());         // e.g. "0.3.2"
```

### ESM

```js
import { optimize, version } from 'svgm-node';
```

### With options

```js
const result = optimize(svgString, {
  preset: 'safe',       // "safe" | "default"
  precision: 2,         // numeric precision (default: 3)
  passes: {
    removeDesc: true,    // enable opt-in passes
    mergePaths: false,   // disable specific passes
  },
});
```

## API

### `optimize(svg: string, options?: OptimizeOptions): OptimizeResult`

Optimizes an SVG string. Throws on invalid SVG input.

### `version(): string`

Returns the svgm version.

### Types

```typescript
interface OptimizeOptions {
  preset?: string;                    // "safe" | "default"
  precision?: number;                 // numeric precision (default: 3)
  passes?: Record<string, boolean>;   // per-pass enable/disable
}

interface OptimizeResult {
  data: string;       // optimized SVG
  iterations: number; // convergence iterations
}
```

## Presets

- **safe** — removal and normalization only (20 passes)
- **default** — full optimization (34 passes)

## Links

- [Website](https://svgm.dev)
- [Playground](https://svgm.dev/playground)
- [GitHub](https://github.com/madebyfrmwrk/svgm)
- [CLI on crates.io](https://crates.io/crates/svgm)
- [WASM build](https://www.npmjs.com/package/svgm-wasm)

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE).

[npm-badge]: https://img.shields.io/npm/v/svgm-node.svg
[npm-url]: https://www.npmjs.com/package/svgm-node
[license-badge]: https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg
[license-url]: https://github.com/madebyfrmwrk/svgm/blob/main/LICENSE-MIT
[ci-badge]: https://github.com/madebyfrmwrk/svgm/actions/workflows/ci.yml/badge.svg
[ci-url]: https://github.com/madebyfrmwrk/svgm/actions
