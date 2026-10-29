// SPDX-License-Identifier: Apache-2.0
//! Joint rollback for admitted neutral candidates and their owned exactness.

use std::borrow::BorrowMut;

use cadmpeg_core::CodecError;

use super::ModelAppendTransaction;
use crate::annotations::{AnnotationBuilder, Annotations};
use crate::document::CadIr;
use crate::draft::{
    identity_index_contains, index_model_identities, ArenaEntity, CommitSession, DraftAccounting,
    DraftError, ModelCheckpoint, ModelDraft,
};
use crate::schema::EntitySchema;

/// Candidate admission with exclusive ownership of the annotation destination.
///
/// Only annotations belonging to new candidate records can be written. Prefix
/// annotations remain unchanged when a child or parent operation gives up.
///
/// ```compile_fail
/// use cadmpeg_ir::{CadIr, Annotations, draft::CommitSession};
/// let ctx = cadmpeg_test_support::service_decode_context();
/// let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
/// let mut annotations = Annotations::default();
/// session.annotated_model_append_transaction(&mut annotations, |stage| {
///     stage.annotations_mut().provenance.clear();
///     Ok(Some(()))
/// }).unwrap();
/// ```
#[derive(Debug)]
pub struct AnnotatedModelAppendTransaction<'annotations, 'session, 'ctx, D: BorrowMut<CadIr>> {
    model: ModelAppendTransaction<'session, 'ctx, D>,
    annotations: &'annotations mut Annotations,
}

impl<'ctx, D: BorrowMut<CadIr>> AnnotatedModelAppendTransaction<'_, '_, 'ctx, D> {
    /// Read committed and tentatively admitted neutral records.
    pub fn document(&self) -> &CadIr {
        self.model.document()
    }

    /// Read annotations without exposing a mutable prefix.
    pub fn annotations(&self) -> &Annotations {
        self.annotations
    }

    /// Admit records and their owned exactness through the actual commit session.
    pub fn commit(
        &mut self,
        mut draft: ModelDraft<DraftAccounting>,
    ) -> Result<Result<(), DraftError>, CodecError> {
        if let Err(error) = self.model.check_feature_parents(&draft.model)? {
            return Ok(Err(error));
        }
        macro_rules! check_annotation_prefix {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                for entity in &draft.model.$field {
                    let id = entity.identity();
                    if self.annotations.provenance.contains_key(id)
                        || self.annotations.exactness().contains_key(id)
                    {
                        return Ok(Err(DraftError::Admission(format!(
                            "append transaction cannot replace existing annotations for {id}"
                        ))));
                    }
                }
            )*};
        }
        crate::document::arena_registry!(check_annotation_prefix);
        let index = match draft.insertion_index.take() {
            Some(index) => index,
            None => match index_model_identities(&draft.model) {
                Ok(index) => index,
                Err(id) => return Ok(Err(DraftError::IdentityCollision(id.to_owned()))),
            },
        };
        for id in draft.accounting.exactness.keys() {
            if !identity_index_contains(&draft.model, &index, id) {
                return Ok(Err(DraftError::Admission(format!(
                    "exactness annotation {id} does not belong to the candidate"
                ))));
            }
        }
        draft.insertion_index = Some(index);
        self.model.session.commit(draft, self.annotations)
    }

    /// Look up an actual admitted identity.
    pub fn contains(&mut self, identity: &str) -> Result<bool, CodecError> {
        self.model.contains(identity)
    }

    /// Stage a child's records and exactness under their own recovery boundary.
    pub fn annotated_model_append_transaction<T>(
        &mut self,
        decode: impl FnOnce(
            &mut AnnotatedModelAppendTransaction<'_, '_, 'ctx, D>,
        ) -> Result<Option<T>, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        self.model
            .session
            .annotated_model_append_transaction(self.annotations, decode)
    }
}

impl<D: BorrowMut<CadIr>> Drop for AnnotatedModelAppendTransaction<'_, '_, '_, D> {
    fn drop(&mut self) {
        if let Some(checkpoint) = &self.model.checkpoint {
            fn remove_suffix<T: ArenaEntity>(
                checkpoint: &ModelCheckpoint,
                arena: &[T],
                annotations: &mut AnnotationBuilder,
            ) {
                for entity in &arena[checkpoint.arena_len::<T>()..] {
                    annotations.remove_entity(entity.identity());
                }
            }
            let mut annotations = AnnotationBuilder::resume(std::mem::take(self.annotations));
            macro_rules! remove_suffix_annotations {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    remove_suffix(checkpoint, &self.model.document().model.$field, &mut annotations);
                )*};
            }
            crate::document::arena_registry!(remove_suffix_annotations);
            *self.annotations = annotations.build();
        }
    }
}

impl<'ctx, D: BorrowMut<CadIr>> CommitSession<'ctx, D> {
    /// Keep neutral candidates and owned exactness tentative through one operation.
    ///
    /// `None`, an error, unwinding or a latched caller refusal removes only the
    /// new records and their annotations. Prefix content and caller budgets are
    /// unchanged. The callback cannot mutate native records, source records,
    /// document settings or the annotation prefix.
    pub fn annotated_model_append_transaction<T>(
        &mut self,
        annotations: &mut Annotations,
        decode: impl FnOnce(
            &mut AnnotatedModelAppendTransaction<'_, '_, 'ctx, D>,
        ) -> Result<Option<T>, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        if let Some(limit) = self.ctx.resource_refusal() {
            return Err(limit.into());
        }
        let checkpoint = ModelCheckpoint::with_parents(
            &self.document().model,
            crate::document::FeatureRegenerationParents::default(),
        );
        let mut stage = AnnotatedModelAppendTransaction {
            model: ModelAppendTransaction {
                session: self,
                checkpoint: Some(checkpoint),
            },
            annotations,
        };
        let result = decode(&mut stage);
        if let Some(limit) = stage.model.session.ctx.resource_refusal() {
            return Err(limit.into());
        }
        if result.as_ref().is_ok_and(Option::is_some) {
            stage.model.checkpoint = None;
        }
        result
    }
}

#[cfg(test)]
mod tests;
