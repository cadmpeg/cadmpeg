// SPDX-License-Identifier: Apache-2.0
//! Caller-bounded encode sessions and fallible retained output buffers.

use std::cell::Cell;

use crate::decode::{
    LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, ResourceLimits,
};

/// One encode operation's cumulative entities and live retained allocations.
#[derive(Debug)]
pub struct EncodeSession {
    limits: ResourceLimits,
    entities: Cell<u64>,
    retained: Cell<u64>,
    failure: Cell<Option<ResourceLimit>>,
}

impl EncodeSession {
    /// Starts an encode operation with the caller's ceilings.
    #[must_use]
    pub const fn new(limits: ResourceLimits) -> Self {
        Self {
            limits,
            entities: Cell::new(0),
            retained: Cell::new(0),
            failure: Cell::new(None),
        }
    }

    /// Charges semantic or native records at their actual emission boundary.
    pub fn charge_entities(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.admit(
            ResourceDimension::Entities,
            self.limits.max_entities,
            self.entities.get(),
            count,
            operation,
        )?;
        self.entities.set(self.entities.get() + count);
        Ok(())
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
        self.failure.get().map_or(Ok(()), Err)
    }

    fn admit(
        &self,
        dimension: ResourceDimension,
        limit: u64,
        used: u64,
        additional: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        if let Some(error) = self.failure.get() {
            return Err(error);
        }
        if !limit
            .checked_sub(used)
            .is_some_and(|remaining| additional <= remaining)
        {
            return Err(self.fail(ResourceLimit {
                scope: LimitScope::Session,
                dimension,
                reason: ResourceFailure::BudgetExceeded,
                limit,
                used,
                additional,
                operation,
            }));
        }
        Ok(())
    }

    fn fail(&self, error: ResourceLimit) -> ResourceLimit {
        match self.failure.get() {
            Some(first) => first,
            None => {
                self.failure.set(Some(error));
                error
            }
        }
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
        self.session.admit(
            ResourceDimension::RetainedBytes,
            self.session.limits.max_retained_bytes,
            self.session.retained.get(),
            crate::decode::u64_from_index(required_growth),
            operation,
        )?;
        if required_growth == 0 {
            return Ok(());
        }
        let preferred_growth = if current_capacity == 0 {
            required_growth
        } else {
            required_growth.max(current_capacity)
        };
        let available = self.session.limits.max_retained_bytes - self.session.retained.get();
        let growth = match usize::try_from(available) {
            Ok(remaining) => preferred_growth.min(remaining),
            Err(_) => preferred_growth,
        };
        let (reserve_additional, requested_growth) = match unused.checked_add(growth) {
            Some(amount) => (amount, growth),
            None => (additional, required_growth),
        };
        self.bytes
            .try_reserve_exact(reserve_additional)
            .map_err(|_| {
                self.session.fail(ResourceLimit {
                    scope: LimitScope::Session,
                    dimension: ResourceDimension::RetainedBytes,
                    reason: ResourceFailure::AllocationFailed,
                    limit: self.session.limits.max_retained_bytes,
                    used: self.session.retained.get(),
                    additional: crate::decode::u64_from_index(requested_growth),
                    operation,
                })
            })?;
        self.admit_reserved_capacity(operation)
    }

    fn admit_reserved_capacity(&mut self, operation: &'static str) -> Result<(), ResourceLimit> {
        let actual_growth = self.bytes.capacity() - self.charged_capacity;
        let actual_bytes = crate::decode::u64_from_index(actual_growth);
        if let Err(error) = self.session.admit(
            ResourceDimension::RetainedBytes,
            self.session.limits.max_retained_bytes,
            self.session.retained.get(),
            actual_bytes,
            operation,
        ) {
            // The allocator may return more capacity than requested. Release
            // both the previous charge and the excess allocation immediately:
            // this failed, fused session cannot retain uncharged backing bytes.
            self.bytes = Vec::new();
            self.session.retained.set(
                self.session.retained.get() - crate::decode::u64_from_index(self.charged_capacity),
            );
            self.charged_capacity = 0;
            return Err(error);
        }
        self.session
            .retained
            .set(self.session.retained.get() + actual_bytes);
        self.charged_capacity = self.bytes.capacity();
        Ok(())
    }
}

impl Drop for EncodeBuffer<'_> {
    fn drop(&mut self) {
        self.session.retained.set(
            self.session.retained.get() - crate::decode::u64_from_index(self.charged_capacity),
        );
    }
}

#[cfg(test)]
mod tests;
