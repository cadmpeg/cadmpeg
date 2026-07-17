// SPDX-License-Identifier: Apache-2.0
//! Committed-parse failure types.
//!
//! Reverse-engineered decoding is speculative: try an interpretation, and a
//! miss means try the next. Leaf reads stay `Option` so the probe path builds
//! no errors and makes no allocations. The commit transition is realized by the
//! `req_*` mirror API on [`View`](super::View): a committed required read
//! returns `Result<T, ParseError>`, so once an interpretation is accepted a
//! failure is classified and cannot be turned back into "try the next
//! candidate".

use super::error::SourceLocation;

/// A classified failure after commitment, with its location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Where the failure occurred.
    pub location: SourceLocation,
    /// What kind of failure it was.
    pub kind: ParseErrorKind,
}

/// The classified reason a committed parse failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// A required read extended past the view's end.
    UnexpectedEof {
        /// How many bytes the read needed.
        needed: u64,
        /// How many bytes remained in the view.
        remaining: u64,
    },
    /// A value inside the view was inconsistent.
    InvalidValue,
    /// The framing inside the view was inconsistent.
    InvalidFraming,
}
