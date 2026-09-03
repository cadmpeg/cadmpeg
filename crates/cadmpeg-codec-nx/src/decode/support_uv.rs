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
    coarse_model_surface_parameters,
    continue_surface_intersection_parameters_with_index_and_seeds_and_budget_and_grid_cache,
    offset_surface_parameters_with_tolerance_with_index_and_budget, point_distance,
    refine_offset_surface_parameters_with_index_and_budget, surface_parameter_domain_with_index,
    surface_parameters,
};
use super::pcurves::{
    blend_boundary_parameter_from_support_spine_with_index_and_budget,
    endpoint_witness_for_candidate, linear_nurbs_curve_endpoint_witness_with_index,
    pcurve_edge_endpoint_contract_with_index, pcurve_matches_edge_endpoint_contract,
    pcurve_surface_endpoints_with_index_and_budget,
    surface_parameters_for_fit_with_index_and_budget, EndpointWitnesses,
};
use super::MISSING_TOLERANCE;
use crate::topology::Graph;
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    analytic_surface_parameters, nurbs_surface_parameter_within_tolerance_with_budget, pcurve_uv,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, OffsetSupportExtension, Pcurve, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
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

pub(crate) type SupportUvLanes = [Option<Vec<[f64; 2]>>; 2];

/// Serialized support-UV lanes retained with one charted intersection.
///
/// The values-array lanes and EXT11 chart lanes have different admission
/// rules. Values-array lanes are ordered by support side; EXT11 lanes remain
/// in their serialized order until their surface identity is proven. Both
/// sources are useful as seeds for coupled completion, but only EXT11 lanes
/// participate in EXT11 assignment.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SerializedSupportUv {
    pub(crate) values: SupportUvLanes,
    pub(crate) ext11: SupportUvLanes,
}

#[cfg(test)]
impl SerializedSupportUv {
    pub(crate) fn from_values(values: SupportUvLanes) -> Self {
        Self {
            values,
            ext11: [None, None],
        }
    }

    pub(crate) fn from_ext11(ext11: SupportUvLanes) -> Self {
        Self {
            values: [None, None],
            ext11,
        }
    }
}

pub(crate) type PendingExt11SupportUv = (
    ProceduralCurveId,
    Vec<Point3>,
    Vec<f64>,
    f64,
    SerializedSupportUv,
);

/// Return endpoint witnesses already certified by serialized support-UV lanes.
/// The lane samples and the intersection parameter range use the same ordered
/// parameter domain, so its first and last model-space samples are the
/// pcurve's endpoint witnesses.
pub(crate) fn validated_support_uv_endpoint_witnesses(
    ir: &CadIr,
    pending: &[PendingExt11SupportUv],
    validated_lanes: &BTreeSet<(ProceduralCurveId, usize)>,
) -> EndpointWitnesses {
    let procedural_by_id = ir
        .model
        .procedural_curves
        .iter()
        .map(|procedural| (&procedural.id, procedural))
        .collect::<BTreeMap<_, _>>();
    let mut witnesses: EndpointWitnesses = BTreeMap::new();
    for (procedural_id, points, parameters, _, _) in pending {
        if points.len() < 2 || points.len() != parameters.len() {
            continue;
        }
        let Some(procedural) = procedural_by_id.get(procedural_id).copied() else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let expected_range = [
            parameters[0],
            *parameters.last().expect("at least two points"),
        ];
        if context.parameter_range != expected_range {
            continue;
        }
        for (side, support) in context.sides.iter().enumerate() {
            if !validated_lanes.contains(&(procedural_id.clone(), side))
                || pcurve_requires_completion(support.pcurve.as_ref())
            {
                continue;
            }
            let Some(surface) = support.surface.clone() else {
                continue;
            };
            let Some(pcurve) = support.pcurve.clone() else {
                continue;
            };
            witnesses
                .entry((procedural.curve.clone(), surface))
                .or_default()
                .push((
                    pcurve,
                    context.parameter_range,
                    [points[0], *points.last().expect("at least two points")],
                ));
        }
    }
    witnesses
}

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
    serialized: &SerializedSupportUv,
    side: usize,
    point_index: usize,
) -> [Option<Point2>; 4] {
    let lane_order = [side, 1 - side];
    std::array::from_fn(|candidate| {
        let lanes = if candidate < 2 {
            &serialized.values
        } else {
            &serialized.ext11
        };
        let lane = lane_order[candidate % 2];
        let [u, v] = *lanes[lane].as_deref()?.get(point_index)?;
        (u.is_finite()
            && v.is_finite()
            && !missing_support_parameter(u)
            && !missing_support_parameter(v))
        .then(|| surface_parameters(geometry, [u, v]))?
    })
}

