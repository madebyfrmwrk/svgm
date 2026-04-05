# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**svgm** (SVG Minimizer) is a Rust-based SVG optimizer designed as a modern replacement for SVGO. Key differentiator: fixed-point convergence in one invocation — no need to run multiple times.

## Build & Development

```bash
source "$HOME/.cargo/env"  # if Rust was just installed

cargo build                 # build all crates
cargo build --release       # release binary
cargo test --workspace      # all unit + integration tests
cargo test -p svgm-core     # core library tests only
cargo test -p svgm-core --test integration  # integration tests only
cargo test <test_name>      # run a single test by name
cargo clippy --workspace    # lint (must pass with zero warnings)
cargo fmt                   # format
cargo run -- input.svg -o output.svg  # run the CLI
```

## Architecture

Cargo workspace with two crates:

- **`crates/svgm-core`** — Core optimization engine
  - `ast.rs` — Arena-based document model (`Document`, `Node`, `Element`, `Attribute`). Nodes stored in `Vec<Node>` with `NodeId` indices. O(1) parent access, mark-and-sweep removal.
  - `parser.rs` — Uses `xmlparser` crate to tokenize XML, builds the `Document` tree. Decodes XML entities.
  - `serializer.rs` — Converts `Document` back to minified SVG string. Handles self-closing elements, entity escaping.
  - `optimizer.rs` — Fixed-point convergence loop. Runs all passes until no pass reports `Changed`, max 10 iterations. Operates entirely on the in-memory AST (no re-parsing between iterations).
  - `passes/` — Optimization passes implementing the `Pass` trait. Each returns `Changed` or `Unchanged`.
  - `lib.rs` — Public API: `svgm_core::optimize(svg_string) -> OptimizeOutput`

- **`crates/svgm-cli`** — CLI binary using clap + indicatif for spinner UX

### Pass Trait

```rust
pub trait Pass {
    fn name(&self) -> &'static str;
    fn run(&self, doc: &mut Document) -> PassResult;  // Changed | Unchanged
}
```

### Safety Tiers

- **Safe (default)**: removal passes (comments, metadata, editor data, empty containers, empty text/attrs, doctype, PI, unused namespaces) + normalization (whitespace cleanup, numeric values, color shortening, whitespace minification)
- **Opt-in**: `remove_desc` (accessibility concern — `<desc>` has semantic meaning)
- **Balanced/Aggressive**: planned post-MVP (structural, geometry, reference-aware passes)

### Test Fixtures

- `crates/svgm-core/tests/fixtures/synthetic/` — handcrafted edge cases
- `crates/svgm-core/tests/fixtures/real/` — real editor exports (Figma etc.)
- `crates/svgm-core/tests/fixtures/regression/` — SVGO issue reproductions

## Non-Goals for v1

- No public plugin API (internal `Pass` trait only)
- No SVGO config compatibility
- No Node.js bindings (CLI-only)
- No formatting-preserving output (minified only)
