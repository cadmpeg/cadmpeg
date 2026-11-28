// SPDX-License-Identifier: Apache-2.0
//! IR assembly, native arena emission, coverage, and decode report.

mod arenas;
mod coverage;
mod design_v11;
pub(super) mod ir;
pub(super) mod ir_features;
mod ir_geometry;
mod meta;
mod passthrough;
pub(super) mod report;
pub(super) mod report_coverage;
mod report_losses;
pub(super) mod units;
