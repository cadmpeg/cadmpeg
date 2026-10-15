// SPDX-License-Identifier: Apache-2.0
//! EXT11 support-UV assignment, completion, and equivalent-parameter transfer.

use super::blend::{
    blend_boundary_parameter_from_contact_pcurve, blend_support_parameter_from_source_pcurve,
    blend_surface_definition_with_index, blend_surface_parameter_grid_with_index_and_budget,
    blend_surface_parameters_for_fit_with_grid_and_budget,
    blend_surface_parameters_for_fit_with_source_continuation_and_budget,
    blend_surface_parameters_from_grid_for_fit_and_budget,
    blend_surface_parameters_from_grid_for_fit_with_source_continuation_and_budget,
    blend_surface_parameters_from_point_with_index_and_budget,
    decoded_surface_point_with_geometry_and_budget, spine_contact_pcurve_with_index,
    BlendContactSeedCache, BlendParameterGrid, BoundaryInverseTarget, CircularBlendDefinition,
    ContactCurveSample, SourcePcurveSample,
};
use super::geometry_work::GeometryWorkBudget;
use super::nurbs_fit::{fit_nurbs_surface_parameter, fit_nurbs_surface_parameter_locally};
use super::offset::{
    coarse_model_surface_parameters,
    continue_surface_intersection_parameters_with_index_and_seeds_and_budget_and_grid_cache,
    offset_surface_parameters_with_tolerance_with_index_and_budget,
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
use crate::framing::node_kind::NodeKind;
use crate::framing::xmt_reference::NonNullXmt;
use crate::intersection::{SupportUv, SupportUvLane};
use crate::topology::Graph;
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{analytic_surface_parameters, finite_or_refusal};
use cadmpeg_ir::geometry::{
    pcurve::{Pcurve, PcurveGeometry},
    ProceduralCurveDefinition, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, PcurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::units::FinitePoint2;
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
const MAX_SUPPORT_UV_COMPLETION_SAMPLES: usize = 65_536;

/// Geometry work reserved for one support-UV sample before the lane slice is
/// capped. A lane is admitted as a whole, so one difficult carrier cannot
/// consume the model-wide budget before later carriers get a certified try.
const SUPPORT_UV_GEOMETRY_WORK_PER_SAMPLE: usize = 2_048;

/// Minimum geometry work available to a support-UV lane with a small chart.
const MIN_SUPPORT_UV_LANE_GEOMETRY_WORK: usize = 131_072;

/// Maximum geometry work available to one support-UV lane in one strategy.
const MAX_SUPPORT_UV_LANE_GEOMETRY_WORK: usize = 262_144;

pub(super) fn support_uv_completion_budget_limit(chart_count: usize) -> usize {
    chart_count
        .checked_mul(SUPPORT_UV_COMPLETION_SAMPLES_PER_CHART)
        .map_or(MAX_SUPPORT_UV_COMPLETION_SAMPLES, |samples| {
            samples.clamp(
                MIN_SUPPORT_UV_COMPLETION_SAMPLES,
                MAX_SUPPORT_UV_COMPLETION_SAMPLES,
            )
        })
}

type SupportUvBudget<'a> = WorkBudget;

pub(super) fn support_uv_budget_exhausted(budget: &SupportUvBudget<'_>) -> bool {
    budget.exhausted() || budget.remaining() == 0
}

fn support_uv_lane_geometry_work_limit(sample_count: usize, remaining: usize) -> usize {
    sample_count
        .checked_mul(SUPPORT_UV_GEOMETRY_WORK_PER_SAMPLE)
        .map_or(MAX_SUPPORT_UV_LANE_GEOMETRY_WORK, |work| {
            work.clamp(
                MIN_SUPPORT_UV_LANE_GEOMETRY_WORK,
                MAX_SUPPORT_UV_LANE_GEOMETRY_WORK,
            )
        })
        .min(remaining)
}

#[cfg(test)]
fn new_support_uv_budget() -> SupportUvBudget<'static> {
    WorkBudget::new(MAX_SUPPORT_UV_SAMPLES)
}

pub(super) fn linear_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::new();
    knots.extend(parameters.first().copied());
    knots.extend_from_slice(parameters);
    knots.extend(parameters.last().copied());
    knots
}

fn linear_pcurve_geometry(
    parameters: &[f64],
    controls: &[Point2],
) -> Result<PcurveGeometry, cadmpeg_core::CodecError> {
    let mut points = Vec::new();
    for control in controls {
        points.push(FinitePoint2::new(*control).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("control_points contains a non-finite point")
        })?);
    }
    let knots = cadmpeg_ir::geometry::nurbs::KnotVector::new(linear_knots(parameters))?;
    let nurbs = cadmpeg_ir::geometry::pcurve::PcurveNurbs::new(
        1,
        knots,
        cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Polynomial { points },
        false,
    )?;
    Ok(PcurveGeometry::Nurbs { nurbs })
}

// Keep the object-map, serialized lanes, and shared geometry budget explicit:
// this function decides which native lane can be admitted to which support.

pub(super) struct SerializedSupportUvFit<'inputs> {
    pub(super) surfaces_by_xmt: &'inputs BTreeMap<u32, SurfaceId>,
    pub(super) supports: [Option<NonNullXmt>; 2],
    pub(super) points: &'inputs [Point3],
    pub(super) fit_tolerance: f64,
    pub(super) lanes: &'inputs SupportUv,
}

pub(super) fn assign_ext11_support_uv_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    serialized_support_uv: &SerializedSupportUvFit<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<SupportUv>, cadmpeg_core::CodecError> {
    let &SerializedSupportUvFit {
        surfaces_by_xmt,
        supports,
        points,
        fit_tolerance,
        lanes,
    } = serialized_support_uv;

    let [first, second] = supports.map(|support| {
        support
            .and_then(|support| surfaces_by_xmt.get(&u32::from(support)))
            .cloned()
    });
    let surface_ids = [first, second];
    let [Some(first_surface), Some(second_surface)] = surface_ids else {
        return Ok(None);
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
pub(super) fn validate_serialized_support_uv_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    serialized_support_uv: &SerializedSupportUvFit<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<SupportUv, cadmpeg_core::CodecError> {
    let &SerializedSupportUvFit {
        surfaces_by_xmt,
        supports,
        points,
        fit_tolerance,
        lanes,
    } = serialized_support_uv;

    let mut admitted = [None, None];
    for side in 0..2 {
        let Some(surface) =
            supports[side].and_then(|support| surfaces_by_xmt.get(&u32::from(support)))
        else {
            continue;
        };
        let Some(values) = lanes[side].as_ref() else {
            continue;
        };
        let tolerance = blend_spine_cache_fit_tolerance_with_index(index, surface, fit_tolerance);
        if support_uv_lane_matches_surface_with_budget(
            index,
            surface,
            points,
            tolerance,
            Some(values),
            geometry_budget,
        )? {
            admitted[side] = Some(
                crate::intersection::SupportUvLane::from_checked(
                    values.as_slice().to_vec(),
                    values.as_slice().len(),
                )
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("NX copied support-UV lane count")
                })?,
            );
        }
    }
    Ok(admitted)
}

fn support_uv_lane_matches_surface_with_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    points: &[Point3],
    fit_tolerance: f64,
    values: Option<&SupportUvLane>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    let Some(values) = values else {
        return Ok(false);
    };
    if values.len() > MAX_SUPPORT_UV_SAMPLES {
        return Ok(false);
    }
    let Some(geometry) = index
        .surfaces(surface.as_str(), cadmpeg_ir::index::StandardIndex)
        .map(|surface| &surface.geometry)
    else {
        return Ok(false);
    };
    for (uv, point) in values.iter().zip(points) {
        if geometry_budget.exhausted() {
            return Ok(false);
        }
        if uv.iter().any(|value| missing_support_parameter(*value)) {
            return Ok(false);
        }
        let Some(uv) = surface_parameters(geometry, **uv) else {
            return Ok(false);
        };
        let Some(candidate) = decoded_surface_point_with_geometry_and_budget(
            index,
            surface,
            geometry,
            uv.u,
            uv.v,
            0,
            geometry_budget,
        )?
        else {
            return Ok(false);
        };
        if Point3::distance(candidate, *point) > fit_tolerance {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
pub(super) fn assign_ext11_support_uv_to_surfaces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &SupportUv,
) -> Option<SupportUv> {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let geometry_budget =
        GeometryWorkBudget::from_context(ctx, super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);

    assign_ext11_support_uv_to_surfaces_with_index(
        &index,
        surfaces,
        points,
        fit_tolerance,
        lanes,
        &geometry_budget,
    )
    .expect("evaluator allocation succeeds")
}

fn assign_ext11_support_uv_to_surfaces_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    lanes: &SupportUv,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<SupportUv>, cadmpeg_core::CodecError> {
    let lane_matches_surface = |surface: &SurfaceId, lane: usize| {
        support_uv_lane_matches_surface_with_budget(
            index,
            surface,
            points,
            fit_tolerance,
            lanes[lane].as_ref(),
            geometry_budget,
        )
    };
    let matches = [
        [
            lane_matches_surface(surfaces[0], 0)?,
            lane_matches_surface(surfaces[0], 1)?,
        ],
        [
            lane_matches_surface(surfaces[1], 0)?,
            lane_matches_surface(surfaces[1], 1)?,
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
            return Ok(None);
        }
        assigned[support] = lanes[lane]
            .as_ref()
            .map(|lane| {
                crate::intersection::SupportUvLane::from_checked(
                    lane.as_slice().to_vec(),
                    lane.as_slice().len(),
                )
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("NX copied support-UV lane count")
                })
            })
            .transpose()?;
        assigned_lanes[support] = Some(lane);
    }
    if surfaces[0] != surfaces[1] && assigned.iter().filter(|lane| lane.is_some()).count() == 1 {
        let Some(assigned_support) = assigned.iter().position(Option::is_some) else {
            return Ok(None);
        };
        let Some(assigned_lane) = assigned_lanes[assigned_support] else {
            return Ok(None);
        };
        let other_support = 1 - assigned_support;
        let other_lane = 1 - assigned_lane;
        if lane_matches_surface(surfaces[other_support], other_lane)? {
            assigned[other_support] = lanes[other_lane]
                .as_ref()
                .map(|lane| {
                    crate::intersection::SupportUvLane::from_checked(
                        lane.as_slice().to_vec(),
                        lane.as_slice().len(),
                    )
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("NX copied support-UV lane count")
                    })
                })
                .transpose()?;
        }
    }
    Ok(assigned.iter().any(Option::is_some).then_some(assigned))
}

