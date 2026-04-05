use crate::ast::Document;
use crate::passes::{self, Pass, PassResult};

const MAX_ITERATIONS: usize = 10;

pub struct OptimizeResult {
    pub iterations: usize,
}

/// Run all default passes in a fixed-point loop until no pass changes the document.
pub fn optimize(doc: &mut Document) -> OptimizeResult {
    let passes = passes::default_passes();
    optimize_with_passes(doc, &passes)
}

/// Run a given set of passes in a fixed-point loop.
pub fn optimize_with_passes(doc: &mut Document, passes: &[Box<dyn Pass>]) -> OptimizeResult {
    let mut iterations = 0;

    for _ in 0..MAX_ITERATIONS {
        iterations += 1;
        let mut any_changed = false;

        for pass in passes {
            let result = pass.run(doc);
            if result == PassResult::Changed {
                any_changed = true;
            }
        }

        if !any_changed {
            break;
        }
    }

    OptimizeResult { iterations }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn converges_on_simple_svg() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- comment --><rect width="50" height="50"/></svg>"#;
        let mut doc = parse(input).unwrap();
        let result = optimize(&mut doc);
        let output = serialize(&doc);
        // Comment should be removed
        assert!(!output.contains("<!--"));
        // Should converge quickly
        assert!(result.iterations <= 3);
    }

    #[test]
    fn converges_in_bounded_iterations() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- a --><!-- b --><metadata><stuff/></metadata><g></g><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        let result = optimize(&mut doc);
        let output = serialize(&doc);
        assert!(!output.contains("<!--"));
        assert!(!output.contains("metadata"));
        assert!(!output.contains("<g>"));
        assert!(result.iterations <= MAX_ITERATIONS);
    }
}
