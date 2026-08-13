// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads. They
//! construct raw bytes only; no native record type crosses in here.
#![allow(clippy::unwrap_used)]

mod bytes;
mod deltas;
mod om;
mod prt;
mod streams;

pub(crate) use bytes::*;
pub(crate) use deltas::*;
pub(crate) use om::*;
pub(crate) use prt::*;
pub(crate) use streams::*;
