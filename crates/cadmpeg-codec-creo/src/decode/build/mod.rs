// SPDX-License-Identifier: Apache-2.0
//! IR assembly, native arena emission, coverage, and decode report.

mod arenas;
mod coverage;
mod ir;
mod ir_features;
mod ir_geometry;
mod meta;
mod passthrough;
mod report;
mod report_coverage;
mod report_losses;

pub(super) use ir::{build_container_ir, build_ir, BuiltIr};
pub(super) use report::build_report;
// Unused in the lib unit; sibling test modules reach it through this barrel.
#[allow(unused_imports)]
pub(super) use report::has_transferred_geometry;
