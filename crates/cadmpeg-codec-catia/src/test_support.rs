// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic CATPart byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.CATPart` byte images and embedded-stream payloads.
//! They construct raw bytes only; decode, native, and family tests own the
//! assertions.
#![allow(clippy::doc_markdown, clippy::unwrap_used)]

pub(crate) mod test_a5_bound;
pub(crate) mod test_a5a8;
pub(crate) mod test_annotations;
pub(crate) mod test_b2;
pub(crate) mod test_b5;
pub(crate) mod test_bytes;
pub(crate) mod test_container;
pub(crate) mod test_e5;
pub(crate) mod test_formula;
pub(crate) mod test_object_graph;
pub(crate) mod test_topology;
pub(crate) mod test_zero_entity;
