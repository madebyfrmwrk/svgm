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
///
/// Stops when an iteration changes nothing, after `MAX_ITERATIONS`, or when
/// an iteration *grows* the document (that iteration is rolled back). The
/// first iteration may legitimately grow it (e.g. inlining a stylesheet onto
/// many elements), but every later iteration must be monotone; growth there
/// means a pass is misbehaving and the loop would otherwise amplify it
/// exponentially (issue #23 grew a 100-byte file to 2 MB this way). Mirrors
/// SVGO's multipass, which stops as soon as the output stops shrinking.
pub fn optimize_with_passes(doc: &mut Document, passes: &[Box<dyn Pass>]) -> OptimizeResult {
    let mut iterations = 0;
    let mut prev_size: Option<usize> = None;

    for _ in 0..MAX_ITERATIONS {
        // From the second iteration on, keep a snapshot so a growing
        // iteration can be rolled back rather than emitted.
        let snapshot = prev_size.map(|_| doc.clone());
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

        let size = doc.byte_estimate();
        if let (Some(prev), Some(snapshot)) = (prev_size, snapshot)
            && size > prev
        {
            *doc = snapshot;
            iterations -= 1;
            break;
        }
        prev_size = Some(size);
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

    /// A deliberately broken pass that doubles the first text node it finds.
    struct Doubler;
    impl Pass for Doubler {
        fn name(&self) -> &'static str {
            "doubler"
        }
        fn run(&self, doc: &mut Document) -> PassResult {
            for id in doc.traverse() {
                if let crate::ast::NodeKind::Text(t) = &mut doc.node_mut(id).kind {
                    let copy = t.clone();
                    t.push_str(&copy);
                    return PassResult::Changed;
                }
            }
            PassResult::Unchanged
        }
    }

    #[test]
    fn stops_when_an_iteration_grows_the_document() {
        // Without the guard a growing pass is amplified MAX_ITERATIONS times
        // (issue #23 turned 100 bytes into 2 MB this way).
        let mut doc =
            parse(r#"<svg xmlns="http://www.w3.org/2000/svg"><style>a</style></svg>"#).unwrap();
        let passes: Vec<Box<dyn Pass>> = vec![Box::new(Doubler)];
        let result = optimize_with_passes(&mut doc, &passes);
        // Iteration 1 (a -> aa) is kept; iteration 2 (aa -> aaaa) grew the
        // document and is rolled back.
        assert_eq!(result.iterations, 1);
        assert!(serialize(&doc).contains("<style>aa</style>"));
    }
}
