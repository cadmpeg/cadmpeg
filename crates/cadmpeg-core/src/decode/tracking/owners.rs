// SPDX-License-Identifier: Apache-2.0
//! Transactional continuations for records produced from disjoint source parts.

use super::{
    merge_range, reads_entry_bytes, DecodeBudget, ReadTransaction, RecordCapture, SessionHandle,
};
use crate::decode::{DecodeContext, View};
use crate::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CaptureOwner {
    token: usize,
    primary: bool,
}

impl CaptureOwner {
    pub(super) const fn primary(token: usize) -> Self {
        Self {
            token,
            primary: true,
        }
    }
    pub(super) const fn token(self) -> usize {
        self.token
    }
    pub(super) const fn is_continuation(self) -> bool {
        !self.primary
    }
}

/// Authority to continue one record in its original decode session and batch.
///
/// The token exposes no source ranges or read map. A rollback of its declaration
/// invalidates it, and final capture extraction seals all continuations.
#[derive(Debug)]
pub struct RecordOwner<'a> {
    session: SessionHandle<'a>,
    token: usize,
}

impl<'a> DecodeContext<'a> {
    /// Produce a record's primary source fragment and its continuation token.
    ///
    /// The returned token remains subject to every enclosing transaction. Its
    /// primary extent is the final layout extent, not the extent of later parts.
    pub fn record_owner<T>(
        &self,
        id: &str,
        primary: View<'_>,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<(T, RecordOwner<'a>), CodecError> {
        self.require_source(primary)?;
        let transaction = ReadTransaction::begin(self.budget, Some((id, primary)))?;
        let token = transaction.token;
        self.budget.declare_owner(token)?;
        let value = decode();
        if let Some(limit) = self.budget.fused() {
            return Err(limit.into());
        }
        let value = value?;
        transaction.commit()?;
        Ok((
            value,
            RecordOwner {
                session: self.budget,
                token,
            },
        ))
    }

    /// Produce a sequential primary fragment once, then permit disjoint continuations.
    ///
    /// The primary parser's own reads must fit its consumed prefix. The owner
    /// has the same session, batch, rollback, and seal limits as `record_owner`.
    pub fn sequential_record_owner<T>(
        &self,
        id: &str,
        remaining: View<'_>,
        decode: impl FnOnce() -> Result<(T, usize), CodecError>,
    ) -> Result<(T, usize, RecordOwner<'a>), CodecError> {
        self.require_source(remaining)?;
        let transaction = ReadTransaction::begin(self.budget, Some((id, remaining)))?;
        let token = transaction.token;
        self.budget.declare_owner(token)?;
        let value = decode();
        if let Some(limit) = self.budget.fused() {
            return Err(limit.into());
        }
        let (value, consumed) = value?;
        if consumed > remaining.window().len() {
            return Err(CodecError::malformed(
                "sequential record exceeds its parent extent",
            ));
        }
        transaction.narrow_record(consumed)?;
        transaction.commit()?;
        Ok((
            value,
            consumed,
            RecordOwner {
                session: self.budget,
                token,
            },
        ))
    }

    /// Retire the complete capture of a source producer that cannot form its record.
    ///
    /// The primary declaration must still belong to this session and batch and
    /// every producer scope must have closed. Enclosing read transactions stage
    /// this retirement, so their rollback restores the original authority.
    pub fn retire_record_owner(&self, owner: &RecordOwner<'_>) -> Result<(), CodecError> {
        if !self.budget.same_session(owner.session) {
            return Err(CodecError::malformed(
                "record owner belongs to another decode session",
            ));
        }
        let authority = self.budget.authority_for_owner(owner.token)?;
        self.retire_record_capture(&authority)
    }

