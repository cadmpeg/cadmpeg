// SPDX-License-Identifier: Apache-2.0
//! Admitted indices for decoded PMI annotations.

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition, PmiTarget};

/// Fields supplied before the source record ID is minted for the IR arena.
pub(super) struct AnnotationDraft {
    pub(super) name: Option<String>,
    pub(super) targets: Vec<PmiTarget>,
    pub(super) visible: Option<bool>,
    pub(super) definition: PmiDefinition,
}

/// An index minted by insertion into the PMI arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct AnnotationIndex(usize);

impl AnnotationIndex {
    /// The inserted annotation’s arena position.
    pub(super) fn get(self) -> usize {
        self.0
    }
}

/// STEP records mapped to inserted PMI annotations.
#[derive(Default)]
pub(super) struct Annotations {
    indices: BTreeMap<u64, AnnotationIndex>,
}

impl Annotations {
    /// Insert an annotation and return its arena index.
    pub(super) fn push(
        &mut self,
        ir: &mut CadIr,
        id: u64,
        draft: AnnotationDraft,
    ) -> AnnotationIndex {
        let index = AnnotationIndex(ir.model.pmi.len());
        ir.model.pmi.push(PmiAnnotation {
            id: super::pmi_id(id),
            name: draft.name.filter(|value| !value.is_empty()),
            targets: draft.targets,
            visible: draft.visible,
            definition: draft.definition,
        });
        self.indices.insert(id, index);
        index
    }

    /// The inserted annotation index for a STEP record.
    pub(super) fn get(&self, id: u64) -> Option<AnnotationIndex> {
        self.indices.get(&id).copied()
    }
}
