use std::collections::HashMap;

use napi::Error;
use napi_derive::napi;

#[napi(object)]
pub struct OptimizeOptions {
    pub preset: Option<String>,
    pub precision: Option<u32>,
    pub passes: Option<HashMap<String, bool>>,
}

#[napi(object)]
pub struct OptimizeResult {
    pub data: String,
    pub iterations: u32,
}

#[napi]
pub fn optimize(svg: String, options: Option<OptimizeOptions>) -> napi::Result<OptimizeResult> {
    let opts = options.unwrap_or(OptimizeOptions {
        preset: None,
        precision: None,
        passes: None,
    });

    let preset = match opts.preset.as_deref() {
        Some("safe") => svgm_core::Preset::Safe,
        Some("default") | Some("balanced") | Some("aggressive") | None => {
            svgm_core::Preset::Default
        }
        Some(other) => return Err(Error::from_reason(format!("unknown preset: {other}"))),
    };

    if let Some(ref passes) = opts.passes {
        let known = svgm_core::config::all_pass_names();
        for name in passes.keys() {
            if !known.contains(&name.as_str()) {
                return Err(Error::from_reason(format!("unknown pass: {name}")));
            }
        }
    }

    if let Some(p) = opts.precision
        && p > svgm_core::config::MAX_PRECISION
    {
        return Err(Error::from_reason(format!(
            "precision must be between 0 and {}",
            svgm_core::config::MAX_PRECISION
        )));
    }

    let config = svgm_core::Config {
        preset,
        precision: opts.precision,
        pass_overrides: opts.passes.unwrap_or_default(),
    };

    // A panic inside the optimizer must surface as a JS exception, not abort
    // the Node process.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        svgm_core::optimize_with_config(&svg, &config)
    }))
    .map_err(|payload| {
        let msg = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown panic".to_string());
        Error::from_reason(format!("internal error while optimizing: {msg}"))
    })?
    .map_err(|e| Error::from_reason(e.to_string()))?;

    Ok(OptimizeResult {
        data: result.data,
        iterations: result.iterations as u32,
    })
}

#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
