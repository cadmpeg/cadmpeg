// SPDX-License-Identifier: Apache-2.0
//! Member-local capture identity on the session's existing transaction journal.

use std::sync::atomic::{AtomicU64, Ordering};

use super::{ReadTransaction, SessionHandle};
use crate::decode::DecodeContext;
use crate::CodecError;

static NEXT_BATCH: AtomicU64 = AtomicU64::new(0);

/// Opaque identity of one transactional record-capture namespace.
///
/// This token identifies captures after final extraction. It carries no source
/// extent, read coverage or authority to export an unfinished transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordBatchId(u64);

impl RecordBatchId {
    fn issue() -> Result<Self, CodecError> {
        NEXT_BATCH
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map(Self)
            .map_err(|_| CodecError::malformed("record batch identities exhausted"))
    }
}

impl SessionHandle<'_> {
    fn assign_batch(self, token: usize) -> Result<RecordBatchId, CodecError> {
        self.session(|session| {
            let frame = session
                .frames
                .last_mut()
                .filter(|frame| {
                    frame.token == token && frame.record.is_none() && frame.batch.is_none()
                })
                .ok_or_else(|| CodecError::malformed("record batch has no current transaction"))?;
            let batch = RecordBatchId::issue()?;
            frame.batch = Some(batch);
            Ok(batch)
        })
    }
}

impl DecodeContext<'_> {
    /// Build member-local records on this context's genuine views and budget.
    ///
    /// Record IDs are unique within this batch. Nested batches have their own
    /// IDs. A failed batch discards its captures and reads; successful captures
    /// remain staged until every ancestor commits. Only the outermost final
    /// `take_record_captures` exports them and seals the session.
    pub fn record_batch<T>(
        &self,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<(T, RecordBatchId), CodecError> {
        let transaction = ReadTransaction::begin(self.budget, None)?;
        let batch = self.budget.assign_batch(transaction.token)?;
        let value = decode();
        if let Some(limit) = self.budget.fused() {
            return Err(limit.into());
        }
        let value = value?;
        transaction.commit()?;
        Ok((value, batch))
    }
}

#[cfg(test)]
mod tests;
