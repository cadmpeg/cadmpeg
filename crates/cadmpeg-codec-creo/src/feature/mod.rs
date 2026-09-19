// SPDX-License-Identifier: Apache-2.0
//! Structural `AllFeatur` feature-to-generated-entity bindings.
//!
//! A mixed generated-entity table is `f8 <count> f7 <table-class> fb e3`, followed by
//! exactly `<count>` compact entity identifiers, each terminated by `e3`.
//! `f7 <entry-class>` may prefix the first entry. The table belongs to an `AllFeatur` row only
//! when its byte offset is bounded by that row's known feature-id header.

pub(crate) mod definitions;
pub(crate) mod entity;
mod helpers;
pub(crate) mod operations;
pub(crate) mod rows;
pub(crate) mod schema;
pub(crate) mod segment_rows;

#[cfg(test)]
mod tests;
