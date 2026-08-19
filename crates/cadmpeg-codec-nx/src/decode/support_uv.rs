// SPDX-License-Identifier: Apache-2.0
//! EXT11 support-UV assignment, completion, and equivalent-parameter transfer.

use super::blend::{
    blend_boundary_parameter_from_contact_pcurve_with_geometry_and_budget,
    blend_support_parameter_from_source_pcurve_with_index_and_budget_and_seed_cache,
    blend_surface_definition_with_index, blend_surface_parameter_grid_with_index_and_budget,
    blend_surface_parameters_for_fit_with_grid_and_budget,
    blend_surface_parameters_for_fit_with_source_continuation_and_budget,
    blend_surface_parameters_from_grid_for_fit_and_budget,
    blend_surface_parameters_from_grid_for_fit_with_source_continuation_and_budget,
    blend_surface_parameters_from_point_with_index_and_budget,
    decoded_surface_point_with_geometry_and_budget, spine_contact_pcurve_with_index,
    BlendContactSeedCache, BlendParameterGrid, BoundaryInverseTarget,
};
use super::geometry_work::GeometryWorkBudget;
use super::offset::{
    continue_surface_intersection_parameters_with_index_and_seeds_and_budget_and_grid_cache,
    offset_surface_parameters_with_tolerance_with_index_and_budget, point_distance,
    refine_offset_surface_parameters_with_index_and_budget, surface_parameters,
};
use super::pcurves::{
    blend_boundary_parameter_from_support_spine_with_index_and_budget,
    linear_nurbs_curve_endpoint_witness_with_index, pcurve_edge_endpoint_contract_with_index,
    pcurve_matches_edge_endpoint_contract, pcurve_surface_endpoints_with_index_and_budget,
    surface_parameters_for_fit_with_index_and_budget,
};
use super::MISSING_TOLERANCE;
use crate::topology::Graph;
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    analytic_surface_parameters, nurbs_surface_parameter_within_tolerance_with_budget, pcurve_uv,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, Pcurve, PcurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, PcurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::AnnotationBuilder;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum serialized support-UV lane length admitted by one model record.
pub(super) const MAX_SUPPORT_UV_SAMPLES: usize = 1_024;

/// Minimum support-UV point fits admitted while completing one model.
const MIN_SUPPORT_UV_COMPLETION_SAMPLES: usize = 1_024;

/// Support-UV fits reserved for each admitted chart candidate before the
/// model-wide ceiling applies.
const SUPPORT_UV_COMPLETION_SAMPLES_PER_CHART: usize = 8;

/// Hard support-UV completion ceiling for one model and one completion
/// strategy. The direct and coupled strategies receive independent slices so
/// a difficult continuation pass cannot starve direct surface inversion.
///
/// Completion work scales with the chart census so a valid model is not
/// truncated at an arbitrary candidate prefix. The ceiling remains in place
/// for unusually large or adversarial inputs.
pub(super) const MAX_SUPPORT_UV_COMPLETION_SAMPLES: usize = 65_536;

/// Geometry work reserved for one support-UV sample before the lane slice is
/// capped. A lane is admitted as a whole, so one difficult carrier cannot
/// consume the model-wide budget before later carriers get a certified try.
const SUPPORT_UV_GEOMETRY_WORK_PER_SAMPLE: usize = 256;

/// Minimum geometry work available to a support-UV lane with a small chart.
const MIN_SUPPORT_UV_LANE_GEOMETRY_WORK: usize = 16_384;

/// Maximum geometry work available to one support-UV lane in one strategy.
const MAX_SUPPORT_UV_LANE_GEOMETRY_WORK: usize = 262_144;

pub(super) fn support_uv_completion_budget_limit(chart_count: usize) -> usize {
    chart_count
        .saturating_mul(SUPPORT_UV_COMPLETION_SAMPLES_PER_CHART)
        .clamp(
            MIN_SUPPORT_UV_COMPLETION_SAMPLES,
            MAX_SUPPORT_UV_COMPLETION_SAMPLES,
        )
}

pub(super) type SupportUvBudget<'a> = WorkBudget<'a>;

pub(super) fn support_uv_budget_exhausted(budget: &SupportUvBudget<'_>) -> bool {
    budget.exhausted() || budget.remaining() == 0
}

fn support_uv_lane_geometry_work_limit(sample_count: usize, remaining: usize) -> usize {
    sample_count
        .saturating_mul(SUPPORT_UV_GEOMETRY_WORK_PER_SAMPLE)
        .clamp(
            MIN_SUPPORT_UV_LANE_GEOMETRY_WORK,
            MAX_SUPPORT_UV_LANE_GEOMETRY_WORK,
        )
        .min(remaining)
}

#[cfg(test)]
pub(super) fn new_support_uv_budget() -> SupportUvBudget<'static> {
    WorkBudget::new(MAX_SUPPORT_UV_SAMPLES)
}

pub(crate) fn linear_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.push(parameters[0]);
    knots.extend_from_slice(parameters);
    knots.push(*parameters.last().expect("non-empty chart parameters"));
    knots
}

// Keep the object-map, serialized lanes, and shared geometry budget explicit:
// this function decides which native lane can be admitted to which support.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assign_ext11_support_uv_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    supports: [u32; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let surface_ids = supports.map(|support| surfaces_by_xmt.get(&support).cloned());
    let [Some(first_surface), Some(second_surface)] = surface_ids else {
        return None;
    };
    assign_ext11_support_uv_to_surfaces_with_index(
        index,
        [&first_surface, &second_surface],
        points,
        fit_tolerance,
        lanes,
        geometry_budget,
    )
}

