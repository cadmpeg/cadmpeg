// SPDX-License-Identifier: Apache-2.0
//! Session-backed entry points for p-curve tests.

use super::{
    exact_analytic_isocurve_pcurve_with_index_and_budget,
    pcurve_matches_edge_range_with_index_and_budget,
};
use crate::decode::geometry_work::{GeometryWorkBudget, MAX_ADAPTIVE_GEOMETRY_WORK};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::ids::{CurveId, EdgeId, SurfaceId};

pub(in crate::decode) fn exact_analytic_isocurve_pcurve(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    curve: &CurveId,
    surface: &SurfaceId,
    range: [f64; 2],
    tolerance: f64,
) -> Option<PcurveGeometry> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let geometry_budget = GeometryWorkBudget::from_context(ctx, MAX_ADAPTIVE_GEOMETRY_WORK);
    exact_analytic_isocurve_pcurve_with_index_and_budget(
        &index,
        curve,
        surface,
        range,
        tolerance,
        &geometry_budget,
    )
    .expect("evaluation stays within session limits")
}

pub(crate) fn pcurve_matches_edge(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    edge_id: &EdgeId,
    surface_id: &SurfaceId,
    geometry: &PcurveGeometry,
    fit_tolerance: Option<f64>,
) -> bool {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let geometry_budget = GeometryWorkBudget::from_context(ctx, MAX_ADAPTIVE_GEOMETRY_WORK);
    pcurve_matches_edge_range_with_index_and_budget(
        &index,
        edge_id,
        surface_id,
        geometry,
        None,
        fit_tolerance,
        &geometry_budget,
    )
    .expect("evaluation stays within session limits")
}
