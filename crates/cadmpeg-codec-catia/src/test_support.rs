// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic CATPart byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.CATPart` byte images and embedded-stream payloads.
//! They construct raw bytes only; decode, native, and family tests own the
//! assertions.
#![allow(clippy::doc_markdown, clippy::unwrap_used)]

mod test_annotations;
mod test_bytes;
mod test_container;
mod test_topology;

pub(crate) use crate::container::{DIR_MAGIC, OUTER_MAGIC};
pub(crate) use test_annotations::*;
pub(crate) use test_bytes::*;
pub(crate) use test_container::*;
pub(crate) use test_topology::*;
