# @frmwrksoftware/wasm

WebAssembly build of [svgm](https://github.com/madebyfrmwrk/svgm), a fast SVG optimizer written in Rust.

## Install

```bash
npm install @frmwrksoftware/wasm
```

## Usage

```js
import init, { optimize, version } from '@frmwrksoftware/wasm';

await init();

const result = optimize('<svg xmlns="http://www.w3.org/2000/svg">...</svg>');
console.log(result); // optimized SVG string

console.log(version()); // e.g. "0.2.2"
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

- **safe** — removal and normalization only
- **default** — full optimization (all passes enabled)

## Links

- [GitHub](https://github.com/madebyfrmwrk/svgm)
- [Playground](https://svgm.dev/playground)
- [CLI on crates.io](https://crates.io/crates/svgm)
