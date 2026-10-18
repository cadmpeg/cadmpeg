// SPDX-License-Identifier: Apache-2.0
//! Session-backed entry points for offset-surface tests.

use super::{
    certified_offset_cache_fit_with_budget,
    continue_surface_intersection_parameters_with_index_and_seeds_and_budget_and_grid_cache,
    offset_surface_parameters_with_tolerance_with_index_and_budget,
};
use crate::decode::geometry_work::{GeometryWorkBudget, MAX_ADAPTIVE_GEOMETRY_WORK};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::NonNegativeLength;
use std::collections::BTreeMap;

pub(in crate::decode) fn certified_offset_cache_fit(
    ctx: &DecodeContext<'_>,
    support: &SurfaceGeometry,
    candidate: &SurfaceGeometry,
    distance: f64,
    tolerance: NonNegativeLength,
) -> Option<f64> {
    let geometry_budget = GeometryWorkBudget::from_context(ctx, MAX_ADAPTIVE_GEOMETRY_WORK);
    certified_offset_cache_fit_with_budget(
        support,
        candidate,
        distance,
        tolerance,
        &geometry_budget,
    )
    .expect("evaluation stays within session limits")
}

pub(in crate::decode) fn offset_surface_parameters(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    offset_surface_parameters_with_tolerance(ctx, ir, surface, point, seed, None)
}

pub(in crate::decode) fn offset_surface_parameters_with_tolerance(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
) -> Option<Point2> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let geometry_budget = GeometryWorkBudget::from_context(ctx, MAX_ADAPTIVE_GEOMETRY_WORK);
    offset_surface_parameters_with_tolerance_with_index_and_budget(
        &index,
        surface,
        point,
        seed,
        fit_tolerance,
        &geometry_budget,
    )
    .expect("evaluation stays within session limits")
}

pub(in crate::decode) fn continue_surface_intersection_parameters(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    chart: &[Point3],
    fit_tolerance: f64,
) -> Option<[Vec<Point2>; 2]> {
    let geometry_budget = GeometryWorkBudget::from_context(ctx, MAX_ADAPTIVE_GEOMETRY_WORK);
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let mut blend_parameter_grids = BTreeMap::new();
    continue_surface_intersection_parameters_with_index_and_seeds_and_budget_and_grid_cache(
        &index,
        surfaces,
        chart,
        fit_tolerance,
        [None, None],
        &geometry_budget,
        &mut blend_parameter_grids,
    )
    .expect("evaluation stays within session limits")
}