fn ordered_support_uv_seed_candidates(
    serialized: [Option<Point2>; 4],
    retained_pcurve: Option<Point2>,
    continuation: Option<Point2>,
    linear_offset_surface: bool,
) -> [Option<Point2>; 7] {
    if linear_offset_surface && continuation.is_some() {
        [
            continuation,
            serialized[0],
            serialized[1],
            serialized[2],
            serialized[3],
            retained_pcurve,
            None,
        ]
    } else {
        [
            serialized[0],
            serialized[1],
            serialized[2],
            serialized[3],
            retained_pcurve,
            continuation,
            None,
        ]
    }
}

fn unseeded_nurbs_surface_parameters_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface_id: &SurfaceId,
    surface: &SurfaceGeometry,
    nurbs: &cadmpeg_ir::geometry::NurbsSurface,
    point: Point3,
    fit_tolerance: f64,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<Point2> {
    let coarse = surface_parameter_domain_with_index(index, surface_id).and_then(|domain| {
        coarse_model_surface_parameters(index, surface_id, point, domain, geometry_budget)
    });
    if let Some(parameters) = coarse.filter(|parameters| {
        decoded_surface_point_with_geometry_and_budget(
            index,
            surface_id,
            surface,
            parameters.u,
            parameters.v,
            0,
            geometry_budget,
        )
        .is_some_and(|candidate| point_distance(candidate, point) <= fit_tolerance)
    }) {
        return Some(parameters);
    }
    nurbs_surface_parameter_within_tolerance_with_budget(
        nurbs,
        point,
        coarse,
        fit_tolerance,
        geometry_budget,
    )
}

