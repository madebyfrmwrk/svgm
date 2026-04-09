use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TS_TYPES: &str = r#"
export interface OptimizeOptions {
    preset?: "safe" | "default" | "balanced" | "aggressive";
    precision?: number;
    passes?: Record<string, boolean>;
}

export interface OptimizeResult {
    data: string;
    iterations: number;
}

export function optimize(svg: string, options?: OptimizeOptions): OptimizeResult;
export function version(): string;
"#;

#[derive(Deserialize, Default)]
struct Options {
    preset: Option<String>,
    precision: Option<u32>,
    passes: Option<HashMap<String, bool>>,
}

#[derive(Serialize)]
struct Output {
    data: String,
    iterations: usize,
}

#[wasm_bindgen(skip_typescript)]
pub fn optimize(svg: &str, options: JsValue) -> Result<JsValue, JsError> {
    let opts: Options = if options.is_undefined() || options.is_null() {
        Options::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(|e| JsError::new(&e.to_string()))?
    };

    let preset = match opts.preset.as_deref() {
        Some("safe") => svgm_core::Preset::Safe,
        Some("default") | Some("balanced") | Some("aggressive") | None => {
            svgm_core::Preset::Default
        }
        Some(other) => return Err(JsError::new(&format!("unknown preset: {other}"))),
    };

    if let Some(ref passes) = opts.passes {
        let known = svgm_core::config::all_pass_names();
        for name in passes.keys() {
            if !known.contains(&name.as_str()) {
                return Err(JsError::new(&format!("unknown pass: {name}")));
            }
        }
    }

    let config = svgm_core::Config {
        preset,
        precision: opts.precision,
        pass_overrides: opts.passes.unwrap_or_default(),
    };

    let result =
        svgm_core::optimize_with_config(svg, &config).map_err(|e| JsError::new(&e.to_string()))?;

    let output = Output {
        data: result.data,
        iterations: result.iterations,
    };

    serde_wasm_bindgen::to_value(&output).map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen(skip_typescript)]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
