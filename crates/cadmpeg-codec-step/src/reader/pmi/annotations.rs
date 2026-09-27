// SPDX-License-Identifier: Apache-2.0
//! Admitted indices for decoded PMI annotations.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition, PmiTarget};

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
        ctx: Option<&DecodeContext<'_>>,
        ir: &mut CadIr,
        id: u64,
        name: Option<String>,
        targets: Vec<PmiTarget>,
        visible: Option<bool>,
        definition: PmiDefinition,
    ) -> Result<AnnotationIndex, CodecError> {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "step_pmi_annotation_arena")?;
            ctx.charge_collection_items(1, "step_pmi_annotation_index")?;
        }
        ir.model.pmi.try_reserve(1).map_err(|_| {
            refuse_annotation(ctx, "step_pmi_annotation_arena")
        })?;
        let index = AnnotationIndex(ir.model.pmi.len());
        ir.model.pmi.push(PmiAnnotation {
            id: super::pmi_id(id),
            name: name.filter(|value| !value.is_empty()),
            visible,
            targets,
            definition,
        });
        self.indices.insert(id, index);
        Ok(index)
    }

    /// The inserted annotation index for a STEP record.
    pub(super) fn get(&self, id: u64) -> Option<AnnotationIndex> {
        self.indices.get(&id).copied()
    }
}

fn refuse_annotation(ctx: Option<&DecodeContext<'_>>, operation: &'static str) -> CodecError {
    match ctx {
        Some(ctx) => ctx.refuse_codec_limit(operation, 0, 1),
        None => cadmpeg_core::decode::refuse_local_limit(operation, 0, 1),
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::pmi::PmiDefinition;

    use super::Annotations;

    fn refusal_at(limit: u64) -> CodecError {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits policy");
        let mut ir = CadIr::empty();
        Annotations::default()
            .push(
                Some(&ctx),
                &mut ir,
                1,
                None,
                Vec::new(),
                None,
                PmiDefinition::Datum {
                    identification: String::new(),
                },
            )
            .expect_err("limit must refuse one annotation")
    }

    #[test]
    fn pmi_annotation_arena_refuses_collection_limit() {
        assert!(matches!(
            refusal_at(0),
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "step_pmi_annotation_arena"
        ));
    }

    #[test]
    fn pmi_annotation_index_refuses_collection_limit() {
        assert!(matches!(
            refusal_at(1),
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "step_pmi_annotation_index"
        ));
    }
}