/// Serialized support-UV lanes retained with one charted intersection.
///
/// The values-array lanes and EXT11 chart lanes have different admission
/// rules. Values-array lanes are ordered by support side; EXT11 lanes remain
/// in their serialized order until their surface identity is proven. Both
/// sources are useful as seeds for coupled completion, but only EXT11 lanes
/// participate in EXT11 assignment.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SerializedSupportUv {
    pub(super) values: SupportUv,
    pub(super) ext11: SupportUv,
}

#[cfg(test)]
impl SerializedSupportUv {
    pub(super) fn from_values(values: [Option<Vec<[f64; 2]>>; 2]) -> Self {
        let values = values.map(|lane| {
            lane.and_then(|values| {
                let count = values.len();
                SupportUvLane::new(values, count)
            })
        });
        Self {
            values,
            ext11: [None, None],
        }
    }

    pub(super) fn from_ext11(ext11: [Option<Vec<[f64; 2]>>; 2]) -> Self {
        let ext11 = ext11.map(|lane| {
            lane.and_then(|values| {
                let count = values.len();
                SupportUvLane::new(values, count)
            })
        });
        Self {
            values: [None, None],
            ext11,
        }
    }
}

type PendingExt11SupportUv = (
    ProceduralCurveId,
    crate::intersection::chart_samples::ChartSamples,
    f64,
    SerializedSupportUv,
);

/// Return endpoint witnesses already certified by serialized support-UV lanes.
/// The lane samples and the intersection parameter range use the same ordered
/// parameter domain, so its first and last model-space samples are the
/// pcurve's endpoint witnesses.
pub(super) fn validated_support_uv_endpoint_witnesses(
    ir: &CadIr,
    pending: &[PendingExt11SupportUv],
    validated_lanes: &BTreeSet<(ProceduralCurveId, usize)>,
) -> EndpointWitnesses {
    let mut procedural_by_id = BTreeMap::new();
    for procedural in &ir.model.procedural_curves {
        procedural_by_id.insert(&procedural.id, procedural);
    }
    let mut witnesses: EndpointWitnesses = BTreeMap::new();
    for (procedural_id, samples, _, _) in pending {
        let Some(procedural) = procedural_by_id.get(procedural_id).copied() else {
            continue;
        };
        let Some(owner) = ir.model.procedural_curve_owner(&procedural.id) else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let expected_range = samples.parameter_range();
        if context.parameter_range().endpoints() != expected_range {
            continue;
        }
        for (side, support) in context.sides().iter().enumerate() {
            if !validated_lanes.contains(&(procedural_id.clone(), side))
                || pcurve_requires_completion(
                    support.pcurve.as_ref().map(|pcurve| &pcurve.geometry),
                )
            {
                continue;
            }
            let Some(surface) = &support.surface else {
                continue;
            };
            let Some(pcurve) = &support.pcurve else {
                continue;
            };
            let key = (owner.clone(), surface.clone());

            let entries = match witnesses.entry(key) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
            };

            entries.push((
                pcurve.geometry.clone(),
                context.parameter_range().endpoints(),
                samples.endpoints(),
            ));
        }
    }
    witnesses
}

pub(super) fn missing_support_parameter(value: f64) -> bool {
    value.to_bits() == MISSING_TOLERANCE.to_bits()
}

pub(super) fn pcurve_requires_completion(pcurve: Option<&PcurveGeometry>) -> bool {
    match pcurve {
        None => true,
        Some(PcurveGeometry::Nurbs { nurbs }) => nurbs
            .control_points()
            .iter()
            .any(|point| missing_support_parameter(point.u) || missing_support_parameter(point.v)),
        Some(_) => false,
    }
}

fn pcurve_control_point_seed(pcurve: Option<&PcurveGeometry>, index: usize) -> Option<Point2> {
    let PcurveGeometry::Nurbs { nurbs } = pcurve? else {
        return None;
    };
    nurbs
        .control_points()
        .get(index)
        .map(|point| point.get())
        .filter(|point| !missing_support_parameter(point.u) && !missing_support_parameter(point.v))
}

fn serialized_support_uv_seed_candidates(
    geometry: &SurfaceGeometry,
    serialized: &SerializedSupportUv,
    side: usize,
    point_index: usize,
) -> [Option<Point2>; 4] {
    let lane_order = [side, 1 - side];
    let mut candidates = [None; 4];
    for (candidate, output) in candidates.iter_mut().enumerate() {
        let lanes = if candidate < 2 {
            &serialized.values
        } else {
            &serialized.ext11
        };
        let lane = lane_order[candidate % 2];
        let Some(uv) = lanes[lane]
            .as_deref()
            .and_then(|lane| lane.get(point_index))
        else {
            continue;
        };
        let [u, v] = **uv;
        if !missing_support_parameter(u) && !missing_support_parameter(v) {
            *output = surface_parameters(geometry, [u, v]).map(FinitePoint2::get);
        }
    }
    candidates
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
    nurbs: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    point: Point3,
    fit_tolerance: f64,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let coarse = if let Some(domain) =
        surface_parameter_domain_with_index(index, surface_id, geometry_budget.ctx)?
    {
        coarse_model_surface_parameters(index, surface_id, point, domain, geometry_budget)?
    } else {
        None
    };
    if let Some(parameters) = coarse {
        if decoded_surface_point_with_geometry_and_budget(
            index,
            surface_id,
            surface,
            parameters.u,
            parameters.v,
            0,
            geometry_budget,
        )?
        .is_some_and(|candidate| Point3::distance(candidate, point) <= fit_tolerance)
        {
            return Ok(Some(parameters));
        }
    }
    fit_nurbs_surface_parameter(
        geometry_budget.ctx,
        nurbs,
        point,
        coarse,
        fit_tolerance,
        geometry_budget,
    )
    .map(|parameter| parameter.map(FinitePoint2::get))
}

fn serialized_support_uv_seed_for_side(
    geometry: &SurfaceGeometry,
    serialized: &SerializedSupportUv,
    side: usize,
) -> Option<Point2> {
    let lane_order = [side, 1 - side];
    for (lanes, lane) in [
        (&serialized.values, lane_order[0]),
        (&serialized.ext11, lane_order[0]),
        (&serialized.ext11, lane_order[1]),
        (&serialized.values, lane_order[1]),
    ] {
        let Some(uv) = lanes[lane].as_deref().and_then(|lane| lane.first()) else {
            continue;
        };
        let [u, v] = **uv;
        if !missing_support_parameter(u) && !missing_support_parameter(v) {
            if let Some(point) = surface_parameters(geometry, [u, v]) {
                return Some(point.get());
            }
        }
    }
    None
}

#[cfg(test)]
pub(super) fn complete_ext11_support_uv(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) -> Result<(), cadmpeg_core::CodecError> {
    let geometry_budget =
        GeometryWorkBudget::from_context(ctx, super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);

    complete_ext11_support_uv_with_budget(ir, pending, &geometry_budget)
}

