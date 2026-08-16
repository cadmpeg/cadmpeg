// SPDX-License-Identifier: Apache-2.0
//! Unit and fixture tests for OM wire parsers owned by `om`.

#![allow(clippy::unwrap_used)]

pub(crate) use super::*;

mod index_and_lanes;
mod instances_and_stores;
mod operation_data_block_references;
mod pattern_lanes;
mod sketch_payload;
mod tagged_references;