fn serialized_support_uv_seed_for_side(
    geometry: &SurfaceGeometry,
    serialized: &SerializedSupportUv,
    side: usize,
) -> Option<Point2> {
    let lane_order = [side, 1 - side];
    [
        (&serialized.values, lane_order[0]),
        (&serialized.ext11, lane_order[0]),
        (&serialized.ext11, lane_order[1]),
        (&serialized.values, lane_order[1]),
    ]
    .into_iter()
    .find_map(|(lanes, lane)| {
        let [u, v] = *lanes[lane].as_deref()?.first()?;
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
    for (procedural_id, points, parameters, fit_tolerance, serialized) in pending {
        let Some(procedural) = model_index.procedural_curves(procedural_id.0.as_str()) else {
            continue;
        };
        let (surfaces, missing) = match procedural.definition() {
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
            &serialized.ext11,
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
        procedural.edit_definition(|definition| {
            if let ProceduralCurveDefinition::Intersection { context, .. } = definition {
                context.sides[side].pcurve = Some(replacement);
            }
        });
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

#[cfg(test)]
pub(super) fn complete_support_uv_with_budget(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    coupled_support_budget: &SupportUvBudget<'_>,
    coupled_geometry_budget: &GeometryWorkBudget<'_>,
) -> bool {
    let mut endpoint_witnesses = BTreeMap::new();
    complete_support_uv_with_budget_and_endpoint_witnesses(
        ir,
        pending,
        support_budget,
        geometry_budget,
        coupled_support_budget,
        coupled_geometry_budget,
        &mut endpoint_witnesses,
    )
}

pub(super) fn complete_support_uv_with_budget_and_endpoint_witnesses(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    coupled_support_budget: &SupportUvBudget<'_>,
    coupled_geometry_budget: &GeometryWorkBudget<'_>,
    endpoint_witnesses: &mut EndpointWitnesses,
) -> bool {
    // A failed fit can become solvable when either lane is filled by an
    // earlier wave. Keep those dependencies as the direct and coupled retry
    // keys; unrelated progress must not repeat the same inverse problems.
    let mut failed_attempts = BTreeMap::<(ProceduralCurveId, usize), Option<PcurveGeometry>>::new();
    let mut failed_coupled_attempts =
        BTreeMap::<ProceduralCurveId, [Option<PcurveGeometry>; 2]>::new();
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
            &mut failed_coupled_attempts,
            endpoint_witnesses,
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
    let _ = invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
        ir,
        pending,
        &BTreeSet::new(),
        &support_budget,
        &geometry_budget,
        false,
    );
}

/// Invalidate support lanes that disagree with their surface and retain
/// endpoint witnesses only for lanes whose complete sample set was evaluated.
pub(crate) struct SupportUvValidationResult {
    pub(crate) endpoint_witnesses: EndpointWitnesses,
    pub(crate) lane_geometry_exhausted: bool,
}

pub(crate) fn invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    validated_lanes: &BTreeSet<(ProceduralCurveId, usize)>,
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    isolate_lanes: bool,
) -> SupportUvValidationResult {
    let (invalid, endpoint_witnesses, lane_geometry_exhausted) = {
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        let mut invalid = Vec::new();
        let mut endpoint_witnesses: EndpointWitnesses = BTreeMap::new();
        let mut lane_geometry_exhausted = false;
        for (procedural_id, points, parameters, fit_tolerance, _) in pending {
            if geometry_budget.exhausted() || support_uv_budget_exhausted(support_budget) {
                break;
            }
            let Some(procedural) = index.procedural_curves(procedural_id.0.as_str()) else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
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
                let parent_geometry_budget = geometry_budget;
                let lane_geometry_budget = isolate_lanes.then(|| {
                    parent_geometry_budget.child_slice(support_uv_lane_geometry_work_limit(
                        points.len(),
                        parent_geometry_budget.remaining(),
                    ))
                });
                let geometry_budget = lane_geometry_budget
                    .as_ref()
                    .unwrap_or(parent_geometry_budget);
                let mut inconsistent = false;
                let mut fully_validated =
                    parameters.len() == points.len() && !parameters.is_empty();
                let mut endpoints = [None, None];
                for (sample_index, (parameter, point)) in parameters.iter().zip(points).enumerate()
                {
                    if geometry_budget.exhausted() || !support_budget.charge() {
                        fully_validated = false;
                        break;
                    }
                    let Some(uv) = pcurve_uv(pcurve, *parameter) else {
                        fully_validated = false;
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
                        fully_validated = false;
                        break;
                    };
                    if sample_index == 0 {
                        endpoints[0] = Some(actual);
                    }
                    if sample_index + 1 == parameters.len() {
                        endpoints[1] = Some(actual);
                    }
                    if point_distance(actual, *point) > tolerance {
                        inconsistent = true;
                        fully_validated = false;
                        break;
                    }
                }
                if let Some(lane_geometry_budget) = &lane_geometry_budget {
                    lane_geometry_exhausted |= lane_geometry_budget.exhausted();
                    let _ = parent_geometry_budget.consume_child(lane_geometry_budget);
                }
                if inconsistent {
                    invalid.push((procedural_id.clone(), side));
                } else if fully_validated {
                    if let [Some(first), Some(last)] = endpoints {
                        endpoint_witnesses
                            .entry((procedural.curve.clone(), surface.clone()))
                            .or_default()
                            .push((pcurve.clone(), context.parameter_range, [first, last]));
                    }
                }
            }
        }
        (invalid, endpoint_witnesses, lane_geometry_exhausted)
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
        procedural.edit_definition(|definition| {
            if let ProceduralCurveDefinition::Intersection { context, .. } = definition {
                context.sides[side].pcurve = None;
            }
        });
    }
    SupportUvValidationResult {
        endpoint_witnesses,
        lane_geometry_exhausted,
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
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
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

// Keep independent work budgets, retry state, and the witness sink explicit at
// this completion boundary.
#[allow(clippy::too_many_arguments)]
fn complete_support_uv_wave(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    coupled_support_budget: &SupportUvBudget<'_>,
    coupled_geometry_budget: &GeometryWorkBudget<'_>,
    failed_attempts: &mut BTreeMap<(ProceduralCurveId, usize), Option<PcurveGeometry>>,
    failed_coupled_attempts: &mut BTreeMap<ProceduralCurveId, [Option<PcurveGeometry>; 2]>,
    endpoint_witnesses: &mut EndpointWitnesses,
) -> bool {
    let mut lane_geometry_exhausted = false;
    if !support_uv_budget_exhausted(support_budget) && !geometry_budget.exhausted() {
        let mut replacements = Vec::new();
        let mut blend_parameter_grids = BTreeMap::<SurfaceId, Option<Vec<(Point2, Point3)>>>::new();
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        for (procedural_id, points, parameters, fit_tolerance, serialized) in pending {
            if support_uv_budget_exhausted(support_budget) {
                break;
            }
            let Some(procedural) = model_index.procedural_curves(procedural_id.0.as_str()) else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
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
                let linear_offset_surface = match &surface.geometry {
                    SurfaceGeometry::Procedural { construction } => model_index
                        .procedural_surfaces(construction.0.as_str())
                        .filter(|procedural| &procedural.surface == surface_id)
                        .is_some_and(|procedural| {
                            matches!(
                                procedural.definition(),
                                ProceduralSurfaceDefinition::Offset {
                                    support_extension: Some(OffsetSupportExtension::Linear),
                                    ..
                                }
                            )
                        }),
                    _ => false,
                };
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
                        serialized,
                        side,
                        point_index,
                    );
                    let continuation_seed = uv.last().copied();
                    let retained_pcurve_seed =
                        pcurve_control_point_seed(context.sides[side].pcurve.as_ref(), point_index);
                    let seed_candidates = ordered_support_uv_seed_candidates(
                        serialized_seeds,
                        retained_pcurve_seed,
                        continuation_seed,
                        linear_offset_surface,
                    )
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
                                if let Some(seed) = seed {
                                    nurbs_surface_parameter_within_tolerance_with_budget(
                                        nurbs,
                                        *point,
                                        Some(seed),
                                        effective_fit_tolerance,
                                        geometry_budget,
                                    )
                                    .map(|parameters| (parameters, true))
                                } else {
                                    unseeded_nurbs_surface_parameters_with_index_and_budget(
                                        &model_index,
                                        surface_id,
                                        &surface.geometry,
                                        nurbs,
                                        *point,
                                        effective_fit_tolerance,
                                        geometry_budget,
                                    )
                                    .map(|parameters| (parameters, true))
                                }
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
                let mut endpoint_values = [None, None];
                let reproduces_chart = if all_parameters_certified {
                    true
                } else {
                    let mut reproduces = true;
                    for (sample_index, (sample_uv, point)) in uv.iter().zip(points).enumerate() {
                        let Some(actual) = decoded_surface_point_with_geometry_and_budget(
                            &model_index,
                            surface_id,
                            &surface.geometry,
                            sample_uv.u,
                            sample_uv.v,
                            0,
                            geometry_budget,
                        ) else {
                            reproduces = false;
                            break;
                        };
                        if sample_index == 0 {
                            endpoint_values[0] = Some(actual);
                        }
                        if sample_index + 1 == points.len() {
                            endpoint_values[1] = Some(actual);
                        }
                        if point_distance(actual, *point) > effective_fit_tolerance {
                            reproduces = false;
                            break;
                        }
                    }
                    reproduces
                };
                if reproduces_chart {
                    if all_parameters_certified {
                        endpoint_values = [
                            uv.first().and_then(|sample_uv| {
                                decoded_surface_point_with_geometry_and_budget(
                                    &model_index,
                                    surface_id,
                                    &surface.geometry,
                                    sample_uv.u,
                                    sample_uv.v,
                                    0,
                                    geometry_budget,
                                )
                            }),
                            uv.last().and_then(|sample_uv| {
                                decoded_surface_point_with_geometry_and_budget(
                                    &model_index,
                                    surface_id,
                                    &surface.geometry,
                                    sample_uv.u,
                                    sample_uv.v,
                                    0,
                                    geometry_budget,
                                )
                            }),
                        ];
                    }
                    let parameter_range = [
                        parameters[0],
                        *parameters.last().expect("non-empty chart parameters"),
                    ];
                    let pcurve = PcurveGeometry::Nurbs {
                        degree: 1,
                        knots: linear_knots(parameters),
                        control_points: uv,
                        weights: None,
                        periodic: false,
                    };
                    if let [Some(first), Some(last)] = endpoint_values {
                        endpoint_witnesses
                            .entry((procedural.curve.clone(), surface_id.clone()))
                            .or_default()
                            .push((pcurve.clone(), parameter_range, [first, last]));
                    }
                    replacements.push((
                        procedural_id.clone(),
                        side,
                        pcurve,
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
            let completed = procedural.edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    return false;
                };
                if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
                    context.sides[side].pcurve = Some(pcurve);
                    true
                } else {
                    false
                }
            });
            if completed && cache_backed_curves.contains(&procedural.curve) {
                procedural.raise_cache_fit_tolerance(effective_fit_tolerance);
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
            failed_coupled_attempts,
            endpoint_witnesses,
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
                .and_then(|procedural| procedural.cache_fit_tolerance())
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
    failed_attempts: &mut BTreeMap<ProceduralCurveId, [Option<PcurveGeometry>; 2]>,
    endpoint_witnesses: &mut EndpointWitnesses,
) -> bool {
    if geometry_budget.exhausted() {
        return false;
    }
    let mut lane_geometry_exhausted = false;
    let mut replacements = Vec::new();
    let mut blend_parameter_grids = BTreeMap::<SurfaceId, Option<Vec<(Point2, Point3)>>>::new();
    let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    for (procedural_id, points, parameters, fit_tolerance, serialized) in pending {
        let Some(procedural) = model_index.procedural_curves(procedural_id.0.as_str()) else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let lane_state = context.sides.each_ref().map(|side| side.pcurve.clone());
        if failed_attempts
            .get(procedural_id)
            .is_some_and(|previous| previous == &lane_state)
        {
            continue;
        }
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
        let seeds = std::array::from_fn(|side| {
            let support = &context.sides[side];
            pcurve_control_point_seed(support.pcurve.as_ref(), 0).or_else(|| {
                model_index
                    .surfaces(surfaces[side].0.as_str())
                    .and_then(|surface| {
                        serialized_support_uv_seed_for_side(&surface.geometry, serialized, side)
                    })
            })
        });
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
        let Some(lanes) = lanes else {
            failed_attempts.insert(procedural_id.clone(), lane_state);
            lane_geometry_exhausted |= lane_geometry_budget.exhausted();
            let _ = parent_geometry_budget.consume_child(&lane_geometry_budget);
            continue;
        };
        for side in 0..2 {
            if missing[side] {
                let endpoint_values =
                    model_index
                        .surfaces(surfaces[side].0.as_str())
                        .map(|surface| {
                            [
                                lanes[side].first().and_then(|parameters| {
                                    decoded_surface_point_with_geometry_and_budget(
                                        &model_index,
                                        surfaces[side],
                                        &surface.geometry,
                                        parameters.u,
                                        parameters.v,
                                        0,
                                        geometry_budget,
                                    )
                                }),
                                lanes[side].last().and_then(|parameters| {
                                    decoded_surface_point_with_geometry_and_budget(
                                        &model_index,
                                        surfaces[side],
                                        &surface.geometry,
                                        parameters.u,
                                        parameters.v,
                                        0,
                                        geometry_budget,
                                    )
                                }),
                            ]
                        });
                let parameter_range = [
                    parameters[0],
                    *parameters.last().expect("non-empty chart parameters"),
                ];
                let pcurve = PcurveGeometry::Nurbs {
                    degree: 1,
                    knots: linear_knots(parameters),
                    control_points: lanes[side].clone(),
                    weights: None,
                    periodic: false,
                };
                if let Some([Some(first), Some(last)]) = endpoint_values {
                    endpoint_witnesses
                        .entry((procedural.curve.clone(), surfaces[side].clone()))
                        .or_default()
                        .push((pcurve.clone(), parameter_range, [first, last]));
                }
                replacements.push((procedural_id.clone(), side, pcurve));
            }
        }
        lane_geometry_exhausted |= lane_geometry_budget.exhausted();
        let _ = parent_geometry_budget.consume_child(&lane_geometry_budget);
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
        procedural.edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                return;
            };
            if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
                context.sides[side].pcurve = Some(pcurve);
            }
        });
    }
    lane_geometry_exhausted
}

#[cfg(test)]
pub(super) fn complete_coupled_support_uv_for_test(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) {
    complete_coupled_support_uv_with_geometry_budget_for_test(
        ir,
        pending,
        super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );
}

#[cfg(test)]
pub(super) fn complete_coupled_support_uv_with_geometry_budget_for_test(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    geometry_work: usize,
) {
    let coupled_support_budget = new_support_uv_budget();
    let geometry_budget = GeometryWorkBudget::new(geometry_work);
    let mut failed_attempts = BTreeMap::new();
    complete_coupled_support_uv(
        ir,
        pending,
        &coupled_support_budget,
        &geometry_budget,
        &mut failed_attempts,
        &mut BTreeMap::new(),
    );
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
                    procedural.definition()
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
        ir.model.procedural_curves[procedural_index].edit_definition(|definition| {
            if let ProceduralCurveDefinition::Intersection { context, .. } = definition {
                if pcurve_requires_completion(context.sides[side].pcurve.as_ref()) {
                    context.sides[side].pcurve = Some(pcurve);
                }
            }
        });
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
                support_extension: first_support_extension,
                extension_flags: first_extensions,
                ..
            }),
            Some(ProceduralSurfaceDefinition::Offset {
                support: second_support,
                distance: second_distance,
                u_sense: second_u_sense,
                v_sense: second_v_sense,
                support_extension: second_support_extension,
                extension_flags: second_extensions,
                ..
            }),
        ) = (
            index
                .procedural_surface_for_carrier(first.0.as_str())
                .map(|surface| surface.definition()),
            index
                .procedural_surface_for_carrier(second.0.as_str())
                .map(|surface| surface.definition()),
        )
        else {
            return false;
        };
        first_distance.to_bits() == second_distance.to_bits()
            && first_u_sense == second_u_sense
            && first_v_sense == second_v_sense
            && first_support_extension == second_support_extension
            && first_extensions == second_extensions
            && equivalent(index, first_support, second_support, visited)
    }

    equivalent(index, first, second, &mut BTreeSet::new())
}