pub(super) fn complete_ext11_support_uv_with_budget(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    if pending.is_empty() {
        return Ok(());
    }
    let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let mut replacements = Vec::new();
    for (procedural_id, samples, fit_tolerance, serialized) in pending {
        let points = &samples.points();
        let parameters = &samples.parameters();
        let Some(procedural) =
            model_index.procedural_curves(procedural_id.as_str(), cadmpeg_ir::index::StandardIndex)
        else {
            continue;
        };
        let (surfaces, missing) = match procedural.definition() {
            ProceduralCurveDefinition::Intersection { context, .. } => {
                let [Some(first), Some(second)] =
                    context.sides().each_ref().map(|side| side.surface.as_ref())
                else {
                    continue;
                };
                (
                    [first, second],
                    context.sides().each_ref().map(|side| {
                        pcurve_requires_completion(
                            side.pcurve.as_ref().map(|pcurve| &pcurve.geometry),
                        )
                    }),
                )
            }
            _ => continue,
        };
        if !missing.into_iter().any(|missing| missing) {
            continue;
        }
        let Some(assigned) = assign_ext11_support_uv_to_surfaces_with_index(
            &model_index,
            [surfaces[0], surfaces[1]],
            points,
            *fit_tolerance,
            &serialized.ext11,
            geometry_budget,
        )?
        else {
            continue;
        };
        let mut side_lanes: [Option<Vec<Point2>>; 2] = [None, None];
        for side in 0..2 {
            if !missing[side] {
                continue;
            }
            let Some(surface_geometry) = model_index
                .surfaces(surfaces[side].as_str(), cadmpeg_ir::index::StandardIndex)
                .map(|surface| &surface.geometry)
            else {
                continue;
            };
            let Some(values) = assigned[side].as_ref() else {
                continue;
            };
            if values
                .iter()
                .flat_map(|pair| pair.iter())
                .any(|value| missing_support_parameter(*value))
            {
                continue;
            }
            let mut controls = Vec::new();

            let mut valid = true;
            for uv in values.iter() {
                if let Some(point) = surface_parameters(surface_geometry, **uv) {
                    controls.push(point.get());
                } else {
                    valid = false;
                    break;
                }
            }
            if valid {
                side_lanes[side] = Some(controls);
            }
        }
        for (side, control_points) in side_lanes.into_iter().enumerate() {
            let Some(control_points) = control_points else {
                continue;
            };
            let replacement = linear_pcurve_geometry(parameters, &control_points)?;

            replacements.push((procedural_id.clone(), side, replacement));
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
        let Some(context) = procedural.intersection_context_mut() else {
            continue;
        };

        context.set_unmapped_pcurve(side, Some(replacement));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn complete_support_uv(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) -> Result<bool, cadmpeg_core::CodecError> {
    let support_budget = new_support_uv_budget();
    let coupled_support_budget = new_support_uv_budget();
    let geometry_budget =
        GeometryWorkBudget::from_context(ctx, super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    complete_support_uv_with_budget(
        ir,
        pending,
        &support_budget,
        &geometry_budget,
        &coupled_support_budget,
        &geometry_budget,
    )
}

#[cfg(test)]
pub(super) fn complete_support_uv_with_budget(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    coupled_support_budget: &SupportUvBudget<'_>,
    coupled_geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut endpoint_witnesses = BTreeMap::new();
    complete_support_uv_with_budget_and_endpoint_witnesses(
        ir,
        pending,
        (support_budget, geometry_budget),
        (coupled_support_budget, coupled_geometry_budget),
        &mut endpoint_witnesses,
    )
}

pub(super) fn complete_support_uv_with_budget_and_endpoint_witnesses(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    direct_budgets: (&SupportUvBudget<'_>, &GeometryWorkBudget<'_>),
    coupled_budgets: (&SupportUvBudget<'_>, &GeometryWorkBudget<'_>),
    endpoint_witnesses: &mut EndpointWitnesses,
) -> Result<bool, cadmpeg_core::CodecError> {
    let (support_budget, geometry_budget) = direct_budgets;
    let (coupled_support_budget, coupled_geometry_budget) = coupled_budgets;
    // A failed fit can become solvable when either lane is filled by an
    // earlier wave. Keep those dependencies as the direct and coupled retry
    // keys; unrelated progress must not repeat the same inverse problems.
    let mut failed_attempts = BTreeMap::<(ProceduralCurveId, usize), Option<PcurveGeometry>>::new();
    let mut failed_coupled_attempts =
        BTreeMap::<ProceduralCurveId, [Option<cadmpeg_ir::geometry::SupportPcurve>; 2]>::new();
    let mut lane_geometry_exhausted = false;
    loop {
        let before = pending_support_lanes_requiring_completion(ir, pending);
        if before == 0 || support_uv_budget_exhausted(support_budget) {
            break;
        }
        geometry_budget.clear_blend_frame_cache();
        coupled_geometry_budget.clear_blend_frame_cache();
        lane_geometry_exhausted |= complete_support_uv_wave(
            ir,
            SupportUvAttempts {
                pending,
                failed_attempts: &mut failed_attempts,
                failed_coupled_attempts: &mut failed_coupled_attempts,
                endpoint_witnesses,
            },
            support_budget,
            geometry_budget,
            coupled_support_budget,
            coupled_geometry_budget,
        )?;
        let after = pending_support_lanes_requiring_completion(ir, pending);
        if after >= before || support_uv_budget_exhausted(support_budget) {
            break;
        }
    }
    Ok(lane_geometry_exhausted)
}

#[cfg(test)]
pub(super) fn invalidate_inconsistent_support_uv(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) {
    let geometry_budget =
        GeometryWorkBudget::from_context(ctx, super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    let support_budget = WorkBudget::new(MAX_SUPPORT_UV_SAMPLES);

    invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
        ctx,
        ir,
        pending,
        &BTreeSet::new(),
        &support_budget,
        &geometry_budget,
        false,
    )
    .expect("evaluator allocation succeeds");
}

/// Invalidate support lanes that disagree with their surface and retain
/// endpoint witnesses only for lanes whose complete sample set was evaluated.
pub(in crate::decode) struct SupportUvValidationResult {
    pub(super) endpoint_witnesses: EndpointWitnesses,
    pub(super) lane_geometry_exhausted: bool,
}

pub(super) fn invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    validated_lanes: &BTreeSet<(ProceduralCurveId, usize)>,
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    isolate_lanes: bool,
) -> Result<SupportUvValidationResult, cadmpeg_core::CodecError> {
    if pending.is_empty() {
        return Ok(SupportUvValidationResult {
            endpoint_witnesses: BTreeMap::new(),
            lane_geometry_exhausted: false,
        });
    }
    let (invalid, endpoint_witnesses, lane_geometry_exhausted) = {
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        let mut invalid = Vec::new();
        let mut endpoint_witnesses: EndpointWitnesses = BTreeMap::new();
        let mut lane_geometry_exhausted = false;
        for (procedural_id, samples, fit_tolerance, _) in pending {
            let points = &samples.points();
            let parameters = &samples.parameters();
            if geometry_budget.exhausted() {
                break;
            }
            if support_uv_budget_exhausted(support_budget) {
                break;
            }
            let Some(procedural) =
                index.procedural_curves(procedural_id.as_str(), cadmpeg_ir::index::StandardIndex)
            else {
                continue;
            };
            let Some(owner) = index.ir().model.procedural_curve_owner(&procedural.id) else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
            else {
                continue;
            };
            for (side, support) in context.sides().iter().enumerate() {
                if geometry_budget.exhausted() {
                    break;
                }
                if support_uv_budget_exhausted(support_budget) {
                    break;
                }
                if validated_lanes.contains(&(procedural_id.clone(), side)) {
                    continue;
                }
                let (Some(surface), Some(pcurve)) = (&support.surface, &support.pcurve) else {
                    continue;
                };
                let Some(geometry) = index
                    .surfaces(surface.as_str(), cadmpeg_ir::index::StandardIndex)
                    .map(|surface| &surface.geometry)
                else {
                    continue;
                };
                let tolerance =
                    blend_spine_cache_fit_tolerance_with_index(&index, surface, *fit_tolerance);
                let parent_geometry_budget = geometry_budget;
                let lane_geometry_budget = if isolate_lanes {
                    Some(
                        parent_geometry_budget.child_slice(support_uv_lane_geometry_work_limit(
                            points.len(),
                            parent_geometry_budget.remaining(),
                        )),
                    )
                } else {
                    None
                };
                let geometry_budget = lane_geometry_budget
                    .as_ref()
                    .unwrap_or(parent_geometry_budget);
                let mut inconsistent = false;
                let mut fully_validated = true;
                let mut endpoints = [None, None];
                for (sample_index, (parameter, point)) in parameters.iter().zip(points).enumerate()
                {
                    if geometry_budget.exhausted() {
                        fully_validated = false;
                        break;
                    }
                    if !support_budget.charge() {
                        fully_validated = false;
                        break;
                    }
                    let Some(uv) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::pcurve_uv(ctx, &pcurve.geometry, *parameter),
                    )?)?
                    else {
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
                    )?
                    else {
                        fully_validated = false;
                        break;
                    };
                    if sample_index == 0 {
                        endpoints[0] = Some(actual);
                    }
                    if sample_index + 1 == parameters.len() {
                        endpoints[1] = Some(actual);
                    }
                    if Point3::distance(actual, *point) > tolerance {
                        inconsistent = true;
                        fully_validated = false;
                        break;
                    }
                }
                if let Some(lane_geometry_budget) = &lane_geometry_budget {
                    let parent_exhausted = parent_geometry_budget
                        .consume_child(lane_geometry_budget)
                        .is_err();
                    let child_exhausted = lane_geometry_budget.exhausted();
                    lane_geometry_exhausted |= child_exhausted || parent_exhausted;
                }
                if inconsistent {
                    invalid.push((procedural_id.clone(), side));
                } else if fully_validated {
                    if let [Some(first), Some(last)] = endpoints {
                        let key = (owner.clone(), surface.clone());

                        let entries = match endpoint_witnesses.entry(key) {
                            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                entry.insert(Vec::new())
                            }
                        };

                        entries.push((
                            pcurve.geometry.clone(),
                            context.parameter_range().endpoints(),
                            [first, last],
                        ));
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
        let Some(context) = procedural.intersection_context_mut() else {
            continue;
        };

        context.set_unmapped_pcurve(side, None);
    }
    Ok(SupportUvValidationResult {
        endpoint_witnesses,
        lane_geometry_exhausted,
    })
}

fn pending_support_lanes_requiring_completion(
    ir: &CadIr,
    pending: &[PendingExt11SupportUv],
) -> usize {
    if pending.is_empty() {
        return 0;
    }
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    let mut count = 0usize;
    for (procedural_id, ..) in pending {
        let Some(procedural) =
            index.procedural_curves(procedural_id.as_str(), cadmpeg_ir::index::StandardIndex)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let lanes = context
            .sides()
            .iter()
            .filter(|side| {
                pcurve_requires_completion(side.pcurve.as_ref().map(|pcurve| &pcurve.geometry))
            })
            .count();
        count += lanes;
    }
    count
}

// Keep independent work budgets, retry state, and the witness sink explicit at
// this completion boundary.

struct SupportUvAttempts<'inputs> {
    pending: &'inputs [PendingExt11SupportUv],
    failed_attempts: &'inputs mut BTreeMap<(ProceduralCurveId, usize), Option<PcurveGeometry>>,
    failed_coupled_attempts:
        &'inputs mut BTreeMap<ProceduralCurveId, [Option<cadmpeg_ir::geometry::SupportPcurve>; 2]>,
    endpoint_witnesses: &'inputs mut EndpointWitnesses,
}

fn complete_support_uv_wave(
    ir: &mut CadIr,
    support_uv_attempts: SupportUvAttempts<'_>,
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    coupled_support_budget: &SupportUvBudget<'_>,
    coupled_geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let SupportUvAttempts {
        pending,
        failed_attempts,
        failed_coupled_attempts,
        endpoint_witnesses,
    } = support_uv_attempts;

    let mut lane_geometry_exhausted = false;
    let geometry_exhausted = geometry_budget.exhausted();
    if !support_uv_budget_exhausted(support_budget) && !geometry_exhausted {
        let mut replacements = Vec::new();
        let mut blend_parameter_grids = BTreeMap::<SurfaceId, Option<Vec<(Point2, Point3)>>>::new();
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        for (procedural_id, samples, fit_tolerance, serialized) in pending {
            let points = &samples.points();
            let parameters = &samples.parameters();
            if support_uv_budget_exhausted(support_budget) {
                break;
            }
            let Some(procedural) = model_index
                .procedural_curves(procedural_id.as_str(), cadmpeg_ir::index::StandardIndex)
            else {
                continue;
            };
            let Some(owner) = model_index
                .ir()
                .model
                .procedural_curve_owner(&procedural.id)
            else {
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
                if !pcurve_requires_completion(
                    context.sides()[side]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| &pcurve.geometry),
                ) {
                    continue;
                }
                let Some(surface_id) = &context.sides()[side].surface else {
                    continue;
                };
                let attempt_key = (procedural_id.clone(), side);
                let source_pcurve = context.sides()[1 - side].pcurve.as_ref();
                let other_surface_id = context.sides()[1 - side].surface.as_ref();
                if failed_attempts.get(&attempt_key).is_some_and(|previous| {
                    previous.as_ref() == source_pcurve.map(|pcurve| &pcurve.geometry)
                }) {
                    continue;
                }
                let Some(surface) =
                    model_index.surfaces(surface_id.as_str(), cadmpeg_ir::index::StandardIndex)
                else {
                    continue;
                };
                let source_chart_available = source_pcurve
                    .is_some_and(|pcurve| !pcurve_requires_completion(Some(&pcurve.geometry)));
                let linear_offset_surface = match &surface.geometry {
                    SurfaceGeometry::Procedural { construction, .. } => model_index
                        .procedural_surfaces(
                            construction.as_str(),
                            cadmpeg_ir::index::StandardIndex,
                        )
                        .is_some_and(|procedural| match procedural.definition() {
                            ProceduralSurfaceDefinition::Offset(matched_payload) => {
                                matched_payload.linear_support_extension()
                            }
                            _ => false,
                        }),
                    SurfaceGeometry::Solved(_) => false,
                };
                let effective_fit_tolerance = blend_spine_cache_fit_tolerance_with_index(
                    &model_index,
                    surface_id,
                    *fit_tolerance,
                );
                let other_support = match context.sides()[1 - side]
                    .surface
                    .as_ref()
                    .zip(context.sides()[1 - side].pcurve.as_ref())
                {
                    Some((other_surface, other_pcurve)) => model_index
                        .surfaces(other_surface.as_str(), cadmpeg_ir::index::StandardIndex)
                        .map(|surface| (other_surface, &other_pcurve.geometry, &surface.geometry)),
                    None => None,
                };
                let other_contact = (|| -> Result<_, cadmpeg_core::decode::ResourceLimit> {
                    let Some((other_surface, other_pcurve, other_geometry)) = other_support else {
                        return Ok(None);
                    };
                    let Some(CircularBlendDefinition {
                        supports,
                        spine,
                        radius,
                        ..
                    }) = blend_surface_definition_with_index(&model_index, surface_id)
                    else {
                        return Ok(None);
                    };
                    let mut selected = None;
                    for (boundary, candidate) in supports.into_iter().enumerate() {
                        if parameterization_equivalent_surfaces_with_index(
                            &model_index,
                            candidate,
                            other_surface,
                        )? && selected.replace(boundary).is_some()
                        {
                            return Ok(None);
                        }
                    }
                    let Some(boundary) = selected else {
                        return Ok(None);
                    };
                    let Some(contact_pcurve) = spine_contact_pcurve_with_index(
                        &model_index,
                        other_surface,
                        spine,
                        radius,
                        0,
                        geometry_budget.ctx,
                    )?
                    else {
                        return Ok(None);
                    };
                    Ok(Some((
                        other_surface,
                        other_pcurve,
                        other_geometry,
                        contact_pcurve,
                        boundary,
                    )))
                })()?;
                let parent_geometry_budget = geometry_budget;
                let lane_geometry_budget =
                    parent_geometry_budget.child_slice(support_uv_lane_geometry_work_limit(
                        points.len(),
                        parent_geometry_budget.remaining(),
                    ));
                let geometry_budget = &lane_geometry_budget;
                let mut contact_seeds = BlendContactSeedCache::default();
                let uv = (|| -> Result<Option<(Vec<Point2>, bool)>, cadmpeg_core::CodecError> {
                    let mut uv = Vec::new();
                    let mut all_parameters_certified = true;
                    for (point_index, point) in points.iter().enumerate() {
                        if !support_budget.charge() {
                            return Ok(None);
                        }
                        let serialized_seeds = serialized_support_uv_seed_candidates(
                            &surface.geometry,
                            serialized,
                            side,
                            point_index,
                        );
                        let continuation_seed = uv.last().copied();
                        let retained_pcurve_seed = pcurve_control_point_seed(
                            context.sides()[side]
                                .pcurve
                                .as_ref()
                                .map(|pcurve| &pcurve.geometry),
                            point_index,
                        );
                        let seed_candidates = ordered_support_uv_seed_candidates(
                            serialized_seeds,
                            retained_pcurve_seed,
                            continuation_seed,
                            linear_offset_surface,
                        )
                        .into_iter()
                        .flatten()
                        .map(Some)
                        .chain([None]);
                        let mut solved = None;
                        for seed in seed_candidates {
                            let sample_parameter = parameters[point_index];
                            let candidate = match &surface.geometry {
                                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
                                    if let Some(seed) = seed {
                                        fit_nurbs_surface_parameter_locally(
                                            geometry_budget.ctx,
                                            nurbs,
                                            *point,
                                            Some(seed),
                                            effective_fit_tolerance,
                                            geometry_budget,
                                        )?
                                        .map(|parameters| (parameters.get(), true))
                                    } else {
                                        unseeded_nurbs_surface_parameters_with_index_and_budget(
                                            &model_index,
                                            surface_id,
                                            &surface.geometry,
                                            nurbs,
                                            *point,
                                            effective_fit_tolerance,
                                            geometry_budget,
                                        )?
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
                                    let mut parameters = None;
                                    if source_chart_available {
                                        if let Some((source_pcurve, source_surface)) =
                                            source_pcurve.zip(other_surface_id)
                                        {
                                            parameters =
                                                blend_support_parameter_from_source_pcurve(
                                                    &model_index,
                                                    &SourcePcurveSample {
                                                        blend: source_surface,
                                                        support: surface_id,
                                                        source_pcurve: &source_pcurve.geometry,
                                                        curve_parameter: sample_parameter,
                                                    },
                                                    BoundaryInverseTarget {
                                                        point: *point,
                                                        seed,
                                                        tolerance: effective_fit_tolerance,
                                                    },
                                                    &mut contact_seeds,
                                                    geometry_budget,
                                                )?;
                                        }
                                    }
                                    if parameters.is_none() {
                                        if let Some((
                                            other_surface,
                                            other_pcurve,
                                            other_geometry,
                                            contact_pcurve,
                                            boundary,
                                        )) = other_contact
                                        {
                                            parameters =
                                                blend_boundary_parameter_from_contact_pcurve(
                                                    &model_index,
                                                    &ContactCurveSample {
                                                        support: other_surface,
                                                        support_geometry: other_geometry,
                                                        contact_pcurve,
                                                        boundary,
                                                        support_pcurve: other_pcurve,
                                                        curve_parameter: sample_parameter,
                                                    },
                                                    BoundaryInverseTarget {
                                                        point: *point,
                                                        seed,
                                                        tolerance: effective_fit_tolerance,
                                                    },
                                                    geometry_budget,
                                                )?;
                                        }
                                    }
                                    if parameters.is_none() {
                                        parameters = blend_surface_parameters_from_point_with_index_and_budget(
                                            &model_index, surface_id, *point, seed,
                                            effective_fit_tolerance, &mut contact_seeds,
                                            geometry_budget,
                                        )?;
                                    }
                                    if parameters.is_none() {
                                        if let Some(other_surface) = other_surface_id {
                                            parameters = blend_boundary_parameter_from_support_spine_with_index_and_budget(
                                                &model_index, surface_id, other_surface, *point,
                                                seed, effective_fit_tolerance, geometry_budget,
                                            )?;
                                        }
                                    }
                                    if parameters.is_none() {
                                        parameters = if let Some(seed) = seed {
                                            refine_offset_surface_parameters_with_index_and_budget(
                                                &model_index,
                                                surface_id,
                                                *point,
                                                seed,
                                                effective_fit_tolerance,
                                                geometry_budget,
                                            )?
                                        } else {
                                            offset_surface_parameters_with_tolerance_with_index_and_budget(
                                                &model_index, surface_id, *point, None,
                                                Some(effective_fit_tolerance), geometry_budget,
                                            )?
                                        };
                                    }
                                    if parameters.is_none() {
                                        parameters = solve_blend_parameters(
                                            &model_index,
                                            surface_id,
                                            *point,
                                            seed,
                                            effective_fit_tolerance,
                                            BlendParameterGrid::Disabled,
                                            geometry_budget,
                                        )?;
                                    }
                                    if parameters.is_none() {
                                        if !blend_parameter_grids.contains_key(surface_id) {
                                            let grid =
                                                blend_surface_parameter_grid_with_index_and_budget(
                                                    &model_index,
                                                    surface_id,
                                                    0,
                                                    geometry_budget,
                                                )?;
                                            blend_parameter_grids.insert(surface_id.clone(), grid);
                                        }
                                        if let Some(grid) = blend_parameter_grids
                                            .get(surface_id)
                                            .and_then(Option::as_deref)
                                        {
                                            parameters = solve_grid_parameters(
                                                &model_index,
                                                surface_id,
                                                *point,
                                                effective_fit_tolerance,
                                                grid,
                                                geometry_budget,
                                            )?;
                                        }
                                    }
                                    parameters.map(|parameters| (parameters, true))
                                }
                                geometry @ SurfaceGeometry::Solved(_) => {
                                    analytic_surface_parameters(geometry, *point)
                                        .map(|parameters| (Point2::from(parameters), false))
                                }
                            };
                            if candidate.is_some() {
                                solved = candidate;
                                break;
                            }
                        }
                        let Some((parameters, certified)) = solved else {
                            return Ok(None);
                        };
                        all_parameters_certified &= certified;

                        uv.push(parameters);
                    }
                    Ok(Some((uv, all_parameters_certified)))
                })();
                let Some((mut uv, all_parameters_certified)) = uv? else {
                    let parent_exhausted = parent_geometry_budget
                        .consume_child(&lane_geometry_budget)
                        .is_err();
                    let child_exhausted = lane_geometry_budget.exhausted();
                    lane_geometry_exhausted |= child_exhausted || parent_exhausted;
                    failed_attempts.insert(
                        attempt_key,
                        source_pcurve.map(|pcurve| pcurve.geometry.clone()),
                    );
                    continue;
                };
                if matches!(
                    surface.geometry,
                    SurfaceGeometry::Solved(
                        SolvedSurfaceGeometry::Cylinder(_)
                            | SolvedSurfaceGeometry::Cone(_)
                            | SolvedSurfaceGeometry::Sphere(_)
                            | SolvedSurfaceGeometry::Torus(_)
                    )
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
                        )?
                        else {
                            reproduces = false;
                            break;
                        };
                        if sample_index == 0 {
                            endpoint_values[0] = Some(actual);
                        }
                        if sample_index + 1 == points.len() {
                            endpoint_values[1] = Some(actual);
                        }
                        if Point3::distance(actual, *point) > effective_fit_tolerance {
                            reproduces = false;
                            break;
                        }
                    }
                    reproduces
                };
                let admitted_fit_tolerance =
                    cadmpeg_ir::geometry::FitTolerance::try_new(effective_fit_tolerance).ok();
                if let Some(admitted_fit_tolerance) =
                    admitted_fit_tolerance.filter(|_| reproduces_chart)
                {
                    if all_parameters_certified {
                        endpoint_values = [
                            if let Some(sample_uv) = uv.first() {
                                decoded_surface_point_with_geometry_and_budget(
                                    &model_index,
                                    surface_id,
                                    &surface.geometry,
                                    sample_uv.u,
                                    sample_uv.v,
                                    0,
                                    geometry_budget,
                                )?
                            } else {
                                None
                            },
                            if let Some(sample_uv) = uv.last() {
                                decoded_surface_point_with_geometry_and_budget(
                                    &model_index,
                                    surface_id,
                                    &surface.geometry,
                                    sample_uv.u,
                                    sample_uv.v,
                                    0,
                                    geometry_budget,
                                )?
                            } else {
                                None
                            },
                        ];
                    }
                    let parameter_range = samples.parameter_range();
                    let pcurve = linear_pcurve_geometry(parameters, &uv)?;
                    if let [Some(first), Some(last)] = endpoint_values {
                        let key = (owner.clone(), surface_id.clone());

                        let entries = match endpoint_witnesses.entry(key) {
                            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                entry.insert(Vec::new())
                            }
                        };

                        entries.push((pcurve.clone(), parameter_range, [first, last]));
                    }

                    replacements.push((
                        procedural_id.clone(),
                        side,
                        pcurve,
                        admitted_fit_tolerance,
                    ));
                } else {
                    failed_attempts.insert(
                        attempt_key,
                        source_pcurve.map(|pcurve| pcurve.geometry.clone()),
                    );
                }
                let parent_exhausted = parent_geometry_budget
                    .consume_child(&lane_geometry_budget)
                    .is_err();
                let child_exhausted = lane_geometry_budget.exhausted();
                lane_geometry_exhausted |= child_exhausted || parent_exhausted;
            }
        }
        let mut cache_backed_constructions = BTreeSet::<ProceduralCurveId>::new();
        for curve in &ir.model.curves {
            if curve.geometry.solved_cache().is_none() {
                continue;
            }
            if let Some(construction) = curve.geometry.procedural_construction() {
                cache_backed_constructions.insert(construction.clone());
            }
        }
        drop(model_index);
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
                if pcurve_requires_completion(
                    context.sides()[side]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| &pcurve.geometry),
                ) {
                    context.set_unmapped_pcurve(side, Some(pcurve));
                    true
                } else {
                    false
                }
            });
            if completed && cache_backed_constructions.contains(&procedural.id) {
                procedural.require_cache_fit_tolerance(effective_fit_tolerance)?;
            }
        }
    }

    // Independent inverse admission is the cheapest certified route. Run
    // coupled continuation only after this wave has had a chance to fill the
    // same lanes, so difficult nested supports are reserved for residuals.
    let coupled_geometry_exhausted = coupled_geometry_budget.exhausted();
    if !coupled_geometry_exhausted {
        coupled_geometry_budget.clear_blend_frame_cache();
        lane_geometry_exhausted |= complete_coupled_support_uv(
            ir,
            pending,
            coupled_support_budget,
            coupled_geometry_budget,
            failed_coupled_attempts,
            endpoint_witnesses,
        )?;
    }
    Ok(lane_geometry_exhausted)
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

