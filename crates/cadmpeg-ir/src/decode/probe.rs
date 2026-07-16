// SPDX-License-Identifier: Apache-2.0
//! Probe and commit result types.
//!
//! Reverse-engineered decoding is speculative: try an interpretation, and a
//! miss means try the next. Leaf reads stay `Option` so the probe path builds
//! no errors and makes no allocations. The commit transition is an explicit
//! type so it cannot decay into a convention: once an interpretation is
//! accepted, a failure is classified and cannot be turned back into "try the
//! next candidate".

use super::error::SourceLocation;

/// The outcome of attempting one interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe<T> {
    /// The interpretation did not apply; try the next one.
    NoMatch,
    /// The interpretation applied and produced a value.
    Match(T),
    /// The interpretation was accepted, then failed: classified, not retried.
    CommittedError(ParseError),
}

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