// Keep the object-map, serialized lanes, and shared geometry budget explicit:
// validation must preserve the same support identity proof as assignment.
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_serialized_support_uv_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces_by_xmt: &BTreeMap<u32, SurfaceId>,
    supports: [u32; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> [Option<Vec<[f64; 2]>>; 2] {
    std::array::from_fn(|side| {
        let surface = surfaces_by_xmt.get(&supports[side])?;
        let values = lanes[side].as_deref()?;
        let tolerance = blend_spine_cache_fit_tolerance_with_index(index, surface, fit_tolerance);
        support_uv_lane_matches_surface_with_budget(
            index,
            surface,
            points,
            tolerance,
            Some(values),
            geometry_budget,
        )
        .then(|| values.to_vec())
    })
}

pub(crate) fn support_uv_lane_matches_surface_with_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    points: &[Point3],
    fit_tolerance: f64,
    values: Option<&[[f64; 2]]>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> bool {
    let Some(values) = values.filter(|values| values.len() == points.len()) else {
        return false;
    };
    if values.len() > MAX_SUPPORT_UV_SAMPLES {
        return false;
    }
    let Some(geometry) = index
        .surfaces(surface.0.as_str())
        .map(|surface| &surface.geometry)
    else {
        return false;
    };
    for (uv, point) in values.iter().zip(points) {
        if geometry_budget.exhausted() {
            return false;
        }
        if uv
            .iter()
            .any(|value| !value.is_finite() || missing_support_parameter(*value))
        {
            return false;
        }
        let Some(uv) = surface_parameters(geometry, *uv) else {
            return false;
        };
        let Some(candidate) = decoded_surface_point_with_geometry_and_budget(
            index,
            surface,
            geometry,
            uv.u,
            uv.v,
            0,
            geometry_budget,
        ) else {
            return false;
        };
        if point_distance(candidate, *point) > fit_tolerance {
            return false;
        }
    }
    true
}

#[cfg(test)]
pub(super) fn assign_ext11_support_uv_to_surfaces(
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let geometry_budget = GeometryWorkBudget::new(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    assign_ext11_support_uv_to_surfaces_with_index(
        &index,
        surfaces,
        points,
        fit_tolerance,
        lanes,
        &geometry_budget,
    )
}

pub(crate) fn assign_ext11_support_uv_to_surfaces_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<[Option<Vec<[f64; 2]>>; 2]> {
    let lane_matches_surface = |surface: &SurfaceId, lane: usize| {
        support_uv_lane_matches_surface_with_budget(
            index,
            surface,
            points,
            fit_tolerance,
            lanes[lane].as_deref(),
            geometry_budget,
        )
    };
    let matches = [
        [
            lane_matches_surface(surfaces[0], 0),
            lane_matches_surface(surfaces[0], 1),
        ],
        [
            lane_matches_surface(surfaces[1], 0),
            lane_matches_surface(surfaces[1], 1),
        ],
    ];
    let mut assigned = [None, None];
    let mut assigned_lanes = [None, None];
    for lane in 0..2 {
        let support_matches = [matches[0][lane], matches[1][lane]];
        let Some(support) = support_matches
            .iter()
            .position(|matches| *matches)
            .filter(|_| support_matches.iter().filter(|matches| **matches).count() == 1)
        else {
            continue;
        };
        if assigned[support].is_some() {
            return None;
        }
        assigned[support].clone_from(&lanes[lane]);
        assigned_lanes[support] = Some(lane);
    }
    if surfaces[0] != surfaces[1] && assigned.iter().filter(|lane| lane.is_some()).count() == 1 {
        let assigned_support = assigned.iter().position(Option::is_some)?;
        let assigned_lane = assigned_lanes[assigned_support]?;
        let other_support = 1 - assigned_support;
        let other_lane = 1 - assigned_lane;
        if lane_matches_surface(surfaces[other_support], other_lane) {
            assigned[other_support].clone_from(&lanes[other_lane]);
        }
    }
    assigned.iter().any(Option::is_some).then_some(assigned)
}

pub(crate) type PendingExt11SupportUv = (
    ProceduralCurveId,
    Vec<Point3>,
    Vec<f64>,
    f64,
    [Option<Vec<[f64; 2]>>; 2],
);

pub(crate) fn missing_support_parameter(value: f64) -> bool {
    value.to_bits() == MISSING_TOLERANCE.to_bits()
}

pub(crate) fn pcurve_requires_completion(pcurve: Option<&PcurveGeometry>) -> bool {
    match pcurve {
        None => true,
        Some(PcurveGeometry::Nurbs { control_points, .. }) => control_points.iter().any(|point| {
            !point.u.is_finite()
                || !point.v.is_finite()
                || missing_support_parameter(point.u)
                || missing_support_parameter(point.v)
        }),
        Some(PcurveGeometry::Line { origin, direction }) => [origin, direction]
            .into_iter()
            .any(|point| !point.u.is_finite() || !point.v.is_finite()),
        Some(_) => false,
    }
}

pub(crate) fn pcurve_control_point_seed(
    pcurve: Option<&PcurveGeometry>,
    index: usize,
) -> Option<Point2> {
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve? else {
        return None;
    };
    control_points.get(index).copied().filter(|point| {
        point.u.is_finite()
            && point.v.is_finite()
            && !missing_support_parameter(point.u)
            && !missing_support_parameter(point.v)
    })
}

fn serialized_support_uv_seed_candidates(
    geometry: &SurfaceGeometry,
    lanes: &[Option<Vec<[f64; 2]>>; 2],
    point_index: usize,
) -> [Option<Point2>; 2] {
    std::array::from_fn(|lane| {
        let [u, v] = *lanes[lane].as_deref()?.get(point_index)?;
        (u.is_finite()
            && v.is_finite()
            && !missing_support_parameter(u)
            && !missing_support_parameter(v))
        .then(|| surface_parameters(geometry, [u, v]))?
    })
}

#[cfg(test)]
pub(crate) fn complete_ext11_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    let geometry_budget = GeometryWorkBudget::new(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    complete_ext11_support_uv_with_budget(ir, pending, &geometry_budget);
}

pub(crate) fn complete_ext11_support_uv_with_budget(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    geometry_budget: &GeometryWorkBudget<'_>,
) {
    let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let mut replacements = Vec::new();
    for (procedural_id, points, parameters, fit_tolerance, lanes) in pending {
        let Some(procedural) = model_index.procedural_curves(procedural_id.0.as_str()) else {
            continue;
        };
        let (surfaces, missing) = match &procedural.definition {
            ProceduralCurveDefinition::Intersection { context, .. } => {
                let [Some(first), Some(second)] = &context.sides.clone().map(|side| side.surface)
                else {
                    continue;
                };
                (
                    [first.clone(), second.clone()],
                    context
                        .sides
                        .each_ref()
                        .map(|side| pcurve_requires_completion(side.pcurve.as_ref())),
                )
            }
            _ => continue,
        };
        if !missing.into_iter().any(|missing| missing) {
            continue;
        }
        let Some(assigned) = assign_ext11_support_uv_to_surfaces_with_index(
            &model_index,
            [&surfaces[0], &surfaces[1]],
            points,
            *fit_tolerance,
            lanes,
            geometry_budget,
        ) else {
            continue;
        };
        let side_replacements: [Option<PcurveGeometry>; 2] = std::array::from_fn(|side| {
            if !missing[side] {
                return None;
            }
            let surface_geometry = model_index
                .surfaces(surfaces[side].0.as_str())
                .map(|surface| &surface.geometry)?;
            let values = assigned[side].as_ref()?;
            if values
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || missing_support_parameter(*value))
            {
                return None;
            }
            let control_points = values
                .iter()
                .map(|uv| surface_parameters(surface_geometry, *uv))
                .collect::<Option<Vec<_>>>()?;
            Some(PcurveGeometry::Nurbs {
                degree: 1,
                knots: linear_knots(parameters),
                control_points,
                weights: None,
                periodic: false,
            })
        });
        for (side, replacement) in side_replacements.into_iter().enumerate() {
            if let Some(replacement) = replacement {
                replacements.push((procedural_id.clone(), side, replacement));
            }
        }
    }
    drop(model_index);
    for (procedural_id, side, replacement) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            unreachable!("definition checked above");
        };
        context.sides[side].pcurve = Some(replacement);
    }
}

