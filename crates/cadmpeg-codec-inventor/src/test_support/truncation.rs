// SPDX-License-Identifier: Apache-2.0
//! Truncation diagnostics for crate tests, read without an unwrap on the route.
//!
//! The two forms differ in what they state. [`displayed_truncation`] returns
//! the error's own `Display` text, which names the space as well as the field
//! and the offset. [`located_truncation`] returns the variant, the field and
//! the offset, so an assertion states the truncation without the space.

use cadmpeg_core::CodecError;

/// The diagnostic a truncated read produces, without an unwrap on the route.
pub(crate) fn displayed_truncation<T: std::fmt::Debug>(result: Result<T, CodecError>) -> String {
    match result {
        Ok(value) => format!("the read succeeded with {value:?}"),
        Err(error) => error.to_string(),
    }
}

/// The truncation a read reports, as its variant, field and offset,
/// without an unwrap on the route.
pub(crate) fn located_truncation<T: std::fmt::Debug>(result: Result<T, CodecError>) -> String {
    match result {
        Ok(value) => format!("the read succeeded with {value:?}"),
        Err(CodecError::Truncated {
            location,
            operation,
        }) => format!("Truncated {operation} at offset {}", location.offset),
        Err(error) => error.to_string(),
    }
}
