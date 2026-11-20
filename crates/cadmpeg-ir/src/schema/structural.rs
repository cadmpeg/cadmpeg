// SPDX-License-Identifier: Apache-2.0
//! Structural JSON values projected under the caller's depth account.

use crate::hash::finite_json::{
    admission::CanonicalAdmission, write_serde_json, CanonicalJsonError,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::Serialize;

/// Project `value` to the JSON value `serde_json` writes for it, entering one
/// depth level of `ctx` at each serialized value.
///
/// # Errors
///
/// The resource refusal of `ctx`, or a malformed error when `value` does not
/// serialize.
pub fn project(
    ctx: &DecodeContext<'_>,
    value: &(impl Serialize + ?Sized),
    operation: &'static str,
) -> Result<serde_json::Value, CodecError> {
    let mut text = Vec::new();
    match write_serde_json(
        CanonicalAdmission::limited(ctx, operation),
        &mut text,
        value,
    ) {
        Ok(()) => {}
        Err(CanonicalJsonError::Resource(error)) => return Err(error),
        Err(error) => return Err(failed(&error)),
    }
    let text = std::str::from_utf8(&text).map_err(|error| failed(&error))?;
    crate::native::replay::value(text).map_err(|error| failed(&error))
}

fn failed(error: &dyn std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("structural projection failed: {error}"))
}

#[cfg(test)]
mod tests;
