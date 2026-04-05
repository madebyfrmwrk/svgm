pub mod ast;
pub mod optimizer;
pub mod parser;
pub mod passes;
pub mod serializer;

use parser::ParseError;

/// Optimize an SVG string using the default safe passes.
/// Returns the optimized SVG string and the number of convergence iterations.
pub fn optimize(input: &str) -> Result<OptimizeOutput, ParseError> {
    let mut doc = parser::parse(input)?;
    let result = optimizer::optimize(&mut doc);
    let output = serializer::serialize(&doc);
    Ok(OptimizeOutput {
        data: output,
        iterations: result.iterations,
    })
}

pub struct OptimizeOutput {
    pub data: String,
    pub iterations: usize,
}
