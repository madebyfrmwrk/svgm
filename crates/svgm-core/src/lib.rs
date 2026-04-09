pub mod ast;
pub mod config;
pub mod optimizer;
pub mod parser;
pub mod passes;
pub mod serializer;

pub use config::{Config, Preset};
use parser::ParseError;

pub struct OptimizeOutput {
    pub data: String,
    pub iterations: usize,
}

/// Optimize an SVG string using the Default preset.
/// Returns the optimized SVG string and the number of convergence iterations.
pub fn optimize(input: &str) -> Result<OptimizeOutput, ParseError> {
    optimize_with_config(input, &Config::default())
}

/// Optimize an SVG string with the given configuration.
pub fn optimize_with_config(input: &str, config: &Config) -> Result<OptimizeOutput, ParseError> {
    let mut doc = parser::parse(input)?;
    let passes = config::passes_for_config(config);
    let result = optimizer::optimize_with_passes(&mut doc, &passes);
    let output = serializer::serialize(&doc);
    Ok(OptimizeOutput {
        data: output,
        iterations: result.iterations,
    })
}
