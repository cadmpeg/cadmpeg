// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic CATPart byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.CATPart` byte images and embedded-stream payloads.
//! They construct raw bytes only; decode, native, and family tests own the
//! assertions.
#![allow(clippy::doc_markdown, clippy::unwrap_used)]

mod bytes;

pub(crate) use bytes::*;
