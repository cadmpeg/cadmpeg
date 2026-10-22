// SPDX-License-Identifier: Apache-2.0
//! Scoped part transactions without document, loss, or retained-byte policy.

use super::{DecodeContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, View};
use crate::CodecError;

/// Actual source extent and innermost record containing one attempted part.
///
/// This is framing metadata, not semantic read coverage. The IR part helper
/// owns Appendix C levels, the part tree, retained bytes and unreadable losses.
#[derive(Debug, PartialEq, Eq)]
pub struct PartCapture {
    stream: String,
    offset: u64,
    byte_len: u64,
    record_id: Option<String>,
}

impl PartCapture {
    pub(super) fn new(
        stream: String,
        offset: u64,
        byte_len: u64,
        record_id: Option<String>,
    ) -> Self {
        Self {
            stream,
            offset,
            byte_len,
            record_id,
        }
    }

    /// Stable name of the actual stream containing the part.
    pub fn stream(&self) -> &str {
        &self.stream
    }
    /// Start of the part in its stream.
    pub fn offset(&self) -> u64 {
        self.offset
    }
    /// Exact bounded length of the part.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }
    /// Innermost enclosing source record, proven by its extent or provenance.
    pub fn record_id(&self) -> Option<&str> {
        self.record_id.as_deref()
    }
    /// Consume the stream, offset, length and containing-record identity.
    pub fn into_parts(self) -> (String, u64, u64, Option<String>) {
        (self.stream, self.offset, self.byte_len, self.record_id)
    }
}

/// The attempted part's framing and decoded value or recoverable error.
///
/// This type is ephemeral. It does not retain source bytes, create a native
/// failure record, or assert that the source bytes were semantically read.
#[derive(Debug)]
pub struct PartOutcome<T> {
    capture: PartCapture,
    result: Result<T, CodecError>,
}

impl<T> PartOutcome<T> {
    /// Actual framing metadata, independent of successful semantic reads.
    pub fn capture(&self) -> &PartCapture {
        &self.capture
    }
    /// Borrow the decoded value or the error that gave up the part.
    pub fn result(&self) -> Result<&T, &CodecError> {
        self.result.as_ref()
    }
    /// Consume framing metadata and the part's result for the IR helper.
    pub fn into_parts(self) -> (PartCapture, Result<T, CodecError>) {
        (self.capture, self.result)
    }
}

impl DecodeContext<'_> {
    /// Capture a genuine source view's canonical extent without reading content.
    pub fn capture_part_extent(&self, extent: View<'_>) -> Result<PartCapture, CodecError> {
        self.require_source(extent)?;
        self.budget.part_capture(extent)
    }

    /// Run one explicitly bounded part with transactional read recovery.
    ///
    /// A failed part loses its reads and descendant captures. Only a Part
    /// refusal raised during this runner is acknowledged; charged cumulative
    /// resources are never refunded. I/O and Session resource errors escape.
    /// A refusal already pending before entry is never acknowledged here.
    pub fn part_transaction<T>(
        &self,
        extent: View<'_>,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<PartOutcome<T>, CodecError> {
        let capture = self.capture_part_extent(extent)?;
        let transaction = match self.read_transaction() {
            Ok(transaction) => transaction,
            Err(error) => return self.finish_given_up(capture, error),
        };
        let result = decode();
        if let Some(limit) = self
            .resource_refusal()
            .filter(|limit| limit.scope == LimitScope::Session)
        {
            return Err(limit.into());
        }
        if matches!(&result, Err(error) if fatal(error)) {
            drop(transaction);
            return result.map(|value| PartOutcome {
                capture,
                result: Ok(value),
            });
        }
        let result = match self.resource_refusal() {
            Some(limit) => Err(limit.into()),
            None => result,
        };
        match result {
            Ok(value) => {
                transaction.commit()?;
                Ok(PartOutcome {
                    capture,
                    result: Ok(value),
                })
            }
            Err(error) => {
                drop(transaction);
                self.finish_given_up(capture, error)
            }
        }
    }

    fn finish_given_up<T>(
        &self,
        capture: PartCapture,
        error: CodecError,
    ) -> Result<PartOutcome<T>, CodecError> {
        if fatal(&error) {
            return Err(error);
        }
        if let Some(limit) = self.resource_refusal() {
            if limit.scope == LimitScope::Session {
                return Err(limit.into());
            }
            self.budget.recover_part(limit)?;
        }
        Ok(PartOutcome {
            capture,
            result: Err(error),
        })
    }

    /// Raise a fixed or per-part allowance without turning it into a session
    /// refusal. The current part runner must acknowledge it before more work.
    pub fn refuse_part_limit(
        &self,
        dimension: ResourceDimension,
        limit: u64,
        used: u64,
        additional: u64,
        operation: &'static str,
    ) -> CodecError {
        self.budget
            .refuse_resource(ResourceLimit {
                scope: LimitScope::Part,
                dimension,
                reason: ResourceFailure::BudgetExceeded,
                limit,
                used,
                additional,
                operation,
            })
            .into()
    }
}

fn fatal(error: &CodecError) -> bool {
    matches!(
        error,
        CodecError::Io(_)
            | CodecError::ResourceLimit(ResourceLimit {
                scope: LimitScope::Session,
                ..
            })
    )
}

#[cfg(test)]
mod tests;