pub(super) fn blend_spine_cache_fit_tolerance_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    fit_tolerance: f64,
) -> f64 {
    let tolerance = match blend_surface_definition_with_index(index, surface) {
        Some(CircularBlendDefinition { spine, .. }) => index
            .procedural_curves_for_curve(spine.as_str(), cadmpeg_ir::index::StandardIndex)
            .and_then(|procedurals| procedurals.first().copied())
            .and_then(cadmpeg_ir::geometry::ProceduralCurve::cache_fit_tolerance),
        None => None,
    };
    tolerance
        .filter(|tolerance| tolerance.get() > 0.0)
        .map_or(fit_tolerance, |tolerance| fit_tolerance + tolerance.get())
}

fn complete_blend_boundary_support_uv_with_index_and_budget(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces: [&SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    seeds: [Option<Point2>; 2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<[Vec<Point2>; 2]>, cadmpeg_core::CodecError> {
    let mut selected_sides = None;
    for blend_side in 0..2 {
        let Some(CircularBlendDefinition { supports, .. }) =
            blend_surface_definition_with_index(index, surfaces[blend_side])
        else {
            continue;
        };
        let mut support_matches = 0;
        for support in supports {
            if parameterization_equivalent_surfaces_with_index(
                index,
                support,
                surfaces[1 - blend_side],
            )? {
                support_matches += 1;
            }
        }
        if support_matches == 1 {
            selected_sides = Some((blend_side, 1 - blend_side));
            break;
        }
    }
    let Some((blend_side, support_side)) = selected_sides else {
        return Ok(None);
    };
    let mut lanes = [Vec::new(), Vec::new()];
    for point in points {
        let blend_seed = lanes[blend_side].last().copied().or(seeds[blend_side]);
        let support_seed = lanes[support_side].last().copied().or(seeds[support_side]);
        let Some(blend_parameters) =
            blend_boundary_parameter_from_support_spine_with_index_and_budget(
                index,
                surfaces[blend_side],
                surfaces[support_side],
                *point,
                blend_seed,
                fit_tolerance,
                geometry_budget,
            )?
        else {
            return Ok(None);
        };
        let Some(support_parameters) = surface_parameters_for_fit_with_index_and_budget(
            index,
            surfaces[support_side],
            *point,
            support_seed,
            fit_tolerance,
            geometry_budget,
        )?
        else {
            return Ok(None);
        };

        lanes[blend_side].push(blend_parameters);
        lanes[support_side].push(support_parameters);
    }
    Ok(Some(lanes))
}

fn complete_coupled_support_uv(
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    coupled_support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    failed_attempts: &mut BTreeMap<
        ProceduralCurveId,
        [Option<cadmpeg_ir::geometry::SupportPcurve>; 2],
    >,
    endpoint_witnesses: &mut EndpointWitnesses,
) -> Result<bool, cadmpeg_core::CodecError> {
    if geometry_budget.exhausted() {
        return Ok(false);
    }
    let mut lane_geometry_exhausted = false;
    let mut replacements = Vec::new();
    let mut blend_parameter_grids = BTreeMap::<&str, Option<Vec<(Point2, Point3)>>>::new();
    let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    for (procedural_id, samples, fit_tolerance, serialized) in pending {
        let points = &samples.points();
        let parameters = &samples.parameters();
        let Some(procedural) =
            model_index.procedural_curves(procedural_id.as_str(), cadmpeg_ir::index::StandardIndex)
        else {
            continue;
        };
        let Some(owner) = model_index
            .ir()
            .model
            .procedural_curve_owner(&procedural.id)
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let [first_state, second_state] = context.sides().each_ref().map(|side| {
            side.pcurve
                .as_ref()
                .map(|pcurve| {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::geometry::SupportPcurve::new(
                        pcurve.geometry.clone(),
                        pcurve.parameter_range,
                    ))
                })
                .transpose()
        });
        let lane_state = [first_state?, second_state?];
        if failed_attempts
            .get(procedural_id)
            .is_some_and(|previous| previous == &lane_state)
        {
            continue;
        }
        let missing = context.sides().each_ref().map(|side| {
            pcurve_requires_completion(side.pcurve.as_ref().map(|pcurve| &pcurve.geometry))
        });
        let [Some(first_surface), Some(second_surface)] =
            context.sides().each_ref().map(|side| side.surface.as_ref())
        else {
            continue;
        };
        let surfaces = [first_surface, second_surface];
        let both_lanes_missing = missing == [true, true];
        let mut has_procedural_support = false;
        for surface in &surfaces {
            if model_index
                .surfaces(surface.as_str(), cadmpeg_ir::index::StandardIndex)
                .is_some_and(|surface| {
                    matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                })
            {
                has_procedural_support = true;
                break;
            }
        }
        let mut seeded_procedural_support = false;
        for side in 0..2 {
            if missing[side]
                && pcurve_control_point_seed(
                    context.sides()[side]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| &pcurve.geometry),
                    0,
                )
                .is_some()
                && model_index
                    .surfaces(surfaces[side].as_str(), cadmpeg_ir::index::StandardIndex)
                    .is_some_and(|surface| {
                        matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                    })
            {
                seeded_procedural_support = true;
                break;
            }
        }
        let mut sourced_procedural_support = false;
        for side in 0..2 {
            if missing[side]
                && context.sides()[1 - side]
                    .pcurve
                    .as_ref()
                    .is_some_and(|pcurve| !pcurve_requires_completion(Some(&pcurve.geometry)))
                && model_index
                    .surfaces(surfaces[side].as_str(), cadmpeg_ir::index::StandardIndex)
                    .is_some_and(|surface| {
                        matches!(surface.geometry, SurfaceGeometry::Procedural { .. })
                    })
            {
                sourced_procedural_support = true;
                break;
            }
        }
        if !(seeded_procedural_support
            || sourced_procedural_support
            || (both_lanes_missing && has_procedural_support))
        {
            continue;
        }
        let missing_lanes = missing.iter().filter(|missing| **missing).count();
        let Some(sample_work) = points.len().checked_mul(missing_lanes) else {
            coupled_support_budget.exhaust();
            break;
        };
        if !coupled_support_budget.charge_by(work_units(sample_work)) {
            break;
        }
        let mut seeds = [None; 2];
        for (side, seed) in seeds.iter_mut().enumerate() {
            let support = &context.sides()[side];
            *seed = pcurve_control_point_seed(
                support.pcurve.as_ref().map(|pcurve| &pcurve.geometry),
                0,
            );
            if seed.is_none() {
                if let Some(surface) =
                    model_index.surfaces(surfaces[side].as_str(), cadmpeg_ir::index::StandardIndex)
                {
                    *seed =
                        serialized_support_uv_seed_for_side(&surface.geometry, serialized, side);
                }
            }
        }
        let parent_geometry_budget = geometry_budget;
        let lane_geometry_budget = parent_geometry_budget.child_slice(
            support_uv_lane_geometry_work_limit(points.len(), parent_geometry_budget.remaining()),
        );
        let geometry_budget = &lane_geometry_budget;
        let mut lanes = complete_blend_boundary_support_uv_with_index_and_budget(
            &model_index,
            surfaces,
            points,
            *fit_tolerance,
            seeds,
            geometry_budget,
        )?;
        if lanes.is_none() {
            lanes = continue_surface_intersection_parameters_with_index_and_seeds_and_budget_and_grid_cache(
                &model_index,
                surfaces,
                points,
                *fit_tolerance,
                seeds,
                geometry_budget,
                &mut blend_parameter_grids,
            )?;
        }
        let Some(lanes) = lanes else {
            failed_attempts.insert(procedural_id.clone(), lane_state);
            let parent_exhausted = parent_geometry_budget
                .consume_child(&lane_geometry_budget)
                .is_err();
            let child_exhausted = lane_geometry_budget.exhausted();
            lane_geometry_exhausted |= child_exhausted || parent_exhausted;
            continue;
        };
        for side in 0..2 {
            if missing[side] {
                let endpoint_values = if let Some(surface) =
                    model_index.surfaces(surfaces[side].as_str(), cadmpeg_ir::index::StandardIndex)
                {
                    let first = if let Some(parameters) = lanes[side].first() {
                        decoded_surface_point_with_geometry_and_budget(
                            &model_index,
                            surfaces[side],
                            &surface.geometry,
                            parameters.u,
                            parameters.v,
                            0,
                            geometry_budget,
                        )?
                    } else {
                        None
                    };
                    let last = if let Some(parameters) = lanes[side].last() {
                        decoded_surface_point_with_geometry_and_budget(
                            &model_index,
                            surfaces[side],
                            &surface.geometry,
                            parameters.u,
                            parameters.v,
                            0,
                            geometry_budget,
                        )?
                    } else {
                        None
                    };
                    Some([first, last])
                } else {
                    None
                };
                let parameter_range = samples.parameter_range();
                let pcurve = linear_pcurve_geometry(parameters, &lanes[side])?;
                if let Some([Some(first), Some(last)]) = endpoint_values {
                    let key = (owner.clone(), surfaces[side].clone());

                    let entries = match endpoint_witnesses.entry(key) {
                        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(Vec::new())
                        }
                    };

                    entries.push((pcurve.clone(), parameter_range, [first, last]));
                }

                replacements.push((procedural_id.clone(), side, pcurve));
            }
        }
        let parent_exhausted = parent_geometry_budget
            .consume_child(&lane_geometry_budget)
            .is_err();
        let child_exhausted = lane_geometry_budget.exhausted();
        lane_geometry_exhausted |= child_exhausted || parent_exhausted;
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
        let Some(context) = procedural.intersection_context_mut() else {
            continue;
        };

        if pcurve_requires_completion(
            context.sides()[side]
                .pcurve
                .as_ref()
                .map(|pcurve| &pcurve.geometry),
        ) {
            context.set_unmapped_pcurve(side, Some(pcurve));
        }
    }
    Ok(lane_geometry_exhausted)
}