#[cfg(test)]
pub(super) fn complete_support_uv(ir: &mut CadIr, pending: &[PendingExt11SupportUv]) {
    let support_budget = new_support_uv_budget();
    let coupled_support_budget = new_support_uv_budget();
    let geometry_budget = GeometryWorkBudget::new(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    complete_support_uv_with_budget(
        ir,
        pending,
        &support_budget,
        &geometry_budget,
        &coupled_support_budget,
        &geometry_budget,
    );
}

pub(super) fn complete_support_uv_with_budget(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    coupled_support_budget: &SupportUvBudget<'_>,
    coupled_geometry_budget: &GeometryWorkBudget<'_>,
) -> bool {
    // A failed fit can become solvable when its opposite lane is filled by an
    // earlier wave. Keep that dependency as the retry key; repeating the same
    // failed inverse after unrelated progress only burns the model-wide cap.
    let mut failed_attempts = BTreeMap::<(ProceduralCurveId, usize), Option<PcurveGeometry>>::new();
    let mut lane_geometry_exhausted = false;
    loop {
        let before = pending_support_lanes_requiring_completion(ir, pending);
        if support_uv_budget_exhausted(support_budget) {
            break;
        }
        geometry_budget.clear_blend_frame_cache();
        coupled_geometry_budget.clear_blend_frame_cache();
        lane_geometry_exhausted |= complete_support_uv_wave(
            ir,
            pending,
            support_budget,
            geometry_budget,
            coupled_support_budget,
            coupled_geometry_budget,
            &mut failed_attempts,
        );
        let after = pending_support_lanes_requiring_completion(ir, pending);
        if after >= before || support_uv_budget_exhausted(support_budget) {
            break;
        }
    }
    lane_geometry_exhausted
}

#[cfg(test)]
pub(crate) fn invalidate_inconsistent_support_uv(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) {
    let geometry_budget = GeometryWorkBudget::new(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    let support_budget = WorkBudget::new(MAX_SUPPORT_UV_SAMPLES);
    invalidate_inconsistent_support_uv_with_validated_lanes(
        ir,
        pending,
        &BTreeSet::new(),
        &support_budget,
        &geometry_budget,
    );
}

pub(crate) fn invalidate_inconsistent_support_uv_with_validated_lanes(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    validated_lanes: &BTreeSet<(ProceduralCurveId, usize)>,
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) {
    let invalid = {
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        let mut invalid = Vec::new();
        for (procedural_id, points, parameters, fit_tolerance, _) in pending {
            if geometry_budget.exhausted() || support_uv_budget_exhausted(support_budget) {
                break;
            }
            let Some(procedural) = index.procedural_curves(procedural_id.0.as_str()) else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                continue;
            };
            for (side, support) in context.sides.iter().enumerate() {
                if geometry_budget.exhausted() || support_uv_budget_exhausted(support_budget) {
                    break;
                }
                if validated_lanes.contains(&(procedural_id.clone(), side)) {
                    continue;
                }
                let (Some(surface), Some(pcurve)) = (&support.surface, &support.pcurve) else {
                    continue;
                };
                let Some(geometry) = index
                    .surfaces(surface.0.as_str())
                    .map(|surface| &surface.geometry)
                else {
                    continue;
                };
                let tolerance =
                    blend_spine_cache_fit_tolerance_with_index(&index, surface, *fit_tolerance);
                let mut inconsistent = false;
                for (parameter, point) in parameters.iter().zip(points) {
                    if geometry_budget.exhausted() || !support_budget.charge() {
                        break;
                    }
                    let Some(uv) = pcurve_uv(pcurve, *parameter) else {
                        continue;
                    };
                    let Some(actual) = decoded_surface_point_with_geometry_and_budget(
                        &index,
                        surface,
                        geometry,
                        uv.u,
                        uv.v,
                        0,
                        geometry_budget,
                    ) else {
                        break;
                    };
                    if point_distance(actual, *point) > tolerance {
                        inconsistent = true;
                        break;
                    }
                }
                if inconsistent {
                    invalid.push((procedural_id.clone(), side));
                }
            }
        }
        invalid
    };
    for (procedural_id, side) in invalid {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            unreachable!("definition selected above");
        };
        context.sides[side].pcurve = None;
    }
}

pub(crate) fn pending_support_lanes_requiring_completion(
    ir: &CadIr,
    pending: &[PendingExt11SupportUv],
) -> usize {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    pending
        .iter()
        .filter_map(|(procedural_id, ..)| index.procedural_curves(procedural_id.0.as_str()))
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            Some(
                context
                    .sides
                    .iter()
                    .filter(|side| pcurve_requires_completion(side.pcurve.as_ref()))
                    .count(),
            )
        })
        .sum()
}

