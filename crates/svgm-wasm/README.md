# svgm-wasm

WebAssembly build of [svgm](https://github.com/madebyfrmwrk/svgm) — a fast SVG optimizer written in Rust. 33x faster than SVGO with better compression.

## Install

```bash
npm install svgm-wasm
```

## Usage

```js
import init, { optimize, version } from 'svgm-wasm';

await init();

const result = optimize('<svg xmlns="http://www.w3.org/2000/svg">...</svg>');
console.log(result); // optimized SVG string
console.log(version()); // e.g. "0.3.1"
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

## Presets

- **safe** — removal and normalization only (17 passes)
- **default** — full optimization (34 passes)

## Links

- [Website](https://svgm.dev)
- [Playground](https://svgm.dev/playground)
- [GitHub](https://github.com/madebyfrmwrk/svgm)
- [CLI on crates.io](https://crates.io/crates/svgm)