#[cfg(test)]
pub(super) fn complete_coupled_support_uv_for_test(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) {
    complete_coupled_support_uv_with_geometry_budget_for_test(
        ctx,
        ir,
        pending,
        super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    )
    .expect("the coupled support-uv wave pairs its lanes");
}

#[cfg(test)]
pub(super) fn complete_coupled_support_uv_with_geometry_budget_for_test(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    geometry_work: usize,
) -> Result<bool, cadmpeg_core::CodecError> {
    let coupled_support_budget = new_support_uv_budget();
    let geometry_budget = GeometryWorkBudget::from_context(ctx, geometry_work);
    let mut failed_attempts = BTreeMap::new();

    complete_coupled_support_uv(
        ir,
        pending,
        &coupled_support_budget,
        &geometry_budget,
        &mut failed_attempts,
        &mut BTreeMap::new(),
    )
}

pub(super) fn complete_parameterization_equivalent_support_uv(
    ir: &mut CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    let replacements = {
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        let mut replacements = Vec::new();
        for (procedural_index, procedural) in ir.model.procedural_curves.iter().enumerate() {
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
            else {
                continue;
            };
            let missing = context.sides().each_ref().map(|side| {
                pcurve_requires_completion(side.pcurve.as_ref().map(|pcurve| &pcurve.geometry))
            });
            let target = match missing {
                [true, false] => 0,
                [false, true] => 1,
                _ => continue,
            };
            let source = 1 - target;
            let (Some(target_surface), Some(source_surface), Some(_)) = (
                context.sides()[target].surface.as_ref(),
                context.sides()[source].surface.as_ref(),
                context.sides()[source].pcurve.as_ref(),
            ) else {
                continue;
            };
            if parameterization_equivalent_surfaces_with_index(
                &model_index,
                target_surface,
                source_surface,
            )? {
                replacements.push((procedural_index, target, source));
            }
        }
        replacements
    };
    for (procedural_index, side, source) in replacements {
        let Some(context) = ir.model.procedural_curves[procedural_index].intersection_context_mut()
        else {
            continue;
        };

        if pcurve_requires_completion(
            context.sides()[side]
                .pcurve
                .as_ref()
                .map(|pcurve| &pcurve.geometry),
        ) {
            context.copy_pcurve(source, side);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn parameterization_equivalent_surfaces(
    ir: &CadIr,
    first: &SurfaceId,
    second: &SurfaceId,
) -> bool {
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
    parameterization_equivalent_surfaces_with_index(&index, first, second).unwrap()
}

pub(super) fn parameterization_equivalent_surfaces_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    first: &SurfaceId,
    second: &SurfaceId,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    enum Step<'a> {
        Equal,
        Different,
        Follow(&'a SurfaceId, &'a SurfaceId),
    }

    fn step<'a>(
        index: &'a cadmpeg_ir::index::ModelIndex<'_>,
        first: &'a SurfaceId,
        second: &'a SurfaceId,
    ) -> Step<'a> {
        if first == second {
            return Step::Equal;
        }
        let (Some(first_geometry), Some(second_geometry)) = (
            index
                .surfaces(first.as_str(), cadmpeg_ir::index::StandardIndex)
                .map(|surface| &surface.geometry),
            index
                .surfaces(second.as_str(), cadmpeg_ir::index::StandardIndex)
                .map(|surface| &surface.geometry),
        ) else {
            return Step::Different;
        };
        if first_geometry == second_geometry {
            return Step::Equal;
        }
        let (
            Some(ProceduralSurfaceDefinition::Offset(first_payload)),
            Some(ProceduralSurfaceDefinition::Offset(second_payload)),
        ) = (
            index
                .procedural_surface_for_surface(first.as_str(), cadmpeg_ir::index::StandardIndex)
                .map(cadmpeg_ir::geometry::ProceduralSurface::definition),
            index
                .procedural_surface_for_surface(second.as_str(), cadmpeg_ir::index::StandardIndex)
                .map(cadmpeg_ir::geometry::ProceduralSurface::definition),
        )
        else {
            return Step::Different;
        };
        let first_support = first_payload.support();
        let first_distance = first_payload.distance();
        let first_u_sense = first_payload.u_sense();
        let first_v_sense = first_payload.v_sense();
        let first_support_extension = first_payload.linear_support_extension();
        let first_extension = first_payload.extension();
        let second_support = second_payload.support();
        let second_distance = second_payload.distance();
        let second_u_sense = second_payload.u_sense();
        let second_v_sense = second_payload.v_sense();
        let second_support_extension = second_payload.linear_support_extension();
        let second_extension = second_payload.extension();
        if first_distance.get().to_bits() == second_distance.get().to_bits()
            && first_u_sense == second_u_sense
            && first_v_sense == second_v_sense
            && first_support_extension == second_support_extension
            && first_extension == second_extension
        {
            Step::Follow(first_support, second_support)
        } else {
            Step::Different
        }
    }

    let mut slow = (first, second);
    let mut fast = (first, second);
    loop {
        slow = match step(index, slow.0, slow.1) {
            Step::Equal => return Ok(true),
            Step::Different => return Ok(false),
            Step::Follow(first, second) => (first, second),
        };
        for _ in 0..2 {
            fast = match step(index, fast.0, fast.1) {
                Step::Equal => return Ok(true),
                Step::Different => return Ok(false),
                Step::Follow(first, second) => (first, second),
            };
        }
        if slow == fast {
            return Ok(false);
        }
    }
}

