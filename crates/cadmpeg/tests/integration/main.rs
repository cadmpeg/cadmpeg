// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the cadmpeg CLI.
#![allow(clippy::unwrap_used)]

mod binary_support;
mod codec_conformance;
mod convert;
mod corpus_manifest;
mod diff;
mod inspect;
mod inspect_bytes;
mod low_confidence_inspect;
mod measure;
mod query;
mod query_fidelity;
mod query_graph;
mod query_join;
mod query_schema_relations;
mod query_support;
mod refusals;
#[cfg(feature = "rhino")]
mod registry;
mod reports;
mod resource_refusals;
mod support;
