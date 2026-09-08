// SPDX-License-Identifier: Apache-2.0
//! IR assembly, native arena emission, coverage, and decode report.

mod arenas;
mod coverage;
pub(super) mod ir;
mod ir_features;
mod ir_geometry;
mod meta;
mod passthrough;
pub(super) mod report;
mod report_coverage;
mod report_losses;
mod units;
