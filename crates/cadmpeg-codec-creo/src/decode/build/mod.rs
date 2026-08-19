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
mod units;

pub(super) use ir::{build_container_ir, build_ir, BuiltIr};
pub(super) use report::build_report;
#[allow(unused_imports)]
pub(super) use report::has_transferred_geometry;

// Remaining `pub(super)` names stay on the build barrel for sibling and test use.
#[allow(unused_imports)]
pub(super) use arenas::{emit_geometry_arenas, emit_reference_arenas};
#[allow(unused_imports)]
pub(super) use coverage::{
    collect_feature_coverage, legacy_numeric_coverage, record_coverage, torus_parameter_coverage,
    TorusParameterCoverage,
};
#[allow(unused_imports)]
pub(super) use ir::{
    body_selection_has_unresolved_operands, edge_selection_has_unresolved_operands,
    face_selection_has_unresolved_operands, path_has_unresolved_operands,
    pattern_kind_has_unresolved_operands, surface_boundary_has_unresolved_operands,
    termination_has_unresolved_operands,
};
#[allow(unused_imports)]
pub(super) use meta::source_meta;
#[allow(unused_imports)]
pub(super) use passthrough::{
    emit_legacy_arenas, emit_legacy_value_arena, legacy_source_stream,
    preserve_passthrough_sections,
};
