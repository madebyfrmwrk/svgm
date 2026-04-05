use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

pub struct RemoveProcInst;

impl Pass for RemoveProcInst {
    fn name(&self) -> &'static str {
        "removeProcInst"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();
        for id in ids {
            if matches!(doc.node(id).kind, NodeKind::ProcessingInstruction { .. }) {
                doc.remove(id);
                changed = true;
            }
        }
        if changed { PassResult::Changed } else { PassResult::Unchanged }
    }
}