/// One stream's ownership and provenance context for deferred intersection-chart
/// attachment.
pub(crate) struct IntersectionCompletionSource<'a> {
    pub(crate) prefix: String,
    pub(crate) graph: &'a Graph,
    pub(crate) source_stream: StreamHandle,
    pub(crate) coedge_start: usize,
    pub(crate) procedural_start: usize,
}

fn stream_owns_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|suffix| suffix.starts_with(':'))
}

/// Attach charts for one stream without rescanning coedges emitted by an earlier
/// phase.
#[allow(clippy::too_many_arguments)]
pub(crate) fn attach_completed_intersection_pcurves_for_stream_with_budget(
    ir: &mut CadIr,
    graph: &Graph,
    prefix: &str,
    coedge_start: usize,
    procedural_start: usize,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
    validated_endpoint_witnesses: &EndpointWitnesses,
    geometry_budget: &GeometryWorkBudget<'_>,
) {
    let source = IntersectionCompletionSource {
        prefix: prefix.to_owned(),
        graph,
        source_stream,
        coedge_start,
        procedural_start,
    };
    attach_completed_intersection_pcurves_for_sources_with_budget(
        ir,
        std::slice::from_ref(&source),
        annotations,
        validated_endpoint_witnesses,
        geometry_budget,
    );
}

