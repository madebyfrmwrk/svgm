use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

pub struct RemoveDoctype;

impl Pass for RemoveDoctype {
    fn name(&self) -> &'static str {
        "removeDoctype"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        for id in ids {
            if matches!(doc.node(id).kind, NodeKind::Doctype(_)) {
                doc.remove(id);
                changed = true;
            }
        }
        if changed { PassResult::Changed } else { PassResult::Unchanged }
    }
}
