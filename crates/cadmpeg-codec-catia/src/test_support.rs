// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic CATPart byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.CATPart` byte images and embedded-stream payloads.
//! They construct raw bytes only; decode, native, and family tests own the
//! assertions.
#![allow(clippy::doc_markdown, clippy::unwrap_used)]

mod test_a5_bound;
mod test_a5a8;
mod test_annotations;
mod test_b2;
mod test_b5;
mod test_bytes;
mod test_container;
mod test_e5;
mod test_topology;
mod test_zero_entity;

pub(crate) use crate::container::OUTER_MAGIC;
pub(crate) use test_a5_bound::*;
pub(crate) use test_a5a8::*;
pub(crate) use test_annotations::*;
pub(crate) use test_b2::*;
pub(crate) use test_b5::*;
pub(crate) use test_bytes::*;
pub(crate) use test_container::*;
pub(crate) use test_e5::*;
pub(crate) use test_topology::*;
pub(crate) use test_zero_entity::*;