/// One stream's ownership and provenance context for deferred intersection-chart
/// attachment.
pub(super) struct IntersectionCompletionSource<'a> {
    pub(super) scope: crate::decode::ids::IdScope,
    pub(super) graph: &'a Graph,
    pub(super) source_stream: StreamHandle,
    pub(super) coedge_start: usize,
    pub(super) procedural_start: usize,
}

fn stream_owns_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|suffix| suffix.starts_with(':'))
}

pub(super) struct IntersectionStream<'inputs> {
    pub(super) graph: &'inputs Graph,
    pub(super) scope: &'inputs crate::decode::ids::IdScope,
    pub(super) coedge_start: usize,
    pub(super) procedural_start: usize,
    pub(super) source_stream: cadmpeg_ir::annotations::StreamHandle,
    pub(super) validated_endpoint_witnesses: &'inputs EndpointWitnesses,
}

/// Attach charts for one stream without rescanning coedges emitted by an earlier
/// phase.
pub(super) fn attach_completed_intersection_pcurves_for_stream_with_budget(
    ir: &mut CadIr,
    intersection_stream: IntersectionStream<'_>,
    annotations: &mut AnnotationBuilder,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let IntersectionStream {
        graph,
        scope,
        coedge_start,
        procedural_start,
        source_stream,
        validated_endpoint_witnesses,
    } = intersection_stream;

    let source = IntersectionCompletionSource {
        scope: scope.clone(),
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
    )?;
    Ok(())
}

