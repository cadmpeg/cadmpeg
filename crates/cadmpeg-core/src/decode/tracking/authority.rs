// SPDX-License-Identifier: Apache-2.0
//! Opaque session identity for transactional producer handoffs.

use std::sync::atomic::{AtomicU64, Ordering};

use super::{RecordBatchId, SessionHandle};
use crate::decode::DecodeContext;
use crate::CodecError;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(0);

/// Identity of a live decode session, without access to its source or budget.
///
/// ```compile_fail
/// use cadmpeg_core::decode::DecodeSessionToken;
/// let forged = DecodeSessionToken(0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeSessionToken(u64);

impl SessionHandle<'_> {
    fn session_token(self) -> Result<DecodeSessionToken, CodecError> {
        self.session(|session| {
            if let Some(token) = session.identity {
                return Ok(token);
            }
            let token = NEXT_SESSION
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    value.checked_add(1)
                })
                .map(DecodeSessionToken)
                .map_err(|_| CodecError::malformed("decode session identities exhausted"))?;
            session.identity = Some(token);
            Ok(token)
        })
    }
}

impl SessionHandle<'_> {
    fn current_record_batch(self) -> Result<Option<RecordBatchId>, CodecError> {
        self.session(|session| Ok(session.frames.iter().rev().find_map(|frame| frame.batch)))
    }
}

impl DecodeContext<'_> {
    /// Return the actual innermost record namespace of the current transaction.
    pub fn current_record_batch(&self) -> Result<Option<RecordBatchId>, CodecError> {
        self.budget.current_record_batch()
    }

    /// Return this session's identity after checking its live refusal channel.
    pub fn session_token(&self) -> Result<DecodeSessionToken, CodecError> {
        self.budget.session_token()
    }
}

#[cfg(test)]
mod tests;