fn complete_support_uv_wave(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    coupled_support_budget: &SupportUvBudget<'_>,
    coupled_geometry_budget: &GeometryWorkBudget<'_>,
    failed_attempts: &mut BTreeMap<(ProceduralCurveId, usize), Option<PcurveGeometry>>,
) -> bool {
    let mut lane_geometry_exhausted = false;
    if !support_uv_budget_exhausted(support_budget) && !geometry_budget.exhausted() {
        let mut replacements = Vec::new();
        let mut blend_parameter_grids = BTreeMap::<SurfaceId, Option<Vec<(Point2, Point3)>>>::new();
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        for (procedural_id, points, parameters, fit_tolerance, lanes) in pending {
            if support_uv_budget_exhausted(support_budget) {
                break;
            }
            let Some(procedural) = model_index.procedural_curves(procedural_id.0.as_str()) else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                continue;
            };
            for side in 0..2 {
                if support_uv_budget_exhausted(support_budget) {
                    break;
                }
                if !pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
                    continue;
                }
                let Some(surface_id) = &context.sides[side].surface else {
                    continue;
                };
                let attempt_key = (procedural_id.clone(), side);
                let source_pcurve = context.sides[1 - side].pcurve.as_ref();
                let other_surface_id = context.sides[1 - side].surface.as_ref();
                if failed_attempts
                    .get(&attempt_key)
                    .is_some_and(|previous| previous.as_ref() == source_pcurve)
                {
                    continue;
                }
                let Some(surface) = model_index.surfaces(surface_id.0.as_str()) else {
                    continue;
                };
                let source_chart_available =
                    source_pcurve.is_some_and(|pcurve| !pcurve_requires_completion(Some(pcurve)));
                let effective_fit_tolerance = blend_spine_cache_fit_tolerance_with_index(
                    &model_index,
                    surface_id,
                    *fit_tolerance,
                );
                let other_support = {
                    let other_side = &context.sides[1 - side];
                    other_side
                        .surface
                        .as_ref()
                        .zip(other_side.pcurve.as_ref())
                        .and_then(|(other_surface, other_pcurve)| {
                            let geometry = model_index
                                .surfaces(other_surface.0.as_str())
                                .map(|surface| &surface.geometry)?;
                            Some((other_surface, other_pcurve, geometry))
                        })
                };
                let other_contact =
                    other_support.and_then(|(other_surface, other_pcurve, other_geometry)| {
                        let (supports, spine, radius, _) =
                            blend_surface_definition_with_index(&model_index, surface_id)?;
                        let boundaries = supports
                            .iter()
                            .enumerate()
                            .filter(|(_, candidate)| {
                                parameterization_equivalent_surfaces_with_index(
                                    &model_index,
                                    candidate,
                                    other_surface,
                                )
                            })
                            .map(|(boundary, _)| boundary)
                            .collect::<Vec<_>>();
                        let [boundary] = boundaries.as_slice() else {
                            return None;
                        };
                        let contact_pcurve = spine_contact_pcurve_with_index(
                            &model_index,
                            other_surface,
                            &spine,
                            radius,
                            0,
                        )?;
                        Some((
                            other_surface,
                            other_pcurve,
                            other_geometry,
                            contact_pcurve,
                            *boundary,
                        ))
                    });
                let parent_geometry_budget = geometry_budget;
                let lane_geometry_budget =
                    parent_geometry_budget.child_slice(support_uv_lane_geometry_work_limit(
                        points.len(),
                        parent_geometry_budget.remaining(),
                    ));
                let geometry_budget = &lane_geometry_budget;
                let mut contact_seeds = BlendContactSeedCache::default();
                let mut uv = Vec::with_capacity(points.len().min(support_budget.remaining()));
                let mut all_parameters_certified = true;
                for (point_index, point) in points.iter().enumerate() {
                    if !support_budget.charge() {
                        uv.clear();
                        break;
                    }
                    let serialized_seeds = serialized_support_uv_seed_candidates(
                        &surface.geometry,
                        lanes,
                        point_index,
                    );
                    let seed_candidates = [
                        serialized_seeds[0],
                        serialized_seeds[1],
                        pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), point_index),
                        uv.last().copied(),
                        None,
                    ]
                    .into_iter();
                    let mut attempted_without_seed = false;
                    let mut solved = None;
                    for seed in seed_candidates {
                        if seed.is_none() {
                            if attempted_without_seed {
                                continue;
                            }
                            attempted_without_seed = true;
                        }
                        let candidate = match &surface.geometry {
                            SurfaceGeometry::Nurbs(nurbs) => {
                                nurbs_surface_parameter_within_tolerance_with_budget(
                                    nurbs,
                                    *point,
                                    seed,
                                    effective_fit_tolerance,
                                    geometry_budget,
                                )
                                .map(|parameters| (parameters, true))
                            }
                            SurfaceGeometry::Procedural { .. } => {
                                let solve_blend_parameters = if source_chart_available {
                                    blend_surface_parameters_for_fit_with_source_continuation_and_budget
                                } else {
                                    blend_surface_parameters_for_fit_with_grid_and_budget
                                };
                                let solve_grid_parameters = if source_chart_available {
                                    blend_surface_parameters_from_grid_for_fit_with_source_continuation_and_budget
                                } else {
                                    blend_surface_parameters_from_grid_for_fit_and_budget
                                };
                                source_chart_available
                                    .then(|| {
                                        source_pcurve
                                            .zip(other_surface_id)
                                            .and_then(|(source_pcurve, source_surface)| {
                                                blend_support_parameter_from_source_pcurve_with_index_and_budget_and_seed_cache(
                                                    &model_index,
                                                    source_surface,
                                                    surface_id,
                                                    source_pcurve,
                                                    parameters[point_index],
                                                    BoundaryInverseTarget {
                                                        point: *point,
                                                        seed,
                                                        tolerance: effective_fit_tolerance,
                                                    },
                                                    &mut contact_seeds,
                                                    geometry_budget,
                                                )
                                            })
                                    })
                                    .flatten()
                                .map(|parameters| (parameters, true))
                                .or_else(|| other_contact
                                .and_then(
                                    |(
                                        other_surface,
                                        other_pcurve,
                                        other_geometry,
                                        contact_pcurve,
                                        boundary,
                                    )| {
                                        blend_boundary_parameter_from_contact_pcurve_with_geometry_and_budget(
                                            &model_index,
                                            other_surface,
                                            other_geometry,
                                            contact_pcurve,
                                            boundary,
                                            other_pcurve,
                                            parameters[point_index],
                                            BoundaryInverseTarget {
                                                point: *point,
                                                seed,
                                                tolerance: effective_fit_tolerance,
                                            },
                                            geometry_budget,
                                        )
                                    },
                                )
                                .map(|parameters| (parameters, true))
                                )
                                .or_else(|| {
                                    blend_surface_parameters_from_point_with_index_and_budget(
                                        &model_index,
                                        surface_id,
                                        *point,
                                        seed,
                                        effective_fit_tolerance,
                                        &mut contact_seeds,
                                        geometry_budget,
                                    )
                                    .map(|parameters| (parameters, true))
                                })
                                .or_else(|| {
                                    other_surface_id.and_then(|other_surface| {
                                        blend_boundary_parameter_from_support_spine_with_index_and_budget(
                                            &model_index,
                                            surface_id,
                                            other_surface,
                                            *point,
                                            seed,
                                            effective_fit_tolerance,
                                            geometry_budget,
                                        )
                                        .map(|parameters| (parameters, true))
                                    })
                                })
                                .or_else(|| {
                                    seed.and_then(|seed| {
                                        refine_offset_surface_parameters_with_index_and_budget(
                                            &model_index,
                                            surface_id,
                                            *point,
                                            seed,
                                            effective_fit_tolerance,
                                            geometry_budget,
                                        )
                                    })
                                    .or_else(|| {
                                        seed.is_none().then(|| {
                                            offset_surface_parameters_with_tolerance_with_index_and_budget(
                                                &model_index,
                                                surface_id,
                                                *point,
                                                None,
                                                Some(effective_fit_tolerance),
                                                geometry_budget,
                                            )
                                        })?
                                    })
                                    .map(|parameters| (parameters, true))
                                })
                                .or_else(|| {
                                    solve_blend_parameters(
                                        &model_index,
                                        surface_id,
                                        *point,
                                        seed,
                                        effective_fit_tolerance,
                                        BlendParameterGrid::Disabled,
                                        geometry_budget,
                                    )
                                    .map(|parameters| (parameters, true))
                                })
                                .or_else(|| {
                                    let blend_grid = blend_parameter_grids
                                        .entry(surface_id.clone())
                                        .or_insert_with(|| {
                                            blend_surface_parameter_grid_with_index_and_budget(
                                                &model_index,
                                                surface_id,
                                                0,
                                                geometry_budget,
                                            )
                                        });
                                    solve_grid_parameters(
                                        &model_index,
                                        surface_id,
                                        *point,
                                        effective_fit_tolerance,
                                        blend_grid.as_deref()?,
                                        geometry_budget,
                                    )
                                    .map(|parameters| (parameters, true))
                                })
                            }
                            geometry => analytic_surface_parameters(geometry, *point)
                                .map(|parameters| (parameters, false)),
                        };
                        if candidate.is_some() {
                            solved = candidate;
                            break;
                        }
                    }
                    let Some((parameters, certified)) = solved else {
                        uv.clear();
                        break;
                    };
                    all_parameters_certified &= certified;
                    uv.push(parameters);
                }
                if uv.len() != points.len() {
                    lane_geometry_exhausted |= lane_geometry_budget.exhausted();
                    let _ = parent_geometry_budget.consume_child(&lane_geometry_budget);
                    failed_attempts.insert(attempt_key, source_pcurve.cloned());
                    continue;
                }
                if matches!(
                    surface.geometry,
                    SurfaceGeometry::Cylinder { .. }
                        | SurfaceGeometry::Cone { .. }
                        | SurfaceGeometry::Sphere { .. }
                        | SurfaceGeometry::Torus { .. }
                ) {
                    for index in 1..uv.len() {
                        let turns =
                            ((uv[index - 1].u - uv[index].u) / std::f64::consts::TAU).round();
                        uv[index].u += turns * std::f64::consts::TAU;
                    }
                }
                let reproduces_chart = all_parameters_certified
                    || uv.iter().zip(points).all(|(uv, point)| {
                        decoded_surface_point_with_geometry_and_budget(
                            &model_index,
                            surface_id,
                            &surface.geometry,
                            uv.u,
                            uv.v,
                            0,
                            geometry_budget,
                        )
                        .is_some_and(|actual| {
                            point_distance(actual, *point) <= effective_fit_tolerance
                        })
                    });
                if reproduces_chart {
                    replacements.push((
                        procedural_id.clone(),
                        side,
                        PcurveGeometry::Nurbs {
                            degree: 1,
                            knots: linear_knots(parameters),
                            control_points: uv,
                            weights: None,
                            periodic: false,
                        },
                        effective_fit_tolerance,
                    ));
                } else {
                    failed_attempts.insert(attempt_key, source_pcurve.cloned());
                }
                lane_geometry_exhausted |= lane_geometry_budget.exhausted();
                let _ = parent_geometry_budget.consume_child(&lane_geometry_budget);
            }
        }
        let cache_backed_curves = ir
            .model
            .curves
            .iter()
            .filter(|curve| !matches!(&curve.geometry, CurveGeometry::Procedural { .. }))
            .map(|curve| curve.id.clone())
            .collect::<BTreeSet<_>>();
        for (procedural_id, side, pcurve, effective_fit_tolerance) in replacements {
            let Some(procedural) = ir
                .model
                .procedural_curves
                .iter_mut()
                .find(|procedural| procedural.id == procedural_id)
            else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } =
                &mut procedural.definition
            else {
                continue;
            };
            if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
                context.sides[side].pcurve = Some(pcurve);
                if cache_backed_curves.contains(&procedural.curve) {
                    procedural.cache_fit_tolerance = Some(
                        procedural
                            .cache_fit_tolerance
                            .unwrap_or(0.0)
                            .max(effective_fit_tolerance),
                    );
                }
            }
        }
    }

    // Independent inverse admission is the cheapest certified route. Run
    // coupled continuation only after this wave has had a chance to fill the
    // same lanes, so difficult nested supports are reserved for residuals.
    if !coupled_geometry_budget.exhausted() {
        coupled_geometry_budget.clear_blend_frame_cache();
        lane_geometry_exhausted |= complete_coupled_support_uv(
            ir,
            pending,
            coupled_support_budget,
            coupled_geometry_budget,
        );
    }
    lane_geometry_exhausted
}

