// SPDX-License-Identifier: Apache-2.0
//! Part-scoped recovery for parts a format frames by itself.

use super::{DecodeContext, LimitScope, ResourceLimit};
use crate::CodecError;

impl DecodeContext<'_> {
    /// Runs the decode of one part.
    ///
    /// The outer error is fatal to the session: a refusal already in force
    /// at entry, a Session-scoped refusal, or an I/O error. The inner result
    /// is the part's own outcome. A Part-scoped refusal raised inside
    /// `decode` is cleared and returned as the part's error, even when
    /// `decode` swallowed it. Charges made inside the part are not refunded.
    ///
    /// # Errors
    ///
    /// The fatal errors above.
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
