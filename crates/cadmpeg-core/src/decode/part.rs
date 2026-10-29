// SPDX-License-Identifier: Apache-2.0
//! Part-scoped resource recovery for independently framed decode parts.

use super::{DecodeContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
use crate::CodecError;

impl DecodeContext<'_> {
    /// Run one independently framed part.
    ///
    /// The outer error is fatal to the session: a refusal pending before
    /// entry, a Session-scoped refusal, or an I/O error. The inner result is
    /// the part's own outcome. A Part-scoped refusal raised inside `decode` is
    /// acknowledged and returned as the part's error, even when `decode`
    /// swallowed it. Cumulative charges are never refunded.
    pub fn run_part<T>(
        &self,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<Result<T, CodecError>, CodecError> {
        if let Some(limit) = self.resource_refusal() {
            return Err(limit.into());
        }
        let result = decode();
        if let Some(limit) = self.resource_refusal() {
            if limit.scope == LimitScope::Session {
                return Err(limit.into());
            }
            self.budget.recover_part(limit)?;
            return Ok(Err(limit.into()));
        }
        match result {
            Err(error) if fatal(&error) => Err(error),
            result => Ok(result),
        }
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
