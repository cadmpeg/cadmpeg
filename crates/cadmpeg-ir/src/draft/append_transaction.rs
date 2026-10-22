// SPDX-License-Identifier: Apache-2.0
//! Tentative neutral admission within an owning parser operation.

use std::borrow::BorrowMut;

use cadmpeg_core::CodecError;

use super::{CommitSession, DraftError, ModelCheckpoint, ModelDraft};
use crate::document::{CadIr, Model};

mod annotated;
pub use annotated::AnnotatedModelAppendTransaction;

/// Restricted candidate admission that cannot edit a committed prefix.
///
/// Only plain neutral drafts can enter this transaction. Native records,
/// source records, annotations and document settings remain outside its API.
///
/// ```compile_fail
/// use cadmpeg_ir::{CadIr, draft::CommitSession};
/// let ctx = cadmpeg_test_support::service_decode_context();
/// let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
/// session.model_append_transaction(|stage| {
///     stage.document_mut().unwrap().model.points.clear();
///     Ok(Some(()))
/// }).unwrap();
/// ```
///
/// ```compile_fail
/// use cadmpeg_ir::{CadIr, draft::CommitSession};
/// let ctx = cadmpeg_test_support::service_decode_context();
/// let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
/// session.model_append_transaction(|stage| {
///     stage.document().native.namespace_mut("rhino").arenas_mut().clear();
///     Ok(Some(()))
/// }).unwrap();
/// ```
#[derive(Debug)]
pub struct ModelAppendTransaction<'session, 'ctx, D: BorrowMut<CadIr>> {
    session: &'session mut CommitSession<'ctx, D>,
    checkpoint: Option<ModelCheckpoint>,
}

impl<'ctx, D: BorrowMut<CadIr>> ModelAppendTransaction<'_, 'ctx, D> {
    /// Read the prefix and tentatively admitted candidates.
    pub fn document(&self) -> &CadIr {
        self.session.document()
    }

    /// Admit a neutral draft through the actual session's identity and reference checks.
    pub fn commit_model(
        &mut self,
        draft: ModelDraft,
    ) -> Result<Result<(), DraftError>, CodecError> {
        if let Err(error) = self.check_feature_parents(&draft.model)? {
            return Ok(Err(error));
        }
        self.session.commit_model(draft)
    }

    fn check_feature_parents(
        &mut self,
        model: &Model,
    ) -> Result<Result<(), DraftError>, CodecError> {
        for owner in model.feature_regeneration_parents.owners() {
            if self.session.contains(owner.as_str())? {
                return Ok(Err(DraftError::FeatureParents {
                    owner: owner.clone(),
                    message: "append transaction cannot change a committed feature's parent"
                        .to_owned(),
                }));
            }
        }
        Ok(Ok(()))
    }

    /// Give an independently recoverable child its own tentative suffix.
    pub fn model_append_transaction<T>(
        &mut self,
        decode: impl FnOnce(&mut ModelAppendTransaction<'_, 'ctx, D>) -> Result<Option<T>, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        self.session.model_append_transaction(decode)
    }

    /// Look up an identity in the current tentative model.
    pub fn contains(&mut self, identity: &str) -> Result<bool, CodecError> {
        self.session.contains(identity)
    }
}

impl<D: BorrowMut<CadIr>> Drop for ModelAppendTransaction<'_, '_, D> {
    fn drop(&mut self) {
        if let Some(checkpoint) = self.checkpoint.take() {
            let model = &mut self.session.state.base.borrow_mut().model;
            for feature in &model.features[checkpoint.length::<crate::features::Feature>()..] {
                model.feature_regeneration_parents.remove_owner(&feature.id);
            }
            let parents = std::mem::take(&mut model.feature_regeneration_parents);
            checkpoint.restore(model, parents);
            self.session.state.identities = None;
            self.session.state.unindexed_start = None;
            self.session.state.admission = None;
        }
    }
}

impl<'ctx, D: BorrowMut<CadIr>> CommitSession<'ctx, D> {
    /// Keep admitted neutral appends tentative until their owning operation succeeds.
    ///
    /// `Some` publishes the suffix. `None`, an error, or unwinding discards it
    /// and invalidates cached admission facts. Existing records are not cloned
    /// or exposed for mutation. Caller budgets are not restored on rollback.
    /// A shared parser part can return its actual `Result<Option<T>, CodecError>`
    /// so parent recovery and output-preflight errors discard candidate appends.
    pub fn model_append_transaction<T>(
        &mut self,
        decode: impl FnOnce(&mut ModelAppendTransaction<'_, 'ctx, D>) -> Result<Option<T>, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        if let Some(limit) = self.ctx.resource_refusal() {
            return Err(limit.into());
        }
        let reads = self.ctx.read_transaction()?;
        let checkpoint = ModelCheckpoint::with_parents(
            &self.document().model,
            crate::document::FeatureRegenerationParents::default(),
        );
        let mut stage = ModelAppendTransaction {
            session: self,
            checkpoint: Some(checkpoint),
        };
        let result = decode(&mut stage);
        if let Some(limit) = stage.session.ctx.resource_refusal() {
            return Err(limit.into());
        }
        if result.as_ref().is_ok_and(Option::is_some) {
            reads.commit()?;
            stage.checkpoint = None;
        }
        result
    }
}

#[cfg(test)]
mod tests;