/// Re-run chart attachment over the complete model after all stream-owned
/// topology and intersection contexts exist.
pub(crate) fn attach_completed_intersection_pcurves_for_model_with_budget(
    ir: &mut CadIr,
    sources: &[IntersectionCompletionSource<'_>],
    annotations: &mut AnnotationBuilder,
    validated_endpoint_witnesses: &EndpointWitnesses,
    geometry_budget: &GeometryWorkBudget<'_>,
) {
    attach_completed_intersection_pcurves_for_sources_with_budget(
        ir,
        sources,
        annotations,
        validated_endpoint_witnesses,
        geometry_budget,
    );
}

fn attach_completed_intersection_pcurves_for_sources_with_budget(
    ir: &mut CadIr,
    sources: &[IntersectionCompletionSource<'_>],
    annotations: &mut AnnotationBuilder,
    validated_endpoint_witnesses: &EndpointWitnesses,
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
        .enumerate()
        .filter_map(|(index, coedge)| {
            if !coedge.pcurves.is_empty() {
                return None;
            }
            let source_index = sources.iter().position(|source| {
                index >= source.coedge_start && stream_owns_id(&coedge.id.0, &source.prefix)
            })?;
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
                source_index,
            ))
        })
        .collect::<Vec<_>>();
    if coedge_candidates.is_empty() {
        return;
    }
    let required_keys = coedge_candidates
        .iter()
        .map(|(_, _, curve, surface, _, _)| (curve.clone(), surface.clone()))
        .collect::<BTreeSet<_>>();
    let mut candidates =
        BTreeMap::<(CurveId, SurfaceId), Vec<(PcurveGeometry, [f64; 2], Option<f64>)>>::new();
    let procedural_start = sources
        .iter()
        .map(|source| source.procedural_start)
        .min()
        .unwrap_or(0);
    let multiple_sources = sources.len() > 1;
    for (index, procedural) in ir
        .model
        .procedural_curves
        .iter()
        .enumerate()
        .skip(procedural_start)
    {
        if multiple_sources
            && !sources.iter().any(|source| {
                index >= source.procedural_start && stream_owns_id(&procedural.id.0, &source.prefix)
            })
        {
            continue;
        }
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
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
                procedural.cache_fit_tolerance(),
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
        // edge-incidence condition. Reuse a prior sample-wise proof; evaluate
        // the face surface only for keys without that proof.
        let endpoint_admissible_keys = coedge_candidates
            .iter()
            .filter_map(|(_, edge_id, curve, surface, edge_tolerance, _)| {
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
        let mut witnessed_keys = BTreeSet::new();
        let mut candidate_endpoints = candidates
            .iter()
            .filter(|(key, _)| endpoint_admissible_keys.contains(key))
            .filter_map(|(key, values)| {
                let [candidate] = values.as_slice() else {
                    return None;
                };
                let witness = endpoint_witness_for_candidate(
                    validated_endpoint_witnesses,
                    key,
                    &candidate.0,
                    candidate.1,
                );
                if witness.is_some() {
                    witnessed_keys.insert(key.clone());
                }
                let endpoints = witness.or_else(|| {
                    pcurve_surface_endpoints_with_index_and_budget(
                        &model_index,
                        &key.1,
                        &candidate.0,
                        None,
                        geometry_budget,
                    )
                });
                Some((key.clone(), endpoints))
            })
            .collect::<BTreeMap<_, _>>();
        let replacements = coedge_candidates
            .into_iter()
            .filter_map(
                |(coedge_id, edge_id, curve, surface, edge_tolerance, source_index)| {
                    let key = (curve.clone(), surface.clone());
                    let [candidate] = candidates.get(&key)?.as_slice() else {
                        return None;
                    };
                    let (edge_endpoints, edge_allowance) =
                        edge_endpoint_contracts.get(&edge_id).copied()?;
                    let fit_tolerance = candidate.2.or(edge_tolerance);
                    let matches = {
                        let coincident_surface = candidate_endpoints.get(&key)?.as_ref()?;
                        pcurve_matches_edge_endpoint_contract(
                            *coincident_surface,
                            edge_endpoints,
                            edge_allowance,
                            fit_tolerance,
                        )
                    };
                    if !matches {
                        if !witnessed_keys.remove(&key) {
                            return None;
                        }
                        // A witness from another geometry phase is a shortcut,
                        // not a new admission rule. Preserve the established
                        // endpoint evaluator whenever the downstream contract
                        // disagrees with the cached proof.
                        let fallback = pcurve_surface_endpoints_with_index_and_budget(
                            &model_index,
                            &key.1,
                            &candidate.0,
                            None,
                            geometry_budget,
                        );
                        candidate_endpoints.insert(key.clone(), fallback);
                        let coincident_surface = candidate_endpoints.get(&key)?.as_ref()?;
                        if !pcurve_matches_edge_endpoint_contract(
                            *coincident_surface,
                            edge_endpoints,
                            edge_allowance,
                            fit_tolerance,
                        ) {
                            return None;
                        }
                    }
                    Some((
                        coedge_id,
                        source_index,
                        (candidate.0.clone(), candidate.1, fit_tolerance),
                    ))
                },
            )
            .collect::<Vec<_>>();
        replacements
    };
    for (coedge_id, source_index, (geometry, parameter_range, fit_tolerance)) in replacements {
        let source = &sources[source_index];
        let Some(fin_xmt) = coedge_id
            .0
            .rsplit_once('#')
            .and_then(|(_, value)| value.parse::<u32>().ok())
        else {
            continue;
        };
        let pcurve_id = PcurveId(format!(
            "{}:intersection-pcurve-completed#{fin_xmt}",
            source.prefix
        ));
        if ir.model.pcurves.iter().any(|pcurve| pcurve.id == pcurve_id) {
            continue;
        }
        let source_offset = source
            .graph
            .get(17, fin_xmt)
            .map_or(0, |node| node.pos as u64);
        annotations
            .note(&pcurve_id, source.source_stream, source_offset)
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
    fn linear_offset_continuation_precedes_unvalidated_serialized_seeds() {
        let serialized = [
            Some(Point2::new(1.0, 1.0)),
            Some(Point2::new(2.0, 2.0)),
            Some(Point2::new(3.0, 3.0)),
            Some(Point2::new(4.0, 4.0)),
        ];
        let retained = Some(Point2::new(5.0, 5.0));
        let continuation = Some(Point2::new(6.0, 6.0));

        assert_eq!(
            ordered_support_uv_seed_candidates(serialized, retained, continuation, true),
            [
                continuation,
                serialized[0],
                serialized[1],
                serialized[2],
                serialized[3],
                retained,
                None,
            ]
        );
        assert_eq!(
            ordered_support_uv_seed_candidates(serialized, retained, continuation, false),
            [
                serialized[0],
                serialized[1],
                serialized[2],
                serialized[3],
                retained,
                continuation,
                None,
            ]
        );
    }

    #[test]
    fn oversized_serialized_lane_is_declined_before_geometry_work() {
        let surface_id = SurfaceId("synthetic:support-plane".into());
        let mut ir = CadIr::empty();
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

    #[test]
    fn unseeded_nurbs_completion_accepts_only_a_tolerance_certified_coarse_fit() {
        const FIT_TOLERANCE: f64 = 1.0e-10;
        const GEOMETRY_WORK: usize = 1_024;

        let surface_id = SurfaceId("synthetic:coarse-nurbs-support".into());
        let nurbs = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            weights: None,
            normal_reversed: false,
            u_periodic: false,
            v_periodic: false,
        };
        let geometry = SurfaceGeometry::Nurbs(nurbs.clone());
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: surface_id.clone(),
            geometry: geometry.clone(),
            source_object: None,
        });
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);

        let fit_budget = GeometryWorkBudget::new(GEOMETRY_WORK);
        let parameters = unseeded_nurbs_surface_parameters_with_index_and_budget(
            &index,
            &surface_id,
            &geometry,
            &nurbs,
            Point3::new(0.5, 0.5, 0.0),
            FIT_TOLERANCE,
            &fit_budget,
        )
        .expect("coarse grid contains the exact chart point");
        assert_eq!(parameters, Point2::new(0.5, 0.5));

        let miss_budget = GeometryWorkBudget::new(GEOMETRY_WORK);
        assert!(unseeded_nurbs_surface_parameters_with_index_and_budget(
            &index,
            &surface_id,
            &geometry,
            &nurbs,
            Point3::new(0.5, 0.5, 1.0),
            FIT_TOLERANCE,
            &miss_budget,
        )
        .is_none());
    }
}