#[cfg(test)]
pub(super) fn blend_spine_cache_fit_tolerance(
    ir: &CadIr,
    surface: &SurfaceId,
    fit_tolerance: f64,
) -> f64 {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    blend_spine_cache_fit_tolerance_with_index(&index, surface, fit_tolerance)
}

pub(crate) fn blend_spine_cache_fit_tolerance_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    fit_tolerance: f64,
) -> f64 {
    blend_surface_definition_with_index(index, surface)
        .and_then(|(_, spine, _, _)| {
            index
                .procedural_curves_for_curve(spine.0.as_str())
                .and_then(|procedurals| procedurals.first().copied())
                .and_then(|procedural| procedural.cache_fit_tolerance)
        })
        .filter(|tolerance| tolerance.is_finite() && *tolerance > 0.0)
        .map_or(fit_tolerance, |tolerance| fit_tolerance + tolerance)
}

fn complete_blend_boundary_support_uv_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    seeds: [Option<Point2>; 2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<[Vec<Point2>; 2]> {
    let (blend_side, support_side) = (0..2).find_map(|blend_side| {
        let (supports, _, _, _) = blend_surface_definition_with_index(index, surfaces[blend_side])?;
        let support_matches = supports
            .iter()
            .filter(|support| {
                parameterization_equivalent_surfaces_with_index(
                    index,
                    support,
                    surfaces[1 - blend_side],
                )
            })
            .count();
        (support_matches == 1).then_some((blend_side, 1 - blend_side))
    })?;
    let mut lanes = [
        Vec::with_capacity(points.len()),
        Vec::with_capacity(points.len()),
    ];
    for point in points {
        let blend_seed = lanes[blend_side].last().copied().or(seeds[blend_side]);
        let support_seed = lanes[support_side].last().copied().or(seeds[support_side]);
        let blend_parameters = blend_boundary_parameter_from_support_spine_with_index_and_budget(
            index,
            surfaces[blend_side],
            surfaces[support_side],
            *point,
            blend_seed,
            fit_tolerance,
            geometry_budget,
        )?;
        let support_parameters = surface_parameters_for_fit_with_index_and_budget(
            index,
            surfaces[support_side],
            *point,
            support_seed,
            fit_tolerance,
            geometry_budget,
        )?;
        lanes[blend_side].push(blend_parameters);
        lanes[support_side].push(support_parameters);
    }
    Some(lanes)
}

fn complete_coupled_support_uv(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    coupled_support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> bool {
    if geometry_budget.exhausted() {
        return false;
    }
    let mut lane_geometry_exhausted = false;
    let mut replacements = Vec::new();
    let mut blend_parameter_grids = BTreeMap::<SurfaceId, Option<Vec<(Point2, Point3)>>>::new();
    let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    for (procedural_id, points, parameters, fit_tolerance, _) in pending {
        let Some(procedural) = model_index.procedural_curves(procedural_id.0.as_str()) else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        let missing = context
            .sides
            .each_ref()
            .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
        let [Some(first_surface), Some(second_surface)] =
            context.sides.each_ref().map(|side| side.surface.as_ref())
        else {
            continue;
        };
        let surfaces = [first_surface, second_surface];
        let both_lanes_missing = missing == [true, true];
        let has_procedural_support = surfaces.iter().any(|surface| {
            model_index
                .surfaces(surface.0.as_str())
                .is_some_and(|surface| {
                    matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                })
        });
        let seeded_procedural_support = (0..2).any(|side| {
            missing[side]
                && pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), 0).is_some()
                && model_index
                    .surfaces(surfaces[side].0.as_str())
                    .is_some_and(|surface| {
                        matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                    })
        });
        let sourced_procedural_support = (0..2).any(|side| {
            missing[side]
                && context.sides[1 - side]
                    .pcurve
                    .as_ref()
                    .is_some_and(|pcurve| !pcurve_requires_completion(Some(pcurve)))
                && model_index
                    .surfaces(surfaces[side].0.as_str())
                    .is_some_and(|surface| {
                        matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                    })
        });
        if !(seeded_procedural_support
            || sourced_procedural_support
            || (both_lanes_missing && has_procedural_support))
        {
            continue;
        }
        let missing_lanes = missing.iter().filter(|missing| **missing).count();
        if !coupled_support_budget.charge_by(points.len().saturating_mul(missing_lanes).max(1)) {
            break;
        }
        let seeds = context
            .sides
            .each_ref()
            .map(|side| pcurve_control_point_seed(side.pcurve.as_ref(), 0));
        let parent_geometry_budget = geometry_budget;
        let lane_geometry_budget = parent_geometry_budget.child_slice(
            support_uv_lane_geometry_work_limit(points.len(), parent_geometry_budget.remaining()),
        );
        let geometry_budget = &lane_geometry_budget;
        let lanes = complete_blend_boundary_support_uv_with_index_and_budget(
            &model_index,
            surfaces,
            points,
            *fit_tolerance,
            seeds,
            geometry_budget,
        )
        .or_else(|| {
            continue_surface_intersection_parameters_with_index_and_seeds_and_budget_and_grid_cache(
                &model_index,
                surfaces,
                points,
                *fit_tolerance,
                seeds,
                geometry_budget,
                &mut blend_parameter_grids,
            )
        });
        lane_geometry_exhausted |= lane_geometry_budget.exhausted();
        let _ = parent_geometry_budget.consume_child(&lane_geometry_budget);
        let Some(lanes) = lanes else {
            continue;
        };
        for side in 0..2 {
            if missing[side] {
                replacements.push((
                    procedural_id.clone(),
                    side,
                    PcurveGeometry::Nurbs {
                        degree: 1,
                        knots: linear_knots(parameters),
                        control_points: lanes[side].clone(),
                        weights: None,
                        periodic: false,
                    },
                ));
            }
        }
    }
    drop(model_index);
    for (procedural_id, side, pcurve) in replacements {
        let Some(procedural) = ir
            .model
            .procedural_curves
            .iter_mut()
            .find(|procedural| procedural.id == procedural_id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = &mut procedural.definition
        else {
            continue;
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
        }
    }
    lane_geometry_exhausted
}

#[cfg(test)]
pub(super) fn complete_coupled_support_uv_for_test(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) {
    let coupled_support_budget = new_support_uv_budget();
    let geometry_budget = GeometryWorkBudget::new(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    complete_coupled_support_uv(ir, pending, &coupled_support_budget, &geometry_budget);
}

pub(crate) fn complete_parameterization_equivalent_support_uv(ir: &mut CadIr) {
    let replacements = {
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        ir.model
            .procedural_curves
            .iter()
            .enumerate()
            .filter_map(|(procedural_index, procedural)| {
                let ProceduralCurveDefinition::Intersection { context, .. } =
                    &procedural.definition
                else {
                    return None;
                };
                let missing = context
                    .sides
                    .each_ref()
                    .map(|side| pcurve_requires_completion(side.pcurve.as_ref()));
                let target = match missing {
                    [true, false] => 0,
                    [false, true] => 1,
                    _ => return None,
                };
                let source = 1 - target;
                let (Some(target_surface), Some(source_surface), Some(source_pcurve)) = (
                    context.sides[target].surface.as_ref(),
                    context.sides[source].surface.as_ref(),
                    context.sides[source].pcurve.as_ref(),
                ) else {
                    return None;
                };
                parameterization_equivalent_surfaces_with_index(
                    &model_index,
                    target_surface,
                    source_surface,
                )
                .then(|| (procedural_index, target, source_pcurve.clone()))
            })
            .collect::<Vec<_>>()
    };
    for (procedural_index, side, pcurve) in replacements {
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &mut ir.model.procedural_curves[procedural_index].definition
        else {
            unreachable!("definition selected above");
        };
        if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
            context.sides[side].pcurve = Some(pcurve);
        }
    }
}

#[cfg(test)]
pub(crate) fn parameterization_equivalent_surfaces(
    ir: &CadIr,
    first: &SurfaceId,
    second: &SurfaceId,
) -> bool {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    parameterization_equivalent_surfaces_with_index(&index, first, second)
}

pub(crate) fn parameterization_equivalent_surfaces_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    first: &SurfaceId,
    second: &SurfaceId,
) -> bool {
    fn equivalent(
        index: &cadmpeg_ir::index::ModelIndex<'_>,
        first: &SurfaceId,
        second: &SurfaceId,
        visited: &mut BTreeSet<(SurfaceId, SurfaceId)>,
    ) -> bool {
        if first == second {
            return true;
        }
        if !visited.insert((first.clone(), second.clone())) {
            return false;
        }
        let geometry = |id: &SurfaceId| {
            index
                .surfaces(id.0.as_str())
                .map(|surface| &surface.geometry)
        };
        let (Some(first_geometry), Some(second_geometry)) = (geometry(first), geometry(second))
        else {
            return false;
        };
        if first_geometry == second_geometry {
            return true;
        }
        let (
            Some(ProceduralSurfaceDefinition::Offset {
                support: first_support,
                distance: first_distance,
                u_sense: first_u_sense,
                v_sense: first_v_sense,
                extension_flags: first_extensions,
                ..
            }),
            Some(ProceduralSurfaceDefinition::Offset {
                support: second_support,
                distance: second_distance,
                u_sense: second_u_sense,
                v_sense: second_v_sense,
                extension_flags: second_extensions,
                ..
            }),
        ) = (
            index
                .procedural_surface_for_carrier(first.0.as_str())
                .map(|surface| &surface.definition),
            index
                .procedural_surface_for_carrier(second.0.as_str())
                .map(|surface| &surface.definition),
        )
        else {
            return false;
        };
        first_distance.to_bits() == second_distance.to_bits()
            && first_u_sense == second_u_sense
            && first_v_sense == second_v_sense
            && first_extensions == second_extensions
            && equivalent(index, first_support, second_support, visited)
    }

    equivalent(index, first, second, &mut BTreeSet::new())
}

