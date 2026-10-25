// SPDX-License-Identifier: Apache-2.0
//! Caller-bounded encode sessions and fallible retained output buffers.

use std::sync::Arc;

use crate::operation_budget::OperationBudget;

use crate::decode::{DecodeContext, DecodePolicy, ResourceLimit};

/// One encode operation's cumulative entities and live retained allocations.
#[derive(Debug)]
pub struct EncodeSession {
    account: Arc<OperationBudget>,
    pub(crate) policy: DecodePolicy,
}

impl EncodeSession {
    /// Wraps the actual existing caller account without opening an allowance.
    #[must_use]
    pub(crate) fn new(account: Arc<OperationBudget>, policy: DecodePolicy) -> Self {
        Self { account, policy }
    }

    /// Shares the context's caller account, including every prior fatal refusal.
    pub fn from_context(context: &DecodeContext<'_>) -> Result<Self, ResourceLimit> {
        let account = context.operation_budget();
        if let Some(error) = account.fused() {
            return Err(error);
        }
        Ok(Self::new(account, *context.policy()))
    }

    pub(crate) fn account(&self) -> Arc<OperationBudget> {
        Arc::clone(&self.account)
    }

    /// Charges semantic or native records at their actual emission boundary.
    pub fn charge_entities(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.account
            .charge_entities(self.account.limits.max_entities, count, operation)
    }

    /// Creates an empty buffer sharing this session's retained-byte allowance.
    #[must_use]
    pub const fn buffer(&self) -> EncodeBuffer<'_> {
        EncodeBuffer {
            session: self,
            bytes: Vec::new(),
            charged_capacity: 0,
        }
    }

    /// Returns a prior fused resource failure even if a caller ignored it.
    pub fn finish(&self) -> Result<(), ResourceLimit> {
        self.account.fused().map_or(Ok(()), Err)
    }
}

/// Retained serialized bytes whose allocation and lifetime belong to a session.
///
/// This type does not implement `std::io::Write`: an allocation refusal stays a
/// resource failure, rather than becoming a fabricated destination I/O error.
#[derive(Debug)]
pub struct EncodeBuffer<'a> {
    session: &'a EncodeSession,
    bytes: Vec<u8>,
    charged_capacity: usize,
}

impl EncodeBuffer<'_> {
    /// Serialized bytes currently retained by this buffer.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Number of initialized retained bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether this buffer currently retains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Appends serialized bytes after admitting and reserving backing capacity.
    ///
    /// A policy refusal before allocation preserves the buffer. If an allocator
    /// returns excess capacity beyond the caller's allowance, this buffer is
    /// released immediately and the session remains failed; no partial output
    /// from that failed session may be emitted.
    pub fn append(&mut self, bytes: &[u8], operation: &'static str) -> Result<(), ResourceLimit> {
        self.reserve_append(bytes.len(), operation)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    /// Replaces existing bytes without changing their length or resource charge.
    /// Returns false, without mutation, when the range does not exist.
    #[must_use]
    pub fn overwrite(&mut self, offset: usize, bytes: &[u8]) -> bool {
        let Some(end) = offset.checked_add(bytes.len()) else {
            return false;
        };
        let Some(target) = self.bytes.get_mut(offset..end) else {
            return false;
        };
        target.copy_from_slice(bytes);
        true
    }

    /// Moves the bytes from `start` to the end in front of the bytes before
    /// `start`, in place. The length and the resource charge do not change.
    /// Returns false, without mutation, when `start` exceeds the length.
    #[must_use]
    pub fn rotate_tail_to_front(&mut self, start: usize) -> bool {
        let Some(tail) = self.bytes.len().checked_sub(start) else {
            return false;
        };
        self.bytes.rotate_right(tail);
        true
    }

    /// Hands over the retained bytes and refunds their charge.
    ///
    /// The session no longer bounds the returned bytes. A caller uses this
    /// for the final output only, in place of a copy that the session does
    /// not charge.
    #[must_use]
    pub fn into_vec(mut self) -> Vec<u8> {
        // `Drop` refunds `charged_capacity`.
        std::mem::take(&mut self.bytes)
    }

    /// Backing bytes currently charged to the caller's retained allowance.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.charged_capacity
    }

    fn reserve_append(
        &mut self,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.session.finish()?;
        let current_capacity = self.bytes.capacity();
        let unused = current_capacity - self.bytes.len();
        let required_growth = if additional > unused {
            additional - unused
        } else {
            0
        };
        if required_growth == 0 {
            return Ok(());
        }
        let preferred = if current_capacity == 0 {
            required_growth
        } else {
            required_growth.max(current_capacity)
        };
        let preferred = if unused.checked_add(preferred).is_some() {
            preferred
        } else {
            required_growth
        };
        let growth =
            self.session
                .account
                .reserve_retained(required_growth, preferred, operation)?;
        let session = self.session;
        let mut reservation = CapacityReservation {
            account: &session.account,
            bytes: crate::decode::u64_from_index(growth),
        };
        if self.bytes.try_reserve_exact(unused + growth).is_err() {
            // The failed allocation retained no growth. Refund its provisional
            // reservation before the real caller account records live usage.
            drop(reservation);
            return Err(self
                .session
                .account
                .allocation_failed(crate::decode::u64_from_index(growth), operation));
        }
        self.admit_reserved_capacity(operation, reservation.bytes)?;
        reservation.bytes = 0;
        Ok(())
    }

    fn admit_reserved_capacity(
        &mut self,
        operation: &'static str,
        reserved: u64,
    ) -> Result<(), ResourceLimit> {
        let actual_growth = self.bytes.capacity() - self.charged_capacity;
        let actual_bytes = crate::decode::u64_from_index(actual_growth);
        debug_assert!(
            actual_bytes >= reserved,
            "Vec capacity covers the exact reservation"
        );
        if let Err(error) = self
            .session
            .account
            .charge_retained(actual_bytes - reserved, operation)
        {
            self.bytes = Vec::new();
            self.session
                .account
                .release_retained(crate::decode::u64_from_index(self.charged_capacity));
            self.charged_capacity = 0;
            return Err(error);
        }
        self.charged_capacity = self.bytes.capacity();
        Ok(())
    }
}

impl Drop for EncodeBuffer<'_> {
    fn drop(&mut self) {
        self.session
            .account
            .release_retained(crate::decode::u64_from_index(self.charged_capacity));
    }
}

struct CapacityReservation<'a> {
    account: &'a OperationBudget,
    bytes: u64,
}
impl Drop for CapacityReservation<'_> {
    fn drop(&mut self) {
        self.account.release_retained(self.bytes);
    }
}

#[cfg(test)]
mod tests;
