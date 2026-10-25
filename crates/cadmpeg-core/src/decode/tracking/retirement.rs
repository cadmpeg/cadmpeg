// SPDX-License-Identifier: Apache-2.0
//! Transactional retirement of complete, genuinely produced record captures.

use super::{RecordBatchId, Session, SessionHandle};
use crate::decode::DecodeContext;
use crate::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CaptureKey {
    pub(super) batch: Option<RecordBatchId>,
    pub(super) token: usize,
}
/// Opaque authority over one actual source producer, including all continuations.
///
/// It contains no editable source ranges or read map. A rolled-back declaration,
/// another session or batch, an active producer, or sealed captures cannot be retired.
#[derive(Debug)]
pub struct RecordCaptureAuthority {
    session: std::sync::Weak<()>,
    key: CaptureKey,
}

impl DecodeContext<'_> {
    /// Attach existing source provenance when registering its actual native or model row.
    ///
    /// Source-less authored records have no authority. This operation grants no reads.
    pub fn record_capture_authority(
        &self,
        id: &str,
    ) -> Result<Option<RecordCaptureAuthority>, CodecError> {
        self.budget.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed(
                    "record captures have already been extracted",
                ));
            }
            let batch = session.frames.iter().rev().find_map(|frame| frame.batch);
            let primary = session.primaries.find(batch, id);
            if primary.is_some_and(|(_, primary)| primary.retired) {
                return Err(CodecError::malformed("record capture producer was retired"));
            }
            let token = primary.map(|(token, _)| token);
            token
                .map(|token| authority(session, CaptureKey { batch, token }))
                .transpose()
        })
    }

    /// Retire every source fragment of the actual producer named by this authority.
    ///
    /// Retirement stages under the current read transaction and rolls back with it.
    /// It cannot edit a surviving record's reads or retire an active producer scope.
    pub fn retire_record_capture(&self, proof: &RecordCaptureAuthority) -> Result<(), CodecError> {
        self.retire_record_captures(std::iter::once(proof))
    }

    /// Atomically retire a dependency closure's actual producer authorities.
    ///
    /// Every proof is validated before any capture is changed.
    pub fn retire_record_captures<'proof>(
        &self,
        proofs: impl IntoIterator<Item = &'proof RecordCaptureAuthority>,
    ) -> Result<(), CodecError> {
        let mut proofs = proofs.into_iter().peekable();
        if proofs.peek().is_none() {
            return Ok(());
        }
        self.budget.session(|session| {
            let mut keys = Vec::new();
            for proof in proofs {
                let identity = proof
                    .session
                    .upgrade()
                    .ok_or_else(|| CodecError::malformed("record capture session has ended"))?;
                if !session
                    .source_identity
                    .as_ref()
                    .is_some_and(|actual| std::sync::Arc::ptr_eq(actual, &identity))
                {
                    return Err(CodecError::malformed(
                        "record capture authority belongs to another decode session",
                    ));
                }
                if session.sealed
                    || session.frames.iter().rev().find_map(|frame| frame.batch) != proof.key.batch
                {
                    return Err(CodecError::malformed(
                        "record capture authority batch is not active",
                    ));
                }
                let primary = session
                    .primaries
                    .get(proof.key.token)
                    .filter(|primary| primary.batch == proof.key.batch)
                    .ok_or_else(|| {
                        CodecError::malformed("record capture producer was rolled back or retired")
                    })?;
                if primary.active {
                    return Err(CodecError::malformed(
                        "record capture producer is still active",
                    ));
                }
                super::reserve_working(&session.budget, &mut keys, 1)?;
                keys.push(proof.key);
            }
            keys.sort_unstable_by_key(|key| key.token);
            keys.dedup();
            if let Some(frame) = session.frames.last_mut() {
                super::reserve_working(&session.budget, &mut frame.retired, keys.len())?;
                for key in keys {
                    if session
                        .primaries
                        .get(key.token)
                        .is_some_and(|primary| !primary.retired)
                    {
                        session.primaries.set_retired(key.token, true);
                        frame.retired.push(key);
                    }
                }
            } else {
                for key in &keys {
                    session.primaries.set_retired(key.token, true);
                }
                session.committed.retain(|record| {
                    !record.owner.is_some_and(|owner| {
                        session
                            .primaries
                            .get(owner.token())
                            .is_some_and(|primary| primary.retired)
                    })
                });
                for key in keys {
                    session.primaries.remove(key.token);
                }
            }
            Ok(())
        })
    }
}

impl SessionHandle<'_> {
    pub(super) fn authority_for_owner(
        self,
        token: usize,
    ) -> Result<RecordCaptureAuthority, CodecError> {
        self.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed(
                    "record captures have already been extracted",
                ));
            }
            let batch = session.frames.iter().rev().find_map(|frame| frame.batch);
            let primary = session.primaries.get(token);
            if primary.is_none_or(|primary| primary.batch != batch) {
                return Err(CodecError::malformed(
                    "record owner declaration is not live in this batch",
                ));
            }
            authority(session, CaptureKey { batch, token })
        })
    }
}

fn authority(session: &mut Session, key: CaptureKey) -> Result<RecordCaptureAuthority, CodecError> {
    Ok(RecordCaptureAuthority {
        session: super::source_identity(session)?,
        key,
    })
}

#[cfg(test)]
mod tests;