#[cfg(test)]
pub(crate) fn attach_completed_intersection_pcurves(
    ir: &mut CadIr,
    graph: &Graph,
    prefix: &str,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let geometry_budget = GeometryWorkBudget::new(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    attach_completed_intersection_pcurves_with_budget(
        ir,
        graph,
        prefix,
        source_stream,
        annotations,
        &geometry_budget,
    );
}

#[cfg(test)]
pub(crate) fn attach_completed_intersection_pcurves_with_budget(
    ir: &mut CadIr,
    graph: &Graph,
    prefix: &str,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
    geometry_budget: &GeometryWorkBudget<'_>,
) {
    attach_completed_intersection_pcurves_for_stream_with_budget(
        ir,
        graph,
        prefix,
        0,
        0,
        source_stream,
        annotations,
        geometry_budget,
    );
}

// Keep stream ownership bounds explicit so this phase does not rescan prior coedges.
#[allow(clippy::too_many_arguments)]
pub(crate) fn attach_completed_intersection_pcurves_for_stream_with_budget(
    ir: &mut CadIr,
    graph: &Graph,
    prefix: &str,
    coedge_start: usize,
    procedural_start: usize,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
    geometry_budget: &GeometryWorkBudget<'_>,
) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (&loop_.id, &loop_.face))
        .collect::<BTreeMap<_, _>>();
    let face_surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| (&face.id, &face.surface))
        .collect::<BTreeMap<_, _>>();
    let edge_curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((&edge.id, edge.curve.as_ref()?)))
        .collect::<BTreeMap<_, _>>();
    let edge_tolerances = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| Some((&edge.id, edge.tolerance?)))
        .collect::<BTreeMap<_, _>>();
    let coedge_candidates = ir
        .model
        .coedges
        .iter()
        .skip(coedge_start)
        .filter(|coedge| coedge.pcurves.is_empty() && coedge.id.0.starts_with(prefix))
        .filter_map(|coedge| {
            let surface = loop_faces
                .get(&coedge.owner_loop)
                .and_then(|face| face_surfaces.get(*face))?;
            let curve = edge_curves.get(&coedge.edge)?;
            Some((
                coedge.id.clone(),
                coedge.edge.clone(),
                (*curve).clone(),
                (*surface).clone(),
                edge_tolerances.get(&coedge.edge).copied(),
            ))
        })
        .collect::<Vec<_>>();
    if coedge_candidates.is_empty() {
        return;
    }
    let required_keys = coedge_candidates
        .iter()
        .map(|(_, _, curve, surface, _)| (curve.clone(), surface.clone()))
        .collect::<BTreeSet<_>>();
    let mut candidates =
        BTreeMap::<(CurveId, SurfaceId), Vec<(PcurveGeometry, [f64; 2], Option<f64>)>>::new();
    for procedural in ir.model.procedural_curves.iter().skip(procedural_start) {
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            continue;
        };
        for side in &context.sides {
            let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            let key = (procedural.curve.clone(), surface.clone());
            if !required_keys.contains(&key) {
                continue;
            }
            let values = candidates.entry(key).or_default();
            let candidate = (
                pcurve.clone(),
                context.parameter_range,
                procedural.cache_fit_tolerance,
            );
            if !values.contains(&candidate) {
                values.push(candidate);
            }
        }
    }

    let replacements = {
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        let edge_endpoint_contracts = coedge_candidates
            .iter()
            .map(|(_, edge_id, ..)| edge_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|edge_id| {
                pcurve_edge_endpoint_contract_with_index(&model_index, &edge_id)
                    .map(|contract| (edge_id, contract))
            })
            .collect::<BTreeMap<_, _>>();
        // A chart carrier's serialized endpoint witnesses are a necessary
        // edge-incidence condition. Keep full face-surface evaluation for
        // every surviving key.
        let endpoint_admissible_keys = coedge_candidates
            .iter()
            .filter_map(|(_, edge_id, curve, surface, edge_tolerance)| {
                let key = (curve.clone(), surface.clone());
                let [candidate] = candidates.get(&key)?.as_slice() else {
                    return None;
                };
                let Some(witness) =
                    linear_nurbs_curve_endpoint_witness_with_index(&model_index, curve)
                else {
                    return Some(key);
                };
                let (edge_endpoints, edge_allowance) =
                    edge_endpoint_contracts.get(edge_id).copied()?;
                let fit_tolerance = candidate.2.or(*edge_tolerance);
                pcurve_matches_edge_endpoint_contract(
                    witness,
                    edge_endpoints,
                    edge_allowance,
                    fit_tolerance,
                )
                .then_some(key)
            })
            .collect::<BTreeSet<_>>();
        let candidate_endpoints = candidates
            .iter()
            .filter(|(key, _)| endpoint_admissible_keys.contains(key))
            .filter_map(|(key, values)| {
                let [candidate] = values.as_slice() else {
                    return None;
                };
                Some((
                    key.clone(),
                    pcurve_surface_endpoints_with_index_and_budget(
                        &model_index,
                        &key.1,
                        &candidate.0,
                        None,
                        geometry_budget,
                    ),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        coedge_candidates
            .into_iter()
            .filter_map(|(coedge_id, edge_id, curve, surface, edge_tolerance)| {
                let [candidate] = candidates
                    .get(&(curve.clone(), surface.clone()))?
                    .as_slice()
                else {
                    return None;
                };
                let coincident_surface = candidate_endpoints
                    .get(&(curve, surface.clone()))?
                    .as_ref()?;
                let (edge_endpoints, edge_allowance) =
                    edge_endpoint_contracts.get(&edge_id).copied()?;
                let fit_tolerance = candidate.2.or(edge_tolerance);
                pcurve_matches_edge_endpoint_contract(
                    *coincident_surface,
                    edge_endpoints,
                    edge_allowance,
                    fit_tolerance,
                )
                .then(|| (coedge_id, (candidate.0.clone(), candidate.1, fit_tolerance)))
            })
            .collect::<Vec<_>>()
    };
    for (coedge_id, (geometry, parameter_range, fit_tolerance)) in replacements {
        let Some(fin_xmt) = coedge_id
            .0
            .rsplit_once('#')
            .and_then(|(_, value)| value.parse::<u32>().ok())
        else {
            continue;
        };
        let pcurve_id = PcurveId(format!("{prefix}:intersection-pcurve-completed#{fin_xmt}"));
        if ir.model.pcurves.iter().any(|pcurve| pcurve.id == pcurve_id) {
            continue;
        }
        let source_offset = graph.get(17, fin_xmt).map_or(0, |node| node.pos as u64);
        annotations
            .note(&pcurve_id, source_stream, source_offset)
            .tag("INTERSECTION_PCURVE");
        annotations.derived(&pcurve_id, "geometry");
        annotations.derived(&pcurve_id, "parameter_range");
        if fit_tolerance.is_some() {
            annotations.derived(&pcurve_id, "fit_tolerance");
        }
        ir.model.pcurves.push(Pcurve {
            id: pcurve_id.clone(),
            geometry,
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some(parameter_range),
            fit_tolerance,
        });
        if let Some(coedge) = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_id && coedge.pcurves.is_empty())
        {
            coedge.pcurves.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: pcurve_id,
                isoparametric: None,
                parameter_range: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_serialized_lane_is_declined_before_geometry_work() {
        let surface_id = SurfaceId("synthetic:support-plane".into());
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
        let points = vec![Point3::new(0.0, 0.0, 0.0); MAX_SUPPORT_UV_SAMPLES + 1];
        let values = vec![[0.0, 0.0]; MAX_SUPPORT_UV_SAMPLES + 1];
        let geometry_budget = GeometryWorkBudget::new(1);

        assert!(!support_uv_lane_matches_surface_with_budget(
            &index,
            &surface_id,
            &points,
            0.0,
            Some(&values),
            &geometry_budget,
        ));
        assert_eq!(geometry_budget.remaining(), 1);
    }

    #[test]
    fn support_uv_lane_geometry_slice_preserves_parent_fairness() {
        let parent = WorkBudget::new(MAX_SUPPORT_UV_LANE_GEOMETRY_WORK * 2);
        let lane_limit =
            support_uv_lane_geometry_work_limit(MAX_SUPPORT_UV_SAMPLES, parent.remaining());
        let lane = parent.child_slice(lane_limit);

        assert_eq!(lane_limit, MAX_SUPPORT_UV_LANE_GEOMETRY_WORK);
        assert!(lane.charge_by(lane_limit));
        assert!(!lane.charge());
        assert_eq!(parent.consumed(), 0);
        assert!(!parent.exhausted());

        assert!(parent.consume_child(&lane));
        assert_eq!(parent.consumed(), MAX_SUPPORT_UV_LANE_GEOMETRY_WORK);
        assert!(!parent.exhausted());

        let later_lane = parent.child_slice(support_uv_lane_geometry_work_limit(
            MAX_SUPPORT_UV_SAMPLES,
            parent.remaining(),
        ));
        assert!(later_lane.charge());
    }
}