/// Re-run chart attachment over the complete model after all stream-owned
/// topology and intersection contexts exist.
pub(super) fn attach_completed_intersection_pcurves_for_model_with_budget(
    ir: &mut CadIr,
    sources: &[IntersectionCompletionSource<'_>],
    annotations: &mut AnnotationBuilder,
    validated_endpoint_witnesses: &EndpointWitnesses,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    attach_completed_intersection_pcurves_for_sources_with_budget(
        ir,
        sources,
        annotations,
        validated_endpoint_witnesses,
        geometry_budget,
    )?;
    Ok(())
}

fn attach_completed_intersection_pcurves_for_sources_with_budget(
    ir: &mut CadIr,
    sources: &[IntersectionCompletionSource<'_>],
    annotations: &mut AnnotationBuilder,
    validated_endpoint_witnesses: &EndpointWitnesses,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut source_prefixes = Vec::new();
    for source in sources {
        source_prefixes.push(source.scope.prefix());
    }
    let mut loop_faces = BTreeMap::new();
    for loop_ in &ir.model.loops {
        loop_faces.insert(&loop_.id, &loop_.face);
    }
    let mut face_surfaces = BTreeMap::new();
    for face in &ir.model.faces {
        face_surfaces.insert(&face.id, &face.surface);
    }
    let mut edge_curves = BTreeMap::new();
    let mut edge_tolerances = BTreeMap::new();
    for edge in &ir.model.edges {
        if let Some(curve) = edge.curve() {
            edge_curves.insert(&edge.id, curve);
        }
        if let Some(tolerance) = edge.tolerance {
            edge_tolerances.insert(&edge.id, tolerance.get());
        }
    }
    let mut coedge_candidates = Vec::new();
    for (index, coedge) in ir.model.coedges.iter().enumerate() {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(source_index) =
            sources
                .iter()
                .zip(&source_prefixes)
                .position(|(source, prefix)| {
                    index >= source.coedge_start && stream_owns_id(coedge.id.as_str(), prefix)
                })
        else {
            continue;
        };
        let Some(surface) = loop_faces
            .get(&coedge.owner_loop)
            .and_then(|face| face_surfaces.get(*face))
        else {
            continue;
        };
        let Some(curve) = edge_curves.get(&coedge.edge) else {
            continue;
        };

        coedge_candidates.push((
            coedge.id.clone(),
            coedge.edge.clone(),
            (*curve).clone(),
            (*surface).clone(),
            edge_tolerances.get(&coedge.edge).copied(),
            source_index,
        ));
    }
    if coedge_candidates.is_empty() {
        return Ok(());
    }
    let mut required_keys = BTreeSet::new();
    for (_, _, curve, surface, _, _) in &coedge_candidates {
        required_keys.insert((curve.clone(), surface.clone()));
    }
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
            && !sources
                .iter()
                .zip(&source_prefixes)
                .any(|(source, prefix)| {
                    index >= source.procedural_start
                        && stream_owns_id(procedural.id.as_str(), prefix)
                })
        {
            continue;
        }
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let Some(owner) = ir.model.procedural_curve_owner(&procedural.id) else {
            continue;
        };
        for side in context.sides() {
            let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            let key = ((*owner).clone(), (*surface).clone());
            if !required_keys.contains(&key) {
                continue;
            }

            let values = match candidates.entry(key) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
            };
            let candidate = (
                pcurve.geometry.clone(),
                context.parameter_range().endpoints(),
                procedural
                    .cache_fit_tolerance()
                    .map(cadmpeg_ir::geometry::FitTolerance::get),
            );
            if !values.contains(&candidate) {
                values.push(candidate);
            }
        }
    }

    let replacements = {
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        let mut edge_endpoint_contracts = BTreeMap::new();
        for (_, edge_id, ..) in &coedge_candidates {
            if edge_endpoint_contracts.contains_key(edge_id) {
                continue;
            }
            if let Some(contract) = pcurve_edge_endpoint_contract_with_index(&model_index, edge_id)?
            {
                edge_endpoint_contracts.insert(edge_id.clone(), contract);
            }
        }
        // A chart carrier's serialized endpoint witnesses are a necessary
        // edge-incidence condition. Reuse a prior sample-wise proof; evaluate
        // the face surface only for keys without that proof.
        let mut endpoint_admissible_keys = BTreeSet::new();
        for (_, edge_id, curve, surface, edge_tolerance, _) in &coedge_candidates {
            let key = ((*curve).clone(), (*surface).clone());
            let Some(values) = candidates.get(&key) else {
                continue;
            };
            let [candidate] = values.as_slice() else {
                continue;
            };
            let admissible = if let Some(witness) =
                linear_nurbs_curve_endpoint_witness_with_index(&model_index, curve)
            {
                let Some((edge_endpoints, edge_allowance)) =
                    edge_endpoint_contracts.get(edge_id).copied()
                else {
                    continue;
                };
                let fit_tolerance = candidate.2.or(*edge_tolerance);
                pcurve_matches_edge_endpoint_contract(
                    witness,
                    edge_endpoints,
                    edge_allowance,
                    fit_tolerance,
                )
            } else {
                true
            };
            if admissible {
                endpoint_admissible_keys.insert(key);
            }
        }
        let mut witnessed_keys = BTreeSet::new();
        let mut candidate_endpoints = BTreeMap::new();
        for (key, values) in &candidates {
            if !endpoint_admissible_keys.contains(key) {
                continue;
            }
            let [candidate] = values.as_slice() else {
                continue;
            };
            let witness = endpoint_witness_for_candidate(
                validated_endpoint_witnesses,
                key,
                &candidate.0,
                candidate.1,
            );
            if witness.is_some() {
                witnessed_keys.insert((key.0.clone(), key.1.clone()));
            }
            let endpoints = if witness.is_some() {
                witness
            } else {
                pcurve_surface_endpoints_with_index_and_budget(
                    &model_index,
                    &key.1,
                    &candidate.0,
                    None,
                    geometry_budget,
                )?
            };
            candidate_endpoints.insert((key.0.clone(), key.1.clone()), endpoints);
        }
        let mut replacements = Vec::new();
        for (coedge_id, edge_id, curve, surface, edge_tolerance, source_index) in coedge_candidates
        {
            let key = (curve, surface);
            let Some(candidate_values) = candidates.get(&key) else {
                continue;
            };
            let [candidate] = candidate_values.as_slice() else {
                continue;
            };
            let Some((edge_endpoints, edge_allowance)) =
                edge_endpoint_contracts.get(&edge_id).copied()
            else {
                continue;
            };
            let fit_tolerance = candidate.2.or(edge_tolerance);
            let matches = {
                let Some(coincident_surface) =
                    candidate_endpoints.get(&key).and_then(Option::as_ref)
                else {
                    continue;
                };
                pcurve_matches_edge_endpoint_contract(
                    *coincident_surface,
                    edge_endpoints,
                    edge_allowance,
                    fit_tolerance,
                )
            };
            if !matches {
                if !witnessed_keys.remove(&key) {
                    continue;
                }
                // A witness from another geometry phase is a shortcut. The
                // endpoint evaluator resolves a disagreement with that proof.
                let fallback = pcurve_surface_endpoints_with_index_and_budget(
                    &model_index,
                    &key.1,
                    &candidate.0,
                    None,
                    geometry_budget,
                )?;
                let Some(slot) = candidate_endpoints.get_mut(&key) else {
                    continue;
                };
                *slot = fallback;
                let Some(coincident_surface) = slot.as_ref() else {
                    continue;
                };
                if !pcurve_matches_edge_endpoint_contract(
                    *coincident_surface,
                    edge_endpoints,
                    edge_allowance,
                    fit_tolerance,
                ) {
                    continue;
                }
            }
            let metadata = (|| {
                Some(cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                    None,
                    Some(cadmpeg_ir::units::FiniteVector::new(candidate.1)?),
                    fit_tolerance
                        .map(cadmpeg_ir::geometry::FitTolerance::try_new)
                        .transpose()
                        .ok()?,
                ))
            })();
            let Some(metadata) = metadata else {
                continue;
            };

            replacements.push((coedge_id, source_index, (candidate.0.clone(), metadata)));
        }
        replacements
    };
    for (coedge_id, source_index, (geometry, metadata)) in replacements {
        let source = &sources[source_index];
        let Some(fin_xmt) = coedge_id
            .as_str()
            .rsplit_once('#')
            .and_then(|(_, value)| value.parse::<u32>().ok())
        else {
            continue;
        };
        let pcurve_id: PcurveId = source.scope.id_from_display(
            &cadmpeg_ir::identity_component!("intersection-pcurve-completed"),
            fin_xmt,
        )?;
        if ir.model.pcurves.iter().any(|pcurve| pcurve.id == pcurve_id) {
            continue;
        }
        let source_offset = source
            .graph
            .get(NodeKind::Fin, fin_xmt)
            .map_or(0, |node| cadmpeg_core::decode::u64_from_index(node.pos()));
        annotations.note(
            &pcurve_id,
            &source.source_stream,
            source_offset,
            Some("INTERSECTION_PCURVE"),
        );
        annotations
            .derived(&pcurve_id, "geometry")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(&pcurve_id, "parameter_range")
            .map_err(cadmpeg_core::CodecError::from)?;
        if metadata.fit_tolerance().is_some() {
            annotations
                .derived(&pcurve_id, "fit_tolerance")
                .map_err(cadmpeg_core::CodecError::from)?;
        }

        ir.model.pcurves.push(Pcurve {
            id: pcurve_id.clone(),
            geometry,
            metadata,
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::geometry_work::GeometryWorkBudget;
    use super::{
        complete_support_uv_with_budget_and_endpoint_witnesses, ordered_support_uv_seed_candidates,
        support_uv_lane_geometry_work_limit, support_uv_lane_matches_surface_with_budget,
        unseeded_nurbs_surface_parameters_with_index_and_budget, MAX_SUPPORT_UV_LANE_GEOMETRY_WORK,
        MAX_SUPPORT_UV_SAMPLES,
    };
    use crate::intersection::SupportUvLane;
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn serialized_support_seed_selection_converts_planar_lanes() {
        use cadmpeg_ir::geometry::analytic::PlaneSurface;
        use cadmpeg_ir::math::Vector3;

        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0., 0., 0.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
            )
            .expect("plane"),
        ));
        let source =
            super::SerializedSupportUv::from_values([Some(vec![[1., 2.]]), Some(vec![[3., 4.]])]);
        assert_eq!(
            super::serialized_support_uv_seed_candidates(&surface, &source, 0, 0),
            [
                Some(Point2::new(1000., 2000.)),
                Some(Point2::new(3000., 4000.)),
                None,
                None
            ]
        );
        assert_eq!(
            super::serialized_support_uv_seed_for_side(&surface, &source, 0),
            Some(Point2::new(1000., 2000.))
        );
    }

    #[test]
    fn linear_knots_repeat_end_parameters() {
        assert_eq!(super::linear_knots(&[0.0, 1.0]), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn support_uv_completion_skips_at_local_geometry_work_limit() {
        crate::test_support::with_decode_context(|ctx| {
            let support_budget = cadmpeg_core::decode::WorkBudget::new(10);
            let geometry_budget = GeometryWorkBudget::from_context(ctx, 0);
            assert!(!geometry_budget.charge());
            assert!(geometry_budget.exhausted());
            let mut ir = CadIr::empty();
            let mut endpoint_witnesses = BTreeMap::new();

            complete_support_uv_with_budget_and_endpoint_witnesses(
                &mut ir,
                &[],
                (&support_budget, &geometry_budget),
                (&support_budget, &geometry_budget),
                &mut endpoint_witnesses,
            )
            .expect("an exhausted local geometry slice skips completion");
            assert!(endpoint_witnesses.is_empty());
            assert!(ctx.resource_refusal().is_none());
        });
    }

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
        crate::test_support::with_decode_context(|geometry_ctx| {
            let surface_id = SurfaceId::mint("test:model:entity#synthetic:support-plane")
                .expect("identity grammar");
            let mut ir = CadIr::empty();
            ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                source_object: None,
            });
            let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
            let points = vec![Point3::new(0.0, 0.0, 0.0); MAX_SUPPORT_UV_SAMPLES + 1];
            let values =
                SupportUvLane::new(vec![[0.0, 0.0]; MAX_SUPPORT_UV_SAMPLES + 1], points.len())
                    .unwrap();
            let geometry_budget = GeometryWorkBudget::from_context(geometry_ctx, 1);

            assert!(!support_uv_lane_matches_surface_with_budget(
                &index,
                &surface_id,
                &points,
                0.0,
                Some(&values),
                &geometry_budget,
            )
            .expect("evaluator allocation succeeds"));
            assert_eq!(geometry_budget.remaining(), 1);
        });
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

        assert!(matches!(parent.consume_child(&lane), Ok(())));
        assert_eq!(parent.consumed(), MAX_SUPPORT_UV_LANE_GEOMETRY_WORK);
        assert!(!parent.exhausted());

        let later_lane = parent.child_slice(support_uv_lane_geometry_work_limit(
            MAX_SUPPORT_UV_SAMPLES,
            parent.remaining(),
        ));
        assert!(later_lane.charge());
    }

    #[test]
    fn support_uv_completion_count_overflow_caps_at_the_model_ceiling() {
        assert_eq!(
            super::support_uv_completion_budget_limit(usize::MAX),
            super::MAX_SUPPORT_UV_COMPLETION_SAMPLES
        );
    }

    #[test]
    fn support_uv_lane_count_overflow_caps_at_the_lane_ceiling() {
        assert_eq!(
            support_uv_lane_geometry_work_limit(usize::MAX, usize::MAX),
            MAX_SUPPORT_UV_LANE_GEOMETRY_WORK
        );
    }

    #[test]
    fn unseeded_nurbs_completion_accepts_only_a_tolerance_certified_coarse_fit() {
        const FIT_TOLERANCE: f64 = 1.0e-10;
        const GEOMETRY_WORK: usize = 1_024;

        for work_limit in [GEOMETRY_WORK, 32768] {
            crate::test_support::with_decode_context(|geometry_ctx| {
                let surface_id =
                    SurfaceId::mint("test:model:entity#synthetic:coarse-nurbs-support")
                        .expect("identity grammar");
                let nurbs = cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        false,
                    ),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        false,
                    ),
                    cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                        vec![
                            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                        ],
                        None,
                    ),
                    false,
                )
                .expect("valid test surface");
                let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs.clone()));
                let mut ir = CadIr::empty();
                ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
                    id: surface_id.clone(),
                    geometry: geometry.clone(),
                    source_object: None,
                });
                let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);

                let fit_budget = GeometryWorkBudget::from_context(geometry_ctx, work_limit);
                let parameters = unseeded_nurbs_surface_parameters_with_index_and_budget(
                    &index,
                    &surface_id,
                    &geometry,
                    &nurbs,
                    Point3::new(0.5, 0.5, 0.0),
                    FIT_TOLERANCE,
                    &fit_budget,
                );
                if work_limit == GEOMETRY_WORK {
                    // The original slice cannot admit the complete grid: the
                    // fit is skipped and the session continues.
                    assert_eq!(parameters, Ok(None));
                    assert!(fit_budget.exhausted());
                    assert!(geometry_ctx.resource_refusal().is_none());
                    crate::test_support::with_decode_context(|miss_ctx| {
                        let miss_budget = GeometryWorkBudget::from_context(miss_ctx, work_limit);
                        let missed = unseeded_nurbs_surface_parameters_with_index_and_budget(
                            &index,
                            &surface_id,
                            &geometry,
                            &nurbs,
                            Point3::new(0.5, 0.5, 1.0),
                            FIT_TOLERANCE,
                            &miss_budget,
                        );
                        assert_eq!(missed, Ok(None));
                        assert!(miss_budget.exhausted());
                        assert!(miss_ctx.resource_refusal().is_none());
                    });
                    return;
                }
                let parameters = parameters
                    .expect("evaluator allocation succeeds")
                    .expect("coarse grid contains the exact chart point");
                assert_eq!(parameters, Point2::new(0.5, 0.5));

                let miss_budget = GeometryWorkBudget::from_context(geometry_ctx, work_limit);
                assert!(unseeded_nurbs_surface_parameters_with_index_and_budget(
                    &index,
                    &surface_id,
                    &geometry,
                    &nurbs,
                    Point3::new(0.5, 0.5, 1.0),
                    FIT_TOLERANCE,
                    &miss_budget,
                )
                .expect("evaluator allocation succeeds")
                .is_none());
            });
        }
    }
    #[test]
    fn empty_support_uv_completion_does_not_index_unrelated_model_records() {
        let mut ir = CadIr::empty();
        for i in 0..1000 {
            ir.model.points.push(cadmpeg_ir::topology::Point::new(
                cadmpeg_ir::ids::PointId::mint(format!("nx:model:point#{i}")).expect("identity"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0., 0., 0.)).expect("point"),
                None,
            ));
        }
        let before = serde_json::to_value(&ir).expect("model serialization");
        crate::test_support::with_decode_context(|ctx| {
            let support_budget = cadmpeg_core::decode::WorkBudget::new(100);
            let geometry_budget = GeometryWorkBudget::from_context(ctx, 100);
            let mut endpoint_witnesses = BTreeMap::new();
            assert!(!complete_support_uv_with_budget_and_endpoint_witnesses(
                &mut ir,
                &[],
                (&support_budget, &geometry_budget),
                (&support_budget, &geometry_budget),
                &mut endpoint_witnesses,
            )
            .expect("an empty candidate list needs no model index"));
            assert!(endpoint_witnesses.is_empty());
            super::complete_ext11_support_uv_with_budget(&mut ir, &[], &geometry_budget)
                .expect("empty serialized completion needs no model index");
            let validation =
                super::invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
                    ctx,
                    &mut ir,
                    &[],
                    &std::collections::BTreeSet::new(),
                    &support_budget,
                    &geometry_budget,
                    false,
                )
                .expect("empty invalidation needs no model index");
            assert!(!validation.lane_geometry_exhausted);
            assert!(validation.endpoint_witnesses.is_empty());
            assert!(ctx.resource_refusal().is_none());
        });
        assert_eq!(
            serde_json::to_value(&ir).expect("model serialization"),
            before
        );
    }

    #[test]
    fn empty_support_uv_phases_skip_at_local_geometry_work_limit() {
        crate::test_support::with_decode_context(|ctx| {
            let geometry_budget = GeometryWorkBudget::from_context(ctx, 0);
            assert!(!geometry_budget.charge());
            let support_budget = cadmpeg_core::decode::WorkBudget::new(100);
            let mut ir = CadIr::empty();
            super::complete_ext11_support_uv_with_budget(&mut ir, &[], &geometry_budget)
                .expect("an exhausted local slice skips serialized completion");
            super::invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
                ctx,
                &mut ir,
                &[],
                &std::collections::BTreeSet::new(),
                &support_budget,
                &geometry_budget,
                false,
            )
            .expect("an exhausted local slice skips lane validation");
            assert!(ctx.resource_refusal().is_none());
        });
    }
}
