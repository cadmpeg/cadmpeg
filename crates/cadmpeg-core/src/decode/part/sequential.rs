// SPDX-License-Identifier: Apache-2.0
//! Select one authentic capture after the actual bounded parser attempt.

use super::{fatal, DecodeContext, LimitScope, PartOutcome, View};
use crate::CodecError;

impl DecodeContext<'_> {
    /// Run a bounded part whose recoverable failure gives up a larger suffix.
    ///
    /// Both views must belong to this session and the same source space, start
    /// at the same position, and contain identical original bytes over the row.
    /// Only the selected result extent is captured and charged: `extent` on
    /// success, `failure_extent` on recoverable failure. The caller owns the
    /// format's policy for selecting a parent suffix. No parser is replayed.
    /// Session and I/O failures escape; a previously pending refusal is never
    /// acknowledged. Reads and record attempts use the same part transaction.
    pub fn part_transaction_with_failure_extent<T>(
        &self,
        extent: View<'_>,
        failure_extent: View<'_>,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<PartOutcome<T>, CodecError> {
        self.validate_failure_extent(extent, failure_extent)?;
        self.selected_part_transaction(
            failure_extent,
            false,
            || decode().map(|value| (value, extent)),
            |_, _| Ok(()),
        )
    }

    fn validate_failure_extent(
        &self,
        extent: View<'_>,
        failure_extent: View<'_>,
    ) -> Result<(), CodecError> {
        self.require_source(extent)?;
        self.require_source(failure_extent)?;
        if extent.location().space != failure_extent.location().space
            || extent.start() != failure_extent.start()
            || extent.end() > failure_extent.end()
            || !failure_extent
                .window()
                .get(..extent.window().len())
                .is_some_and(|prefix| prefix.as_ptr() == extent.window().as_ptr())
        {
            return Err(CodecError::malformed(
                "part failure extent is not its original source suffix",
            ));
        }
        Ok(())
    }

    /// Parse a sequential row whose successful end is discovered by its grammar.
    ///
    /// `prefix` is the genuine known beginning, and `failure_extent` is the
    /// authenticated parent remainder. The callback runs once and returns its
    /// actual consumed original view. Only a positive original prefix covering
    /// the known beginning can commit. Observed View reads, captured primary
    /// record extents and all credited reads must fit that prefix or its registered
    /// ancestor projections; extra derived
    /// source streams are refused. Failure captures the parent remainder once.
    pub fn part_transaction_discovered<'source, T>(
        &self,
        prefix: View<'source>,
        failure_extent: View<'source>,
        decode: impl FnOnce() -> Result<(T, View<'source>), CodecError>,
    ) -> Result<PartOutcome<T>, CodecError> {
        self.validate_failure_extent(prefix, failure_extent)?;
        self.selected_part_transaction(failure_extent, true, decode, |transaction, consumed| {
            self.validate_failure_extent(consumed, failure_extent)?;
            if consumed.start() == consumed.end() || consumed.end() < prefix.end() {
                return Err(CodecError::malformed(
                    "discovered row has no positive consumed prefix",
                ));
            }
            transaction.validate_part_prefix(consumed)
        })
    }

    fn selected_part_transaction<'source, T>(
        &self,
        failure_extent: View<'source>,
        observe_boundary: bool,
        decode: impl FnOnce() -> Result<(T, View<'source>), CodecError>,
        validate: impl FnOnce(
            &super::super::tracking::ReadTransaction<'_>,
            View<'source>,
        ) -> Result<(), CodecError>,
    ) -> Result<PartOutcome<T>, CodecError> {
        if let Some(limit) = self.resource_refusal() {
            return Err(limit.into());
        }
        let mut transaction = match super::super::tracking::ReadTransaction::begin_part(self.budget)
        {
            Ok(transaction) => transaction,
            Err(error) => return self.capture_given_up(failure_extent, Vec::new(), error),
        };
        if observe_boundary {
            transaction.observe_part_boundary()?;
        }
        let result = decode().and_then(|(value, consumed)| {
            validate(&transaction, consumed)?;
            Ok((value, consumed))
        });
        if let Some(limit) = self
            .resource_refusal()
            .filter(|limit| limit.scope == LimitScope::Session)
        {
            return Err(limit.into());
        }
        let result = match result {
            Err(error) if fatal(&error) => return Err(error),
            result => result,
        };
        let result = match self.resource_refusal() {
            Some(limit) => Err(limit.into()),
            None => result,
        };
        match result {
            Ok((value, consumed)) => {
                let capture = self.capture_part_extent(consumed)?;
                transaction.commit()?;
                Ok(PartOutcome {
                    capture,
                    result: Ok(value),
                })
            }
            Err(error) => {
                let attempted = transaction.take_part_attempts()?;
                drop(transaction);
                self.capture_given_up(failure_extent, attempted, error)
            }
        }
    }

    fn capture_given_up<T>(
        &self,
        extent: View<'_>,
        attempted: Vec<String>,
        error: CodecError,
    ) -> Result<PartOutcome<T>, CodecError> {
        if fatal(&error) {
            return Err(error);
        }
        // A newly raised Part fuse must be acknowledged before allocating the
        // single selected capture. Cumulative charges remain untouched.
        if let Some(limit) = self.resource_refusal() {
            if limit.scope == LimitScope::Session {
                return Err(limit.into());
            }
            self.budget.recover_part(limit)?;
        }
        let mut capture = self.capture_part_extent(extent)?;
        capture.attempted_record_ids = attempted;
        self.finish_given_up(capture, error)
    }
}

#[cfg(test)]
mod tests;