    /// Produce another actual source fragment of the same record.
    ///
    /// Only this opaque token permits repeated record identity. Its declaration
    /// must survive in the current batch. A failed callback discards this
    /// fragment and its children; prior successful contributions remain staged.
    pub fn record_fragment<T>(
        &self,
        owner: &RecordOwner<'_>,
        extent: View<'_>,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        self.sequential_record_fragment(owner, extent, || {
            decode().map(|value| (value, extent.window().len()))
        })
        .map(|(value, _consumed)| value)
    }

    /// Continue one actual source fragment once and finalize its consumed prefix.
    pub fn sequential_record_fragment<T>(
        &self,
        owner: &RecordOwner<'_>,
        extent: View<'_>,
        decode: impl FnOnce() -> Result<(T, usize), CodecError>,
    ) -> Result<(T, usize), CodecError> {
        self.record_fragment_impl(owner, extent, decode)
    }

    fn record_fragment_impl<T>(
        &self,
        owner: &RecordOwner<'_>,
        extent: View<'_>,
        decode: impl FnOnce() -> Result<(T, usize), CodecError>,
    ) -> Result<(T, usize), CodecError> {
        self.require_source(extent)?;
        if !self.budget.same_session(owner.session) {
            return Err(CodecError::malformed(
                "record owner belongs to another decode session",
            ));
        }
        let depth = self.budget.enter_nested("record read transaction")?;
        let token = self.budget.begin_fragment(owner.token, extent)?;
        let transaction = ReadTransaction {
            session: self.budget,
            token,
            active: true,
            _depth: depth,
        };
        let value = decode();
        if let Some(limit) = self.budget.fused() {
            return Err(limit.into());
        }
        let (value, consumed) = value?;
        if consumed > extent.window().len() {
            return Err(CodecError::malformed(
                "sequential record fragment exceeds its parent extent",
            ));
        }
        transaction.narrow_record(consumed)?;
        transaction.commit()?;
        Ok((value, consumed))
    }
}

impl SessionHandle<'_> {
    fn declare_owner(self, token: usize) -> Result<(), CodecError> {
        self.session(|session| {
            let record = session
                .frames
                .last_mut()
                .filter(|frame| frame.token == token)
                .and_then(|frame| frame.record.as_mut())
                .ok_or_else(|| CodecError::malformed("record owner has no active primary scope"))?;
            record.owner = Some(CaptureOwner {
                token,
                primary: true,
            });
            Ok(())
        })
    }

    fn begin_fragment(self, token: usize, extent: View<'_>) -> Result<usize, CodecError> {
        self.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed(
                    "record captures have already been extracted",
                ));
            }
            let primary = session
                .primaries
                .get(token)
                .ok_or_else(|| CodecError::malformed("record owner declaration was rolled back"))?;
            if primary.active {
                return Err(CodecError::malformed(
                    "record owner fragment is already active",
                ));
            }
            let batch = session.frames.iter().rev().find_map(|frame| frame.batch);
            if primary.batch != batch {
                return Err(CodecError::malformed(
                    "record owner belongs to another record batch",
                ));
            }
            if primary.retired {
                return Err(CodecError::malformed("record owner was retired"));
            }
            let mut record = session.capture(primary.id.as_str(), extent)?;
            record.owner = Some(CaptureOwner {
                token,
                primary: false,
            });
            session.push_frame(Some(record), false)
        })
    }
}

pub(super) fn coalesce(
    budget: &DecodeBudget,
    captures: &mut Vec<RecordCapture>,
) -> Result<(), CodecError> {
    captures.sort_unstable_by(|a, b| {
        a.batch
            .cmp(&b.batch)
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| {
                b.owner
                    .is_some_and(|owner| owner.primary)
                    .cmp(&a.owner.is_some_and(|owner| owner.primary))
            })
    });
    let mut error = None;
    captures.dedup_by(|later, primary| {
        if error.is_some() || later.batch != primary.batch || later.id != primary.id {
            return false;
        }
        match (primary.owner, later.owner) {
            (Some(first), Some(next))
                if first.primary && !next.primary && first.token == next.token => {}
            _ => {
                error = Some(CodecError::malformed(
                    "record fragments have no common primary owner",
                ));
                return false;
            }
        }
        // The continuation held its reads as working storage. The primary is
        // charged for what it gains: a range that a held range does not
        // cover, or a reads-table entry for a stream new to the primary.
        for (stream, ranges) in std::mem::take(&mut later.reads) {
            if let Some(held) = primary.reads.get_mut(&stream) {
                for range in ranges {
                    if let Err(failure) = merge_range(budget, held, range) {
                        error = Some(failure);
                        return false;
                    }
                }
            } else {
                let charged = reads_entry_bytes(&ranges)
                    .and_then(|entry| entry.checked_add(stream.len()))
                    .ok_or_else(|| {
                        budget.retained_allocation_failed(u64::MAX, "record reads table")
                    })
                    .and_then(|bytes| {
                        budget
                            .retain_bytes(
                                crate::decode::u64_from_index(bytes),
                                "record reads table",
                            )
                            .map_err(CodecError::from)
                    });
                if let Err(failure) = charged {
                    error = Some(failure);
                    return false;
                }
                primary.reads.insert(stream, ranges);
            }
        }
        true
    });
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
