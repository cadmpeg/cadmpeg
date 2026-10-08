// SPDX-License-Identifier: Apache-2.0
//! EXT11 support-UV assignment, completion, and equivalent-parameter transfer.

use super::blend::{
    blend_boundary_parameter_from_contact_pcurve_with_geometry_and_budget,
    blend_support_parameter_from_source_pcurve_with_index_and_budget_and_seed_cache,
    blend_surface_definition_with_index, blend_surface_parameters_for_fit_with_grid_and_budget,
    blend_surface_parameters_for_fit_with_source_continuation_and_budget,
    blend_surface_parameters_from_grid_for_fit_and_budget,
    blend_surface_parameters_from_grid_for_fit_with_source_continuation_and_budget,
    blend_surface_parameters_from_point_with_index_and_budget,
    decoded_surface_point_with_geometry_and_budget, spine_contact_pcurve_with_index,
    BlendContactSeedCache, BlendParameterGrid, BlendParameterGridCache, BoundaryInverseTarget,
    CircularBlendDefinition,
};
use super::emit::{
    procedural_curve_owners, push_endpoint_witness, EndpointWitnessOperations,
};
use super::geometry_work::GeometryWorkBudget;
use super::offset::{
    coarse_model_surface_parameters,
    continue_surface_intersection_parameters_with_index_and_seeds_and_budget_and_grid_cache,
    offset_surface_parameters_with_tolerance_with_index_and_budget,
    refine_offset_surface_parameters_with_index_and_budget, surface_parameter_domain_with_index,
    surface_parameters, ParameterLanes,
};
use super::pcurves::{
    blend_boundary_parameter_from_support_spine_with_index_and_budget,
    endpoint_witness_for_candidate, linear_nurbs_curve_endpoint_witness_with_index,
    pcurve_edge_endpoint_contract_with_index, pcurve_matches_edge_endpoint_contract,
    pcurve_surface_endpoints_with_index_and_budget, procedural_curve_owner,
    procedural_curve_position, procedural_curve_positions,
    surface_parameters_for_fit_with_index_and_budget, EndpointWitnesses,
};
use super::MISSING_TOLERANCE;
use crate::framing::node_kind::NodeKind;
use crate::framing::xmt_reference::NonNullXmt;
use crate::intersection::{SupportUv, SupportUvLane};
use crate::topology::Graph;
use cadmpeg_core::decode::{work_units, DecodeContext, ScopedReservation, WorkBudget};
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    analytic_surface_parameters, finite_or_refusal,
    nurbs_surface_parameter_within_tolerance_with_budget,
};
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

pub(super) fn support_uv_completion_budget_limit(
    ctx: &DecodeContext<'_>,
    chart_count: usize,
) -> Result<usize, cadmpeg_core::CodecError> {
    let requested = chart_count
        .checked_mul(SUPPORT_UV_COMPLETION_SAMPLES_PER_CHART)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "nx support UV completion samples",
                u64::MAX,
                cadmpeg_core::decode::u64_from_index(chart_count),
            )
        })?;
    // These bounds set the admitted work slice after its size is checked.
    Ok(match requested {
        requested if requested < MIN_SUPPORT_UV_COMPLETION_SAMPLES => {
            MIN_SUPPORT_UV_COMPLETION_SAMPLES
        }
        requested if requested > MAX_SUPPORT_UV_COMPLETION_SAMPLES => {
            MAX_SUPPORT_UV_COMPLETION_SAMPLES
        }
        requested => requested,
    })
}

type SupportUvBudget<'a> = WorkBudget<'a>;

pub(super) fn support_uv_budget_exhausted(budget: &SupportUvBudget<'_>) -> bool {
    budget.exhausted() || budget.remaining() == 0
}

fn refuse_geometry_work(
    budget: &GeometryWorkBudget<'_>,
) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
    budget.resource_refusal().map_or(Ok(()), Err)
}

fn support_uv_lane_geometry_work_limit(
    ctx: &DecodeContext<'_>,
    sample_count: usize,
    remaining: usize,
) -> Result<usize, cadmpeg_core::CodecError> {
    let requested = sample_count
        .checked_mul(SUPPORT_UV_GEOMETRY_WORK_PER_SAMPLE)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "nx support UV lane geometry work",
                u64::MAX,
                cadmpeg_core::decode::u64_from_index(sample_count),
            )
        })?;
    let admitted = match requested {
        requested if requested < MIN_SUPPORT_UV_LANE_GEOMETRY_WORK => {
            MIN_SUPPORT_UV_LANE_GEOMETRY_WORK
        }
        requested if requested > MAX_SUPPORT_UV_LANE_GEOMETRY_WORK => {
            MAX_SUPPORT_UV_LANE_GEOMETRY_WORK
        }
        requested => requested,
    };
    Ok(if remaining < admitted {
        remaining
    } else {
        admitted
    })
}

#[cfg(test)]
fn new_support_uv_budget() -> SupportUvBudget<'static> {
    WorkBudget::new(MAX_SUPPORT_UV_SAMPLES)
}

pub(super) fn linear_knots(
    parameters: &[f64],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
    if parameters.is_empty() {
        return Ok(Vec::new());
    }
    let ctx = geometry_budget.charges;
    let count = parameters
        .len()
        .checked_add(2)
        .ok_or_else(|| ctx.refuse_codec_limit("nx linear knot count", u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(count),
        "form nx linear knots",
    )?;
    let mut knots = ctx.collection_vec(count, "nx linear knots")?;
    knots.extend(parameters.first().copied());
    knots.extend_from_slice(parameters);
    knots.extend(parameters.last().copied());
    Ok(knots)
}

fn linear_pcurve_geometry(
    ctx: &DecodeContext<'_>,
    parameters: &[f64],
    controls: &[Point2],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<PcurveGeometry, cadmpeg_core::CodecError> {
    let mut points = ctx.collection_vec(controls.len(), "nx support UV admitted controls")?;
    for control in ctx.admit_iter(controls, "nx support UV control admission")? {
        points.push(FinitePoint2::new(*control).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("control_points contains a non-finite point")
        })?);
    }
    let knots = cadmpeg_ir::geometry::nurbs::KnotVector::new(
        ctx,
        linear_knots(parameters, geometry_budget)?,
    )??;
    let nurbs = cadmpeg_ir::geometry::pcurve::PcurveNurbs::new(
        ctx,
        1,
        knots,
        cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Polynomial { points },
        false,
    )??;
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
    ctx: &DecodeContext<'_>,
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

    let mut surface_ids = [None, None];
    for (support, surface_id) in supports.into_iter().zip(&mut surface_ids) {
        let Some(support) = support else {
            continue;
        };
        *surface_id = ctx.get_btree_map(
            surfaces_by_xmt,
            &u32::from(support),
            "nx EXT11 support identity lookup",
        )?;
    }
    let [Some(first_surface), Some(second_surface)] = surface_ids else {
        return Ok(None);
    };
    assign_ext11_support_uv_to_surfaces_with_index(
        ctx,
        index,
        [first_surface, second_surface],
        points,
        fit_tolerance,
        lanes,
        geometry_budget,
    )
}

// Keep the object-map, serialized lanes, and shared geometry budget explicit:
// validation must preserve the same support identity proof as assignment.
pub(super) fn validate_serialized_support_uv_with_index(
    ctx: &DecodeContext<'_>,
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
        let Some(support) = supports[side] else {
            continue;
        };
        let Some(surface) = ctx.get_btree_map(
            surfaces_by_xmt,
            &u32::from(support),
            "nx serialized support identity lookup",
        )?
        else {
            continue;
        };
        let Some(values) = lanes[side].as_ref() else {
            continue;
        };
        let tolerance = blend_spine_cache_fit_tolerance_with_index(
            index,
            surface,
            fit_tolerance,
            geometry_budget.charges,
        )?;
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
                    ctx.copy_slice(values.as_slice(), "NX solved support-UV lane copy")?,
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
        .surfaces(surface.as_str(), geometry_budget.charges)?
        .map(|surface| &surface.geometry)
    else {
        return Ok(false);
    };
    for sample in geometry_budget.charges.admit_iter(
        &(0..values.len().min(points.len())),
        "nx support UV lane fit traversal",
    )? {
        let (uv, point) = (&values[sample], &points[sample]);
        if geometry_budget.exhausted() {
            refuse_geometry_work(geometry_budget)?;
            return Ok(false);
        }
        if uv.iter().any(|value| missing_support_parameter(*value)) {
            return Ok(false);
        }
        let Some(uv) = surface_parameters(geometry_budget.charges, geometry, **uv)? else {
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
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)
        .expect("decode index allocation succeeds");
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK),
    );

    assign_ext11_support_uv_to_surfaces_with_index(
        ctx,
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
    ctx: &DecodeContext<'_>,
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
        if assigned_lanes[support].is_some() {
            return Ok(None);
        }
        assigned_lanes[support] = Some(lane);
    }
    let distinct_surfaces = !ctx.equal(
        surfaces[0].as_str(),
        surfaces[1].as_str(),
        "nx EXT11 support identity comparison",
    )?;
    if distinct_surfaces && assigned_lanes.iter().filter(|lane| lane.is_some()).count() == 1 {
        let Some(assigned_support) = assigned_lanes.iter().position(Option::is_some) else {
            return Ok(None);
        };
        let Some(assigned_lane) = assigned_lanes[assigned_support] else {
            return Ok(None);
        };
        let other_support = 1 - assigned_support;
        let other_lane = 1 - assigned_lane;
        if lane_matches_surface(surfaces[other_support], other_lane)? {
            assigned_lanes[other_support] = Some(other_lane);
        }
    }
    let mut assigned = [None, None];
    for lane in 0..2 {
        let Some(support) = assigned_lanes.iter().position(|assigned| *assigned == Some(lane)) else {
            continue;
        };
        let Some(lane) = lanes[lane].as_ref() else {
            continue;
        };
        assigned[support] = Some(
            crate::intersection::SupportUvLane::from_checked(
                ctx.copy_slice(lane.as_slice(), "NX solved support-UV lane copy")?,
                lane.as_slice().len(),
            )
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("NX copied support-UV lane count")
            })?,
        );
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
pub(super) fn validated_support_uv_endpoint_witnesses<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    pending: &[PendingExt11SupportUv],
    validated_lanes: &BTreeSet<(ProceduralCurveId, usize)>,
    endpoint_witnesses: &mut EndpointWitnesses,
    endpoint_witness_storage: &mut ScopedReservation<'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    let (procedural_by_id, _procedural_storage) = ctx.unique_index(
        ir.model
            .procedural_curves
            .iter()
            .map(|procedural| (procedural.id.as_str(), procedural)),
        "nx validated procedural index",
    )?;
    let (owners, _owner_storage) = procedural_curve_owners(ctx, &ir.model.curves)?;
    for (procedural_id, samples, _, _) in ctx.admit_iter(pending, "nx validated lane traversal")? {
        let Some(procedural) = ctx
            .get_hash_map(
                &procedural_by_id,
                procedural_id.as_str(),
                "nx validated procedural lookup",
            )?
            .copied()
            .flatten()
        else {
            continue;
        };
        let Some(owner) = procedural_curve_owner(ctx, &owners, &procedural.id)? else {
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
            if !validated_lane(ctx, validated_lanes, procedural_id, side)?
                || pcurve_requires_completion(
                    ctx,
                    support.pcurve.as_ref().map(|pcurve| &pcurve.geometry),
                )?
            {
                continue;
            }
            let Some(surface) = &support.surface else {
                continue;
            };
            let Some(pcurve) = &support.pcurve else {
                continue;
            };
            push_endpoint_witness(
                ctx,
                endpoint_witness_storage,
                endpoint_witnesses,
                owner,
                surface,
                &pcurve.geometry,
                context.parameter_range().endpoints(),
                samples.endpoints(),
                EndpointWitnessOperations {
                    lookup_key: "nx validated witness lookup key",
                    curve: "nx validated witness owner",
                    support: "nx validated witness surface",
                    pcurve: "nx validated witness pcurve",
                    index: "nx validated witness index",
                    witnesses: "nx validated endpoint witnesses",
                },
            )?;
        }
    }
    Ok(())
}

pub(super) fn missing_support_parameter(value: f64) -> bool {
    value.to_bits() == MISSING_TOLERANCE.to_bits()
}

/// Whether a lane still needs a support pcurve: it has none, or a NURBS pcurve
/// whose control points carry the missing-parameter marker. The marker search
/// reads the poles in place, charges each pole it reads and stops at the first
/// marker.
pub(super) fn pcurve_requires_completion(
    ctx: &DecodeContext<'_>,
    pcurve: Option<&PcurveGeometry>,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    match pcurve {
        None => Ok(true),
        Some(PcurveGeometry::Nurbs { nurbs }) => {
            let poles = nurbs.pole_rows();
            for index in 0..poles.count() {
                ctx.charge_work_limit(1, "nx support UV missing parameter search")?;
                if poles.point_at(index).is_some_and(|point| {
                    missing_support_parameter(point.u) || missing_support_parameter(point.v)
                }) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Some(_) => Ok(false),
    }
}

/// Whether `validated_lanes` holds the lane `side` of `procedural`. The lookup
/// key is a temporary copy of the identity, held under its own scoped
/// reservation.
fn validated_lane(
    ctx: &DecodeContext<'_>,
    validated_lanes: &BTreeSet<(ProceduralCurveId, usize)>,
    procedural: &ProceduralCurveId,
    side: usize,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut key_storage = ctx.reserve_scoped(0, "nx validated lane lookup key")?;
    let key = key_storage.with_storage(|| {
        Ok::<_, cadmpeg_core::CodecError>((
            procedural.try_clone_for_decode(ctx, "nx validated lane lookup key")?,
            side,
        ))
    })?;
    ctx.contains_btree_set(validated_lanes, &key, "nx validated lane lookup")
}

fn pcurve_control_point_seed(pcurve: Option<&PcurveGeometry>, index: usize) -> Option<Point2> {
    let PcurveGeometry::Nurbs { nurbs } = pcurve? else {
        return None;
    };
    nurbs
        .pole_rows()
        .point_at(index)
        .map(FinitePoint2::get)
        .filter(|point| !missing_support_parameter(point.u) && !missing_support_parameter(point.v))
}

fn serialized_support_uv_seed_candidates(
    ctx: &DecodeContext<'_>,
    geometry: &SurfaceGeometry,
    serialized: &SerializedSupportUv,
    side: usize,
    point_index: usize,
) -> Result<[Option<Point2>; 4], cadmpeg_core::decode::ResourceLimit> {
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
            *output = surface_parameters(ctx, geometry, [u, v])?.map(FinitePoint2::get);
        }
    }
    Ok(candidates)
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
        surface_parameter_domain_with_index(index, surface_id, geometry_budget.charges)?
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
    nurbs_surface_parameter_within_tolerance_with_budget(
        geometry_budget.charges,
        nurbs,
        point,
        coarse,
        fit_tolerance,
        geometry_budget,
    )
    .map(|parameter| parameter.map(FinitePoint2::get))
}

fn serialized_support_uv_seed_for_side(
    ctx: &DecodeContext<'_>,
    geometry: &SurfaceGeometry,
    serialized: &SerializedSupportUv,
    side: usize,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
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
            if let Some(point) = surface_parameters(ctx, geometry, [u, v])? {
                return Ok(Some(point.get()));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
pub(super) fn complete_ext11_support_uv(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
) -> Result<(), cadmpeg_core::CodecError> {
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK),
    );

    complete_ext11_support_uv_with_budget(ctx, ir, pending, &geometry_budget)
}

pub(super) fn complete_ext11_support_uv_with_budget(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut replacements = Vec::new();
    let mut replacement_storage = ctx.reserve_scoped(0, "nx serialized support UV replacements")?;
    {
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
        let (positions, _position_storage) = procedural_curve_positions(ctx, ir)?;
        for (procedural_id, samples, fit_tolerance, serialized) in
            ctx.admit_iter(pending, "nx serialized support UV traversal")?
        {
            let points = &samples.points_charged(ctx)?;
            let parameters = &samples.parameters_charged(ctx)?;
            let Some(procedural) =
                model_index.procedural_curves(procedural_id.as_str(), geometry_budget.charges)?
            else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
            else {
                continue;
            };
            let [first, second] = context.sides();
            let (Some(first_surface), Some(second_surface)) =
                (first.surface.as_ref(), second.surface.as_ref())
            else {
                continue;
            };
            let surfaces = [first_surface, second_surface];
            let missing = [
                pcurve_requires_completion(
                    ctx,
                    first.pcurve.as_ref().map(|pcurve| &pcurve.geometry),
                )?,
                pcurve_requires_completion(
                    ctx,
                    second.pcurve.as_ref().map(|pcurve| &pcurve.geometry),
                )?,
            ];
            if !missing.into_iter().any(|missing| missing) {
                continue;
            }
            let Some(assigned) = assign_ext11_support_uv_to_surfaces_with_index(
                ctx,
                &model_index,
                surfaces,
                points,
                *fit_tolerance,
                &serialized.ext11,
                geometry_budget,
            )?
            else {
                continue;
            };
            let Some(position) = procedural_curve_position(ctx, &positions, procedural_id)? else {
                continue;
            };
            for side in 0..2 {
                if !missing[side] {
                    continue;
                }
                let Some(surface_geometry) = model_index
                    .surfaces(surfaces[side].as_str(), geometry_budget.charges)?
                    .map(|surface| &surface.geometry)
                else {
                    continue;
                };
                let Some(values) = assigned[side].as_ref() else {
                    continue;
                };
                if ctx.any_by(
                    values.as_slice(),
                    |pair| Ok(pair.iter().any(|value| missing_support_parameter(*value))),
                    "nx serialized support UV missing parameter search",
                )? {
                    continue;
                }
                let mut control_storage =
                    ctx.reserve_scoped(0, "nx serialized support UV controls")?;
                let mut controls = Vec::new();
                ctx.reserve_scoped_vec(
                    &mut control_storage,
                    &mut controls,
                    values.len(),
                    "nx serialized support UV controls",
                )?;
                let mut valid = true;
                for uv in ctx.admit_iter(values.as_slice(), "nx serialized support UV controls")? {
                    if let Some(point) = surface_parameters(ctx, surface_geometry, **uv)? {
                        controls.push(point.get());
                    } else {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    continue;
                }
                let replacement =
                    linear_pcurve_geometry(ctx, parameters, &controls, geometry_budget)?;
                ctx.reserve_scoped_vec(
                    &mut replacement_storage,
                    &mut replacements,
                    1,
                    "nx serialized support UV replacements",
                )?;
                replacements.push((position, side, replacement));
            }
        }
    }
    for (position, side, replacement) in ctx.admit_iter(
        replacements,
        "nx serialized support UV replacement traversal",
    )? {
        let Some(context) = ir
            .model
            .procedural_curves
            .get_mut(position)
            .and_then(|procedural| procedural.intersection_context_mut())
        else {
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
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK),
    );
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
    let ctx = geometry_budget.charges;
    let mut endpoint_witness_storage =
        ctx.reserve_scoped(0, "nx support UV endpoint witnesses")?;
    let mut endpoint_witnesses = BTreeMap::new();
    complete_support_uv_with_budget_and_endpoint_witnesses(
        ctx,
        ir,
        pending,
        (support_budget, geometry_budget),
        (coupled_support_budget, coupled_geometry_budget),
        &mut endpoint_witnesses,
        &mut endpoint_witness_storage,
    )
}

pub(super) fn complete_support_uv_with_budget_and_endpoint_witnesses<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    direct_budgets: (&SupportUvBudget<'_>, &GeometryWorkBudget<'_>),
    coupled_budgets: (&SupportUvBudget<'_>, &GeometryWorkBudget<'_>),
    endpoint_witnesses: &mut EndpointWitnesses,
    endpoint_witness_storage: &mut ScopedReservation<'ctx>,
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
    // Completion edits procedurals in place, so the pending procedurals keep
    // the arena positions resolved here across every wave.
    let (pending_positions, _pending_storage) = {
        let (positions, _position_storage) = procedural_curve_positions(ctx, ir)?;
        let (mut pending_positions, pending_storage) =
            ctx.temporary_vec(pending.len(), "nx pending support lane positions")?;
        for (procedural_id, ..) in ctx.admit_iter(pending, "nx pending support lane positions")? {
            pending_positions.push(procedural_curve_position(ctx, &positions, procedural_id)?);
        }
        (pending_positions, pending_storage)
    };
    loop {
        ctx.charge_work(1, "nx support UV completion wave pass")?;
        let before = pending_support_lanes_requiring_completion(ctx, ir, &pending_positions)?;
        if support_uv_budget_exhausted(support_budget) {
            break;
        }
        geometry_budget.clear_blend_frame_cache();
        coupled_geometry_budget.clear_blend_frame_cache();
        lane_geometry_exhausted |= complete_support_uv_wave(
            ctx,
            ir,
            SupportUvAttempts {
                pending,
                failed_attempts: &mut failed_attempts,
                failed_coupled_attempts: &mut failed_coupled_attempts,
                endpoint_witnesses,
                endpoint_witness_storage,
            },
            support_budget,
            geometry_budget,
            coupled_support_budget,
            coupled_geometry_budget,
        )?;
        let after = pending_support_lanes_requiring_completion(ctx, ir, &pending_positions)?;
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
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    let support_budget = WorkBudget::new(MAX_SUPPORT_UV_SAMPLES);

    let mut endpoint_witness_storage = ctx
        .reserve_scoped(0, "nx support UV endpoint witnesses")
        .expect("evaluator allocation succeeds");
    let mut endpoint_witnesses = EndpointWitnesses::new();
    invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
        ctx,
        ir,
        pending,
        &BTreeSet::new(),
        &support_budget,
        &geometry_budget,
        false,
        &mut endpoint_witnesses,
        &mut endpoint_witness_storage,
    )
    .expect("evaluator allocation succeeds");
}

/// Invalidate support lanes that disagree with their surface and retain
/// endpoint witnesses only for lanes whose complete sample set was evaluated.
pub(in crate::decode) struct SupportUvValidationResult {
    pub(super) lane_geometry_exhausted: bool,
}

pub(super) fn invalidate_inconsistent_support_uv_with_validated_lanes_and_status<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    validated_lanes: &BTreeSet<(ProceduralCurveId, usize)>,
    support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    isolate_lanes: bool,
    endpoint_witnesses: &mut EndpointWitnesses,
    endpoint_witness_storage: &mut ScopedReservation<'ctx>,
) -> Result<SupportUvValidationResult, cadmpeg_core::CodecError> {
    let mut invalid_storage = ctx.reserve_scoped(0, "nx inconsistent support UV lanes")?;
    let (invalid, lane_geometry_exhausted) = {
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
        let (owners, _owner_storage) = procedural_curve_owners(ctx, &ir.model.curves)?;
        let (positions, _position_storage) = procedural_curve_positions(ctx, ir)?;
        let mut invalid = Vec::new();
        let mut lane_geometry_exhausted = false;
        for (procedural_id, samples, fit_tolerance, _) in
            ctx.admit_iter(pending, "nx support UV validation traversal")?
        {
            let points = &samples.points_charged(ctx)?;
            let parameters = &samples.parameters_charged(ctx)?;
            if geometry_budget.exhausted() {
                refuse_geometry_work(geometry_budget)?;
                break;
            }
            if support_uv_budget_exhausted(support_budget) {
                break;
            }
            let Some(procedural) =
                index.procedural_curves(procedural_id.as_str(), geometry_budget.charges)?
            else {
                continue;
            };
            let Some(owner) = procedural_curve_owner(ctx, &owners, &procedural.id)? else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
            else {
                continue;
            };
            for (side, support) in context.sides().iter().enumerate() {
                if geometry_budget.exhausted() {
                    refuse_geometry_work(geometry_budget)?;
                    break;
                }
                if support_uv_budget_exhausted(support_budget) {
                    break;
                }
                if validated_lane(ctx, validated_lanes, procedural_id, side)? {
                    continue;
                }
                let (Some(surface), Some(pcurve)) = (&support.surface, &support.pcurve) else {
                    continue;
                };
                let Some(geometry) = index
                    .surfaces(surface.as_str(), geometry_budget.charges)?
                    .map(|surface| &surface.geometry)
                else {
                    continue;
                };
                let tolerance = blend_spine_cache_fit_tolerance_with_index(
                    &index,
                    surface,
                    *fit_tolerance,
                    geometry_budget.charges,
                )?;
                let parent_geometry_budget = geometry_budget;
                let lane_geometry_budget = if isolate_lanes {
                    Some(
                        parent_geometry_budget.child_slice(support_uv_lane_geometry_work_limit(
                            ctx,
                            points.len(),
                            parent_geometry_budget.remaining(),
                        )?),
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
                        refuse_geometry_work(geometry_budget)?;
                        fully_validated = false;
                        break;
                    }
                    if !support_budget.charge() {
                        if let Some(limit) = ctx.resource_refusal() {
                            return Err(limit.into());
                        }
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
                    refuse_geometry_work(lane_geometry_budget)?;
                    refuse_geometry_work(parent_geometry_budget)?;
                    lane_geometry_exhausted |= child_exhausted || parent_exhausted;
                }
                if inconsistent {
                    if let Some(position) =
                        procedural_curve_position(ctx, &positions, procedural_id)?
                    {
                        ctx.reserve_scoped_vec(
                            &mut invalid_storage,
                            &mut invalid,
                            1,
                            "nx inconsistent support UV lanes",
                        )?;
                        invalid.push((position, side));
                    }
                } else if fully_validated {
                    if let [Some(first), Some(last)] = endpoints {
                        push_endpoint_witness(
                            ctx,
                            endpoint_witness_storage,
                            endpoint_witnesses,
                            owner,
                            surface,
                            &pcurve.geometry,
                            context.parameter_range().endpoints(),
                            [first, last],
                            EndpointWitnessOperations {
                                lookup_key: "nx validated endpoint lookup key",
                                curve: "nx validated endpoint owner",
                                support: "nx validated endpoint support",
                                pcurve: "nx validated endpoint pcurve",
                                index: "nx validated endpoint index",
                                witnesses: "nx validated endpoint witnesses",
                            },
                        )?;
                    }
                }
            }
        }
        (invalid, lane_geometry_exhausted)
    };
    for &(position, side) in ctx.admit_iter(&invalid, "nx inconsistent support UV traversal")? {
        let Some(context) = ir
            .model
            .procedural_curves
            .get_mut(position)
            .and_then(|procedural| procedural.intersection_context_mut())
        else {
            continue;
        };
        context.set_unmapped_pcurve(side, None);
    }
    Ok(SupportUvValidationResult {
        lane_geometry_exhausted,
    })
}

/// The lanes still requiring a support pcurve among the pending procedurals at
/// `positions`.
fn pending_support_lanes_requiring_completion(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    positions: &[Option<usize>],
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut count = 0usize;
    for position in ctx.admit_iter(positions, "NX pending support lane scan")? {
        let Some(procedural) =
            position.and_then(|position| ir.model.procedural_curves.get(position))
        else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        for side in context.sides() {
            if pcurve_requires_completion(ctx, side.pcurve.as_ref().map(|pcurve| &pcurve.geometry))?
            {
                count = count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("NX pending support lane count", u64::MAX - 1, u64::MAX)
                })?;
            }
        }
    }
    Ok(count)
}

// Keep independent work budgets, retry state, and the witness sink explicit at
// this completion boundary.

struct SupportUvAttempts<'inputs, 'ctx> {
    pending: &'inputs [PendingExt11SupportUv],
    failed_attempts: &'inputs mut BTreeMap<(ProceduralCurveId, usize), Option<PcurveGeometry>>,
    failed_coupled_attempts:
        &'inputs mut BTreeMap<ProceduralCurveId, [Option<cadmpeg_ir::geometry::SupportPcurve>; 2]>,
    endpoint_witnesses: &'inputs mut EndpointWitnesses,
    endpoint_witness_storage: &'inputs mut ScopedReservation<'ctx>,
}

fn complete_support_uv_wave<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &mut CadIr,
    support_uv_attempts: SupportUvAttempts<'_, 'ctx>,
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
        endpoint_witness_storage,
    } = support_uv_attempts;

    let mut lane_geometry_exhausted = false;
    let geometry_exhausted = geometry_budget.exhausted();
    refuse_geometry_work(geometry_budget)?;
    if !support_uv_budget_exhausted(support_budget) && !geometry_exhausted {
        let mut replacements = Vec::new();
        let mut replacement_storage = ctx.reserve_scoped(0, "nx support UV replacements")?;
        {
            let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
            let (owners, _owner_storage) = procedural_curve_owners(ctx, &ir.model.curves)?;
            let (positions, _position_storage) = procedural_curve_positions(ctx, ir)?;
            let (cache_backed_constructions, _cache_backed_storage) = ctx
                .collect_scoped_string_set(
                    0,
                    ctx.admit_iter(&ir.model.curves, "nx support UV cache backed curve scan")?
                        .filter(|curve| curve.geometry.solved_cache().is_some())
                        .filter_map(|curve| {
                            curve
                                .geometry
                                .procedural_construction()
                                .map(ProceduralCurveId::as_str)
                        }),
                    "nx support UV cache backed constructions",
                )?;
            let mut blend_parameter_grids = BlendParameterGridCache::new(ctx)?;
            for (procedural_id, samples, fit_tolerance, serialized) in
                ctx.admit_iter(pending, "nx support UV completion traversal")?
            {
                if support_uv_budget_exhausted(support_budget) {
                    break;
                }
                let points = &samples.points_charged(ctx)?;
                let parameters = &samples.parameters_charged(ctx)?;
                let Some(procedural) = model_index
                    .procedural_curves(procedural_id.as_str(), geometry_budget.charges)?
                else {
                    continue;
                };
                let Some(owner) = procedural_curve_owner(ctx, &owners, &procedural.id)? else {
                    continue;
                };
                let ProceduralCurveDefinition::Intersection { context, .. } =
                    procedural.definition()
                else {
                    continue;
                };
                let missing = [
                    pcurve_requires_completion(
                        ctx,
                        context.sides()[0]
                            .pcurve
                            .as_ref()
                            .map(|pcurve| &pcurve.geometry),
                    )?,
                    pcurve_requires_completion(
                        ctx,
                        context.sides()[1]
                            .pcurve
                            .as_ref()
                            .map(|pcurve| &pcurve.geometry),
                    )?,
                ];
                for side in 0..2 {
                    if support_uv_budget_exhausted(support_budget) {
                        break;
                    }
                    if !missing[side] {
                        continue;
                    }
                    let Some(surface_id) = &context.sides()[side].surface else {
                        continue;
                    };
                    let attempt_key = (
                        procedural_id.try_clone_for_decode(ctx, "nx support UV retry identity")?,
                        side,
                    );
                    let source_pcurve = context.sides()[1 - side].pcurve.as_ref();
                    let other_surface_id = context.sides()[1 - side].surface.as_ref();
                    if ctx
                        .get_btree_map(failed_attempts, &attempt_key, "nx support UV retry lookup")?
                        .is_some_and(|previous| {
                            previous.as_ref() == source_pcurve.map(|pcurve| &pcurve.geometry)
                        })
                    {
                        continue;
                    }
                    let Some(surface) =
                        model_index.surfaces(surface_id.as_str(), geometry_budget.charges)?
                    else {
                        continue;
                    };
                    // An absent pcurve requires completion, so an available
                    // source chart is also a present one.
                    let source_chart_available = !missing[1 - side];
                    let linear_offset_surface = match &surface.geometry {
                        SurfaceGeometry::Procedural { construction, .. } => model_index
                            .procedural_surfaces(construction.as_str(), geometry_budget.charges)?
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
                        geometry_budget.charges,
                    )?;
                    let other_support = match context.sides()[1 - side]
                        .surface
                        .as_ref()
                        .zip(context.sides()[1 - side].pcurve.as_ref())
                    {
                        Some((other_surface, other_pcurve)) => model_index
                            .surfaces(other_surface.as_str(), geometry_budget.charges)?
                            .map(|surface| {
                                (other_surface, &other_pcurve.geometry, &surface.geometry)
                            }),
                        None => None,
                    };
                    let other_contact = (|| -> Result<_, cadmpeg_core::decode::ResourceLimit> {
                        let Some((other_surface, other_pcurve, other_geometry)) = other_support
                        else {
                            return Ok(None);
                        };
                        let Some(CircularBlendDefinition {
                            supports,
                            spine,
                            radius,
                            ..
                        }) = blend_surface_definition_with_index(
                            &model_index,
                            surface_id,
                            geometry_budget.charges,
                        )?
                        else {
                            return Ok(None);
                        };
                        let mut selected = None;
                        for (boundary, candidate) in supports.into_iter().enumerate() {
                            if parameterization_equivalent_surfaces_with_index(
                                &model_index,
                                candidate,
                                other_surface,
                                geometry_budget.charges,
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
                            geometry_budget.charges,
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
                            ctx,
                            points.len(),
                            parent_geometry_budget.remaining(),
                        )?);
                    let geometry_budget = &lane_geometry_budget;
                    let mut contact_seeds = BlendContactSeedCache::default();
                    let mut uv_storage =
                        ctx.reserve_scoped(0, "nx support UV fitted parameters")?;
                    let uv =
                        (|| -> Result<Option<(Vec<Point2>, bool)>, cadmpeg_core::CodecError> {
                            let mut uv = Vec::new();
                            let mut all_parameters_certified = true;
                            for (point_index, point) in points.iter().enumerate() {
                                if !support_budget.charge() {
                                    if let Some(limit) = ctx.resource_refusal() {
                                        return Err(limit.into());
                                    }
                                    return Ok(None);
                                }
                                let serialized_seeds = serialized_support_uv_seed_candidates(
                                    ctx,
                                    &surface.geometry,
                                    serialized,
                                    side,
                                    point_index,
                                )?;
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
                                    let sample_parameter = parameters[point_index];
                                    let candidate = match &surface.geometry {
                                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                                            nurbs,
                                        )) => {
                                            if let Some(seed) = seed {
                                                nurbs_surface_parameter_within_tolerance_with_budget(
                                            geometry_budget.charges,
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
                                            let mut parameters = None;
                                            if source_chart_available {
                                                if let Some((source_pcurve, source_surface)) =
                                                    source_pcurve.zip(other_surface_id)
                                                {
                                                    parameters = blend_support_parameter_from_source_pcurve_with_index_and_budget_and_seed_cache(
&model_index,
&crate::decode::blend::SourcePcurveSample { blend: source_surface, support: surface_id, source_pcurve: &source_pcurve.geometry, curve_parameter: sample_parameter },
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
                                                    parameters = blend_boundary_parameter_from_contact_pcurve_with_geometry_and_budget(
&model_index,
&crate::decode::blend::ContactCurveSample { support: other_surface, support_geometry: other_geometry, contact_pcurve, boundary, support_pcurve: other_pcurve, curve_parameter: sample_parameter },
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
                                                parameters = if source_chart_available {
                                                    blend_surface_parameters_for_fit_with_source_continuation_and_budget(
                                                    &model_index,
                                                    surface_id,
                                                    *point,
                                                    seed,
                                                    effective_fit_tolerance,
                                                    BlendParameterGrid::Disabled,
                                                    geometry_budget,
                                                                                                    )?
                                                } else {
                                                    blend_surface_parameters_for_fit_with_grid_and_budget(
                                                    &model_index,
                                                    surface_id,
                                                    *point,
                                                    seed,
                                                    effective_fit_tolerance,
                                                    BlendParameterGrid::Disabled,
                                                    geometry_budget,
                                                                                                    )?
                                                };
                                            }
                                            if parameters.is_none() {
                                                if let BlendParameterGrid::Provided(grid) =
                                                    blend_parameter_grids.grid(
                                                        &model_index,
                                                        surface_id,
                                                        geometry_budget,
                                                    )?
                                                {
                                                    parameters = if source_chart_available {
                                                        blend_surface_parameters_from_grid_for_fit_with_source_continuation_and_budget(
                                                        &model_index,
                                                        surface_id,
                                                        *point,
                                                        effective_fit_tolerance,
                                                        grid,
                                                        geometry_budget,
                                                                                                            )?
                                                    } else {
                                                        blend_surface_parameters_from_grid_for_fit_and_budget(
                                                        &model_index,
                                                        surface_id,
                                                        *point,
                                                        effective_fit_tolerance,
                                                        grid,
                                                        geometry_budget,
                                                                                                            )?
                                                    };
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
                                ctx.reserve_scoped_vec(
                                    &mut uv_storage,
                                    &mut uv,
                                    1,
                                    "nx support UV fitted parameters",
                                )?;
                                uv.push(parameters);
                            }
                            Ok(Some((uv, all_parameters_certified)))
                        })();
                    let Some((mut uv, all_parameters_certified)) = uv? else {
                        let parent_exhausted = parent_geometry_budget
                            .consume_child(&lane_geometry_budget)
                            .is_err();
                        let child_exhausted = lane_geometry_budget.exhausted();
                        refuse_geometry_work(&lane_geometry_budget)?;
                        refuse_geometry_work(parent_geometry_budget)?;
                        lane_geometry_exhausted |= child_exhausted || parent_exhausted;
                        ctx.insert_btree_map(
                            failed_attempts,
                            attempt_key,
                            source_pcurve
                                .map(|pcurve| {
                                    pcurve
                                        .geometry
                                        .try_clone_for_decode(ctx, "nx support UV retry pcurve")
                                })
                                .transpose()?,
                            "nx support UV failed retries",
                        )?;
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
                        for index in ctx.admit_iter(&(1..uv.len()), "nx support UV turn lifting")? {
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
                        for sample_index in ctx.admit_iter(
                            &(0..uv.len().min(points.len())),
                            "nx support UV chart reproduction",
                        )? {
                            let (sample_uv, point) = (&uv[sample_index], &points[sample_index]);
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
                        let pcurve = linear_pcurve_geometry(ctx, parameters, &uv, geometry_budget)?;
                        if let [Some(first), Some(last)] = endpoint_values {
                            push_endpoint_witness(
                                ctx,
                                endpoint_witness_storage,
                                endpoint_witnesses,
                                owner,
                                surface_id,
                                &pcurve,
                                parameter_range,
                                [first, last],
                                EndpointWitnessOperations {
                                    lookup_key: "nx support UV witness lookup key",
                                    curve: "nx support UV witness owner",
                                    support: "nx support UV witness surface",
                                    pcurve: "nx support UV witness pcurve",
                                    index: "nx support UV witness index",
                                    witnesses: "nx support UV endpoint witnesses",
                                },
                            )?;
                        }
                        if let Some(position) =
                            procedural_curve_position(ctx, &positions, procedural_id)?
                        {
                            let cache_backed = ctx.contains_hash_set(
                                &cache_backed_constructions,
                                procedural.id.as_str(),
                                "nx support UV cache backed lookup",
                            )?;
                            ctx.reserve_scoped_vec(
                                &mut replacement_storage,
                                &mut replacements,
                                1,
                                "nx support UV replacements",
                            )?;
                            replacements.push((
                                position,
                                side,
                                pcurve,
                                admitted_fit_tolerance,
                                cache_backed,
                            ));
                        }
                    } else {
                        ctx.insert_btree_map(
                            failed_attempts,
                            attempt_key,
                            source_pcurve
                                .map(|pcurve| {
                                    pcurve
                                        .geometry
                                        .try_clone_for_decode(ctx, "nx support UV retry pcurve")
                                })
                                .transpose()?,
                            "nx support UV failed retries",
                        )?;
                    }
                    let parent_exhausted = parent_geometry_budget
                        .consume_child(&lane_geometry_budget)
                        .is_err();
                    let child_exhausted = lane_geometry_budget.exhausted();
                    refuse_geometry_work(&lane_geometry_budget)?;
                    refuse_geometry_work(parent_geometry_budget)?;
                    lane_geometry_exhausted |= child_exhausted || parent_exhausted;
                }
            }
        }
        for (position, side, pcurve, effective_fit_tolerance, cache_backed) in
            ctx.admit_iter(replacements, "nx support UV replacement traversal")?
        {
            let Some(procedural) = ir.model.procedural_curves.get_mut(position) else {
                continue;
            };
            let completed = procedural.edit_definition(
                |definition| -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
                    let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                        return Ok(false);
                    };
                    if pcurve_requires_completion(
                        ctx,
                        context.sides()[side]
                            .pcurve
                            .as_ref()
                            .map(|pcurve| &pcurve.geometry),
                    )? {
                        context.set_unmapped_pcurve(side, Some(pcurve));
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                },
            )?;
            if completed && cache_backed {
                procedural.require_cache_fit_tolerance(effective_fit_tolerance)?;
            }
        }
    }

    // Independent inverse admission is the cheapest certified route. Run
    // coupled continuation only after this wave has had a chance to fill the
    // same lanes, so difficult nested supports are reserved for residuals.
    let coupled_geometry_exhausted = coupled_geometry_budget.exhausted();
    refuse_geometry_work(coupled_geometry_budget)?;
    if !coupled_geometry_exhausted {
        coupled_geometry_budget.clear_blend_frame_cache();
        lane_geometry_exhausted |= complete_coupled_support_uv(
            ctx,
            ir,
            pending,
            coupled_support_budget,
            coupled_geometry_budget,
            failed_coupled_attempts,
            endpoint_witnesses,
            endpoint_witness_storage,
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
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, cadmpeg_ir::index::StandardIndex);
    blend_spine_cache_fit_tolerance_with_index(
        &index,
        surface,
        fit_tolerance,
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap()
}

pub(super) fn blend_spine_cache_fit_tolerance_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    fit_tolerance: f64,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<f64, cadmpeg_core::decode::ResourceLimit> {
    let tolerance = match blend_surface_definition_with_index(index, surface, ctx)? {
        Some(CircularBlendDefinition { spine, .. }) => index
            .procedural_curves_for_curve(spine.as_str(), ctx)?
            .and_then(|procedurals| procedurals.first().copied())
            .and_then(cadmpeg_ir::geometry::ProceduralCurve::cache_fit_tolerance),
        None => None,
    };
    Ok(tolerance
        .filter(|tolerance| tolerance.get() > 0.0)
        .map_or(fit_tolerance, |tolerance| fit_tolerance + tolerance.get()))
}

/// Fit both lanes of a blend-boundary intersection. The lanes come back with
/// the scoped reservation that holds their storage. The caller admits one
/// sample per point and missing lane through its coupled support budget.
fn complete_blend_boundary_support_uv_with_index_and_budget<'a, 'ctx>(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surfaces: [&'a SurfaceId; 2],
    points: &[Point3],
    fit_tolerance: f64,
    seeds: [Option<Point2>; 2],
    geometry_budget: &GeometryWorkBudget<'ctx>,
    blend_parameter_grids: &mut BlendParameterGridCache<'a, '_>,
) -> Result<Option<ParameterLanes<'ctx>>, cadmpeg_core::CodecError> {
    let ctx = geometry_budget.charges;
    let mut selected_sides = None;
    for blend_side in 0..2 {
        let Some(CircularBlendDefinition { supports, .. }) = blend_surface_definition_with_index(
            index,
            surfaces[blend_side],
            geometry_budget.charges,
        )?
        else {
            continue;
        };
        let mut support_matches = 0;
        for support in supports {
            if parameterization_equivalent_surfaces_with_index(
                index,
                support,
                surfaces[1 - blend_side],
                ctx,
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
    let mut lane_storage = ctx.reserve_scoped(0, "nx coupled support UV boundary lanes")?;
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
            blend_parameter_grids,
        )?
        else {
            return Ok(None);
        };
        for lane in &mut lanes {
            ctx.reserve_scoped_vec(
                &mut lane_storage,
                lane,
                1,
                "nx coupled support UV boundary lanes",
            )?;
        }
        lanes[blend_side].push(blend_parameters);
        lanes[support_side].push(support_parameters);
    }
    Ok(Some((lanes, lane_storage)))
}

fn complete_coupled_support_uv<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &mut CadIr,
    pending: &[PendingExt11SupportUv],
    coupled_support_budget: &SupportUvBudget<'_>,
    geometry_budget: &GeometryWorkBudget<'_>,
    failed_attempts: &mut BTreeMap<
        ProceduralCurveId,
        [Option<cadmpeg_ir::geometry::SupportPcurve>; 2],
    >,
    endpoint_witnesses: &mut EndpointWitnesses,
    endpoint_witness_storage: &mut ScopedReservation<'ctx>,
) -> Result<bool, cadmpeg_core::CodecError> {
    if geometry_budget.exhausted() {
        refuse_geometry_work(geometry_budget)?;
        return Ok(false);
    }
    let mut lane_geometry_exhausted = false;
    let mut replacements = Vec::new();
    let mut replacement_storage = ctx.reserve_scoped(0, "nx coupled support UV replacements")?;
    {
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
        let (owners, _owner_storage) = procedural_curve_owners(ctx, &ir.model.curves)?;
        let (positions, _position_storage) = procedural_curve_positions(ctx, ir)?;
        let mut blend_parameter_grids = BlendParameterGridCache::new(ctx)?;
        for (procedural_id, samples, fit_tolerance, serialized) in
            ctx.admit_iter(pending, "nx coupled support UV traversal")?
        {
            let points = &samples.points_charged(ctx)?;
            let parameters = &samples.parameters_charged(ctx)?;
            let Some(procedural) =
                model_index.procedural_curves(procedural_id.as_str(), geometry_budget.charges)?
            else {
                continue;
            };
            let Some(owner) = procedural_curve_owner(ctx, &owners, &procedural.id)? else {
                continue;
            };
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
            else {
                continue;
            };
            // A retry is skipped while both lanes still hold the state that
            // failed; the state is copied only when this attempt fails.
            if ctx
                .get_btree_map(
                    failed_attempts,
                    procedural_id,
                    "nx coupled support UV retry lookup",
                )?
                .is_some_and(|previous| {
                    previous
                        .iter()
                        .zip(context.sides())
                        .all(|(previous, side)| previous.as_ref() == side.pcurve.as_ref())
                })
            {
                continue;
            }
            let missing = [
                pcurve_requires_completion(
                    ctx,
                    context.sides()[0]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| &pcurve.geometry),
                )?,
                pcurve_requires_completion(
                    ctx,
                    context.sides()[1]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| &pcurve.geometry),
                )?,
            ];
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
                    .surfaces(surface.as_str(), geometry_budget.charges)?
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
                        .surfaces(surfaces[side].as_str(), geometry_budget.charges)?
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
                    && !missing[1 - side]
                    && model_index
                        .surfaces(surfaces[side].as_str(), geometry_budget.charges)?
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
            let sample_work = points.len().checked_mul(missing_lanes).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "nx coupled support UV samples",
                    u64::MAX,
                    cadmpeg_core::decode::u64_from_index(points.len()),
                )
            })?;
            if !coupled_support_budget.charge_by(work_units(sample_work)) {
                if let Some(limit) = ctx.resource_refusal() {
                    return Err(limit.into());
                }
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
                        model_index.surfaces(surfaces[side].as_str(), geometry_budget.charges)?
                    {
                        *seed = serialized_support_uv_seed_for_side(
                            ctx,
                            &surface.geometry,
                            serialized,
                            side,
                        )?;
                    }
                }
            }
            let parent_geometry_budget = geometry_budget;
            let lane_geometry_budget =
                parent_geometry_budget.child_slice(support_uv_lane_geometry_work_limit(
                    ctx,
                    points.len(),
                    parent_geometry_budget.remaining(),
                )?);
            let geometry_budget = &lane_geometry_budget;
            let mut lanes = complete_blend_boundary_support_uv_with_index_and_budget(
                &model_index,
                surfaces,
                points,
                *fit_tolerance,
                seeds,
                geometry_budget,
                &mut blend_parameter_grids,
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
            let Some((lanes, _lane_storage)) = lanes else {
                let mut lane_state = [None, None];
                for (state, side) in lane_state.iter_mut().zip(context.sides()) {
                    *state = side
                        .pcurve
                        .as_ref()
                        .map(|pcurve| {
                            Ok::<_, cadmpeg_core::CodecError>(
                                cadmpeg_ir::geometry::SupportPcurve::new(
                                    pcurve.geometry.try_clone_for_decode(
                                        ctx,
                                        "nx coupled support UV retry pcurve",
                                    )?,
                                    pcurve.parameter_range,
                                ),
                            )
                        })
                        .transpose()?;
                }
                ctx.insert_btree_map(
                    failed_attempts,
                    procedural_id
                        .try_clone_for_decode(ctx, "nx coupled support UV retry identity")?,
                    lane_state,
                    "nx coupled support UV failed retries",
                )?;
                let parent_exhausted = parent_geometry_budget
                    .consume_child(&lane_geometry_budget)
                    .is_err();
                let child_exhausted = lane_geometry_budget.exhausted();
                refuse_geometry_work(&lane_geometry_budget)?;
                refuse_geometry_work(parent_geometry_budget)?;
                lane_geometry_exhausted |= child_exhausted || parent_exhausted;
                continue;
            };
            for side in 0..2 {
                if missing[side] {
                    let endpoint_values = if let Some(surface) =
                        model_index.surfaces(surfaces[side].as_str(), geometry_budget.charges)?
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
                    let pcurve =
                        linear_pcurve_geometry(ctx, parameters, &lanes[side], geometry_budget)?;
                    if let Some([Some(first), Some(last)]) = endpoint_values {
                        push_endpoint_witness(
                            ctx,
                            endpoint_witness_storage,
                            endpoint_witnesses,
                            owner,
                            surfaces[side],
                            &pcurve,
                            parameter_range,
                            [first, last],
                            EndpointWitnessOperations {
                                lookup_key: "nx coupled support UV witness lookup key",
                                curve: "nx coupled support UV witness owner",
                                support: "nx coupled support UV witness surface",
                                pcurve: "nx coupled support UV witness pcurve",
                                index: "nx coupled support UV witness index",
                                witnesses: "nx coupled support UV endpoint witnesses",
                            },
                        )?;
                    }
                    if let Some(position) =
                        procedural_curve_position(ctx, &positions, procedural_id)?
                    {
                        ctx.reserve_scoped_vec(
                            &mut replacement_storage,
                            &mut replacements,
                            1,
                            "nx coupled support UV replacements",
                        )?;
                        replacements.push((position, side, pcurve));
                    }
                }
            }
            let parent_exhausted = parent_geometry_budget
                .consume_child(&lane_geometry_budget)
                .is_err();
            let child_exhausted = lane_geometry_budget.exhausted();
            refuse_geometry_work(&lane_geometry_budget)?;
            refuse_geometry_work(parent_geometry_budget)?;
            lane_geometry_exhausted |= child_exhausted || parent_exhausted;
        }
    }
    for (position, side, pcurve) in
        ctx.admit_iter(replacements, "nx coupled support UV replacement traversal")?
    {
        let Some(context) = ir
            .model
            .procedural_curves
            .get_mut(position)
            .and_then(|procedural| procedural.intersection_context_mut())
        else {
            continue;
        };
        if pcurve_requires_completion(
            ctx,
            context.sides()[side]
                .pcurve
                .as_ref()
                .map(|pcurve| &pcurve.geometry),
        )? {
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
    let geometry_budget =
        GeometryWorkBudget::from_context(ctx, cadmpeg_core::decode::u64_from_index(geometry_work));
    let mut failed_attempts = BTreeMap::new();
    let mut endpoint_witness_storage =
        ctx.reserve_scoped(0, "nx coupled support UV endpoint witnesses")?;
    let mut endpoint_witnesses = EndpointWitnesses::new();

    complete_coupled_support_uv(
        ctx,
        ir,
        pending,
        &coupled_support_budget,
        &geometry_budget,
        &mut failed_attempts,
        &mut endpoint_witnesses,
        &mut endpoint_witness_storage,
    )
}

pub(super) fn complete_parameterization_equivalent_support_uv(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut replacement_storage = ctx.reserve_scoped(0, "nx equivalent support UV replacements")?;
    let replacements = {
        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
        let mut replacements = Vec::new();
        for (procedural_index, procedural) in ctx
            .admit_iter(
                &ir.model.procedural_curves,
                "nx equivalent support UV traversal",
            )?
            .enumerate()
        {
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
            else {
                continue;
            };
            let missing = [
                pcurve_requires_completion(
                    ctx,
                    context.sides()[0]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| &pcurve.geometry),
                )?,
                pcurve_requires_completion(
                    ctx,
                    context.sides()[1]
                        .pcurve
                        .as_ref()
                        .map(|pcurve| &pcurve.geometry),
                )?,
            ];
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
                ctx,
            )? {
                ctx.reserve_scoped_vec(
                    &mut replacement_storage,
                    &mut replacements,
                    1,
                    "nx equivalent support UV replacements",
                )?;
                replacements.push((procedural_index, target, source));
            }
        }
        replacements
    };
    for &(procedural_index, side, source) in ctx.admit_iter(
        &replacements,
        "nx equivalent support UV replacement traversal",
    )? {
        let Some(context) = ir.model.procedural_curves[procedural_index].intersection_context_mut()
        else {
            continue;
        };
        if pcurve_requires_completion(
            ctx,
            context.sides()[side]
                .pcurve
                .as_ref()
                .map(|pcurve| &pcurve.geometry),
        )? {
            context.try_copy_pcurve_for_decode(ctx, source, side)?;
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
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, cadmpeg_ir::index::StandardIndex);
    parameterization_equivalent_surfaces_with_index(
        &index,
        first,
        second,
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap()
}

pub(super) fn parameterization_equivalent_surfaces_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    first: &SurfaceId,
    second: &SurfaceId,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Step<'a>, cadmpeg_core::decode::ResourceLimit> {
        if ctx.equal_bytes_limit(
            first.as_str().as_bytes(),
            second.as_str().as_bytes(),
            "NX equivalent surface identity comparison",
        )? {
            return Ok(Step::Equal);
        }
        let (Some(first_geometry), Some(second_geometry)) = (
            index
                .surfaces(first.as_str(), ctx)?
                .map(|surface| &surface.geometry),
            index
                .surfaces(second.as_str(), ctx)?
                .map(|surface| &surface.geometry),
        ) else {
            return Ok(Step::Different);
        };
        if first_geometry == second_geometry {
            return Ok(Step::Equal);
        }
        let (
            Some(ProceduralSurfaceDefinition::Offset(first_payload)),
            Some(ProceduralSurfaceDefinition::Offset(second_payload)),
        ) = (
            index
                .procedural_surface_for_surface(first.as_str(), ctx)?
                .map(cadmpeg_ir::geometry::ProceduralSurface::definition),
            index
                .procedural_surface_for_surface(second.as_str(), ctx)?
                .map(cadmpeg_ir::geometry::ProceduralSurface::definition),
        )
        else {
            return Ok(Step::Different);
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
            Ok(Step::Follow(first_support, second_support))
        } else {
            Ok(Step::Different)
        }
    }

    let mut slow = (first, second);
    let mut fast = (first, second);
    loop {
        ctx.charge_work_limit(1, "NX equivalent surface carrier scan")?;
        slow = match step(index, slow.0, slow.1, ctx)? {
            Step::Equal => return Ok(true),
            Step::Different => return Ok(false),
            Step::Follow(first, second) => (first, second),
        };
        for _ in 0..2 {
            fast = match step(index, fast.0, fast.1, ctx)? {
                Step::Equal => return Ok(true),
                Step::Different => return Ok(false),
                Step::Follow(first, second) => (first, second),
            };
        }
        if ctx.equal_bytes_limit(
            slow.0.as_str().as_bytes(),
            fast.0.as_str().as_bytes(),
            "NX equivalent surface cycle comparison",
        )? && ctx.equal_bytes_limit(
            slow.1.as_str().as_bytes(),
            fast.1.as_str().as_bytes(),
            "NX equivalent surface cycle comparison",
        )? {
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

/// Whether `id` lies in the stream namespace `prefix`: the prefix followed by
/// the namespace separator.
fn stream_owns_id(
    ctx: &DecodeContext<'_>,
    id: &str,
    prefix: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(ctx
        .strip_prefix(id, prefix, "nx completion stream ownership")?
        .is_some_and(|suffix| suffix.starts_with(':')))
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
    ctx: &DecodeContext<'_>,
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
        scope: scope.try_clone_for_decode(ctx)?,
        graph,
        source_stream,
        coedge_start,
        procedural_start,
    };
    attach_completed_intersection_pcurves_for_sources_with_budget(
        ctx,
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
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &[IntersectionCompletionSource<'_>],
    annotations: &mut AnnotationBuilder,
    validated_endpoint_witnesses: &EndpointWitnesses,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    attach_completed_intersection_pcurves_for_sources_with_budget(
        ctx,
        ir,
        sources,
        annotations,
        validated_endpoint_witnesses,
        geometry_budget,
    )?;
    Ok(())
}

/// The first source, in order, that owns a record of `id` at `position`.
/// Each visited source is charged before its ownership test.
fn owning_source(
    ctx: &DecodeContext<'_>,
    sources: &[IntersectionCompletionSource<'_>],
    prefixes: &[String],
    id: &str,
    position: usize,
    start: impl Fn(&IntersectionCompletionSource<'_>) -> usize,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    for (index, (source, prefix)) in sources.iter().zip(prefixes).enumerate() {
        ctx.charge_work(1, "nx completion source search")?;
        if position >= start(source) && stream_owns_id(ctx, id, prefix)? {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn attach_completed_intersection_pcurves_for_sources_with_budget(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &[IntersectionCompletionSource<'_>],
    annotations: &mut AnnotationBuilder,
    validated_endpoint_witnesses: &EndpointWitnesses,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    // Every table and list below is dropped before return.
    let mut storage = ctx.reserve_scoped(0, "nx completion tables")?;
    let mut source_prefixes = Vec::new();
    ctx.reserve_scoped_vec(
        &mut storage,
        &mut source_prefixes,
        sources.len(),
        "nx completion source prefixes",
    )?;
    for source in ctx.admit_iter(sources, "nx completion source prefixes")? {
        let prefix = storage.with_storage(|| source.scope.prefix_charged(ctx))?;
        source_prefixes.push(prefix);
    }
    let replacements = {
        let (loop_faces, _loop_storage) = ctx.unique_index(
            ir.model
                .loops
                .iter()
                .map(|loop_| (loop_.id.as_str(), &loop_.face)),
            "nx completion loop-face index",
        )?;
        let (face_surfaces, _face_storage) = ctx.unique_index(
            ir.model
                .faces
                .iter()
                .map(|face| (face.id.as_str(), &face.surface)),
            "nx completion face-surface index",
        )?;
        let (edges_by_id, _edge_storage) = ctx.unique_index(
            ir.model.edges.iter().map(|edge| (edge.id.as_str(), edge)),
            "nx completion edge index",
        )?;
        let mut coedge_candidates = Vec::new();
        for (index, coedge) in ctx
            .admit_iter(&ir.model.coedges, "nx completion coedge traversal")?
            .enumerate()
        {
            if !coedge.pcurves.is_empty() {
                continue;
            }
            let Some(source_index) = owning_source(
                ctx,
                sources,
                &source_prefixes,
                coedge.id.as_str(),
                index,
                |source| source.coedge_start,
            )?
            else {
                continue;
            };
            let Some(face) = ctx
                .get_hash_map(
                    &loop_faces,
                    coedge.owner_loop.as_str(),
                    "nx completion loop-face lookup",
                )?
                .copied()
                .flatten()
            else {
                continue;
            };
            let Some(surface) = ctx
                .get_hash_map(
                    &face_surfaces,
                    face.as_str(),
                    "nx completion face-surface lookup",
                )?
                .copied()
                .flatten()
            else {
                continue;
            };
            let Some(edge) = ctx
                .get_hash_map(
                    &edges_by_id,
                    coedge.edge.as_str(),
                    "nx completion edge lookup",
                )?
                .copied()
                .flatten()
            else {
                continue;
            };
            let Some(curve) = edge.curve() else {
                continue;
            };
            ctx.reserve_scoped_vec(
                &mut storage,
                &mut coedge_candidates,
                1,
                "nx completion coedge candidates",
            )?;
            coedge_candidates.push((
                index,
                &coedge.edge,
                curve,
                surface,
                edge.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
                source_index,
            ));
        }
        if coedge_candidates.is_empty() {
            return Ok(());
        }
        let mut required_keys = BTreeSet::new();
        for &(_, _, curve, surface, _, _) in
            ctx.admit_iter(&coedge_candidates, "nx completion required chart keys")?
        {
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut required_keys,
                    (curve, surface),
                    "nx completion required chart keys",
                )
            })?;
        }
        let (owners, _owner_storage) = procedural_curve_owners(ctx, &ir.model.curves)?;
        let mut candidates =
            BTreeMap::<(&CurveId, &SurfaceId), Vec<(&PcurveGeometry, [f64; 2], Option<f64>)>>::new(
            );
        let procedural_start = ctx
            .min_by_key(
                sources,
                |source| Ok(source.procedural_start),
                |first, second| Ok(first.cmp(second)),
                "nx completion procedural start",
            )?
            .map_or(0, |source| source.procedural_start);
        let multiple_sources = sources.len() > 1;
        let procedurals = ir
            .model
            .procedural_curves
            .get(procedural_start..)
            .unwrap_or_default();
        for (offset, procedural) in ctx
            .admit_iter(procedurals, "nx completion procedural traversal")?
            .enumerate()
        {
            let index = procedural_start + offset;
            if multiple_sources
                && owning_source(
                    ctx,
                    sources,
                    &source_prefixes,
                    procedural.id.as_str(),
                    index,
                    |source| source.procedural_start,
                )?
                .is_none()
            {
                continue;
            }
            let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
            else {
                continue;
            };
            let Some(owner) = procedural_curve_owner(ctx, &owners, &procedural.id)? else {
                continue;
            };
            for side in context.sides() {
                let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                    continue;
                };
                let key = (owner, surface);
                if !ctx.contains_btree_set(&required_keys, &key, "nx completion chart key test")? {
                    continue;
                }
                let candidate = (
                    &pcurve.geometry,
                    context.parameter_range().endpoints(),
                    procedural
                        .cache_fit_tolerance()
                        .map(cadmpeg_ir::geometry::FitTolerance::get),
                );
                match ctx.get_mut_btree_map(
                    &mut candidates,
                    &key,
                    "nx completion candidate keys",
                )? {
                    Some(values) => {
                        // Equal candidates are recorded once; the search stops
                        // at the first equal value.
                        let mut recorded = false;
                        for value in values.iter() {
                            ctx.charge_work(1, "nx completion candidate search")?;
                            if *value == candidate {
                                recorded = true;
                                break;
                            }
                        }
                        if !recorded {
                            storage.with_storage(|| {
                                ctx.push_vec(values, candidate, "nx completion candidate values")
                            })?;
                        }
                    }
                    None => storage.with_storage(|| {
                        ctx.push_btree_group(
                            &mut candidates,
                            key,
                            candidate,
                            "nx completion candidate keys",
                            "nx completion candidate values",
                        )
                    })?,
                }
            }
        }

        let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx)?;
        let mut edge_endpoint_contracts = BTreeMap::new();
        for &(_, edge_id, ..) in
            ctx.admit_iter(&coedge_candidates, "nx completion edge endpoint contracts")?
        {
            if ctx.contains_key_btree_map(
                &edge_endpoint_contracts,
                edge_id.as_str(),
                "nx completion edge endpoint contract lookup",
            )? {
                continue;
            }
            if let Some(contract) = pcurve_edge_endpoint_contract_with_index(
                &model_index,
                edge_id,
                geometry_budget.charges,
            )? {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut edge_endpoint_contracts,
                        edge_id.as_str(),
                        contract,
                        "nx completion edge endpoint contracts",
                    )
                })?;
            }
        }
        // A chart carrier's serialized endpoint witnesses are a necessary
        // edge-incidence condition. Reuse a prior sample-wise proof; evaluate
        // the face surface only for keys without that proof.
        let mut endpoint_admissible_keys = BTreeSet::new();
        for &(_, edge_id, curve, surface, edge_tolerance, _) in
            ctx.admit_iter(&coedge_candidates, "nx completion admissible chart keys")?
        {
            let key = (curve, surface);
            let Some(values) =
                ctx.get_btree_map(&candidates, &key, "nx completion candidate lookup")?
            else {
                continue;
            };
            let [candidate] = values.as_slice() else {
                continue;
            };
            let admissible = if let Some(witness) = linear_nurbs_curve_endpoint_witness_with_index(
                &model_index,
                curve,
                geometry_budget.charges,
            )? {
                let Some(&(edge_endpoints, edge_allowance)) = ctx.get_btree_map(
                    &edge_endpoint_contracts,
                    edge_id.as_str(),
                    "nx completion edge endpoint contract lookup",
                )?
                else {
                    continue;
                };
                let fit_tolerance = candidate.2.or(edge_tolerance);
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
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut endpoint_admissible_keys,
                        key,
                        "nx completion admissible chart keys",
                    )
                })?;
            }
        }
        let mut witnessed_keys = BTreeSet::new();
        let mut candidate_endpoints = BTreeMap::new();
        for (key, values) in ctx.admit_iter(&candidates, "nx completion candidate endpoints")? {
            if !ctx.contains_btree_set(
                &endpoint_admissible_keys,
                key,
                "nx completion admissible chart key test",
            )? {
                continue;
            }
            let [candidate] = values.as_slice() else {
                continue;
            };
            let witness = endpoint_witness_for_candidate(
                ctx,
                validated_endpoint_witnesses,
                *key,
                candidate.0,
                candidate.1,
            )?;
            if witness.is_some() {
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut witnessed_keys,
                        *key,
                        "nx completion witnessed chart keys",
                    )
                })?;
            }
            let endpoints = if witness.is_some() {
                witness
            } else {
                pcurve_surface_endpoints_with_index_and_budget(
                    &model_index,
                    key.1,
                    candidate.0,
                    None,
                    geometry_budget,
                )?
            };
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut candidate_endpoints,
                    *key,
                    endpoints,
                    "nx completion candidate endpoints",
                )
            })?;
        }
        let mut replacements = Vec::new();
        for &(coedge_index, edge_id, curve, surface, edge_tolerance, source_index) in
            ctx.admit_iter(&coedge_candidates, "nx completion replacements")?
        {
            let key = (curve, surface);
            let Some(candidate_values) =
                ctx.get_btree_map(&candidates, &key, "nx completion candidate lookup")?
            else {
                continue;
            };
            let [candidate] = candidate_values.as_slice() else {
                continue;
            };
            let Some(&(edge_endpoints, edge_allowance)) = ctx.get_btree_map(
                &edge_endpoint_contracts,
                edge_id.as_str(),
                "nx completion edge endpoint contract lookup",
            )?
            else {
                continue;
            };
            let fit_tolerance = candidate.2.or(edge_tolerance);
            let matches = {
                let Some(coincident_surface) = ctx
                    .get_btree_map(
                        &candidate_endpoints,
                        &key,
                        "nx completion candidate endpoint lookup",
                    )?
                    .and_then(Option::as_ref)
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
                if !ctx.remove_btree_set(
                    &mut witnessed_keys,
                    &key,
                    "nx completion witnessed chart key removal",
                )? {
                    continue;
                }
                // A witness from another geometry phase is a shortcut. The
                // endpoint evaluator resolves a disagreement with that proof.
                let fallback = pcurve_surface_endpoints_with_index_and_budget(
                    &model_index,
                    key.1,
                    candidate.0,
                    None,
                    geometry_budget,
                )?;
                let Some(slot) = ctx.get_mut_btree_map(
                    &mut candidate_endpoints,
                    &key,
                    "nx completion candidate endpoint lookup",
                )?
                else {
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
            ctx.reserve_scoped_vec(
                &mut storage,
                &mut replacements,
                1,
                "nx completion pcurve replacements",
            )?;
            replacements.push((
                coedge_index,
                source_index,
                (
                    candidate
                        .0
                        .try_clone_for_decode(ctx, "nx completion replacement pcurve")?,
                    metadata,
                ),
            ));
        }
        replacements
    };

    // Each replacement names its completed pcurve from its coedge's fin. A
    // name already present, before or earlier in this batch, adds nothing.
    let mut prepared = Vec::new();
    {
        let (existing_pcurves, _existing_storage) = ctx.collect_scoped_string_set(
            0,
            ir.model.pcurves.iter().map(|pcurve| pcurve.id.as_str()),
            "nx completion existing pcurve identities",
        )?;
        for (coedge_index, source_index, completed) in
            ctx.admit_iter(replacements, "nx completion replacement naming")?
        {
            let Some(coedge) = ir.model.coedges.get(coedge_index) else {
                continue;
            };
            let source = &sources[source_index];
            let Some((_, value)) =
                ctx.rsplit_once(coedge.id.as_str(), "#", "nx completion coedge XMT")?
            else {
                continue;
            };
            let Ok(fin_xmt) = ctx.parse_text::<u32>(value, "nx completion coedge XMT")? else {
                continue;
            };
            let pcurve_id: PcurveId = source.scope.id_charged(
                ctx,
                &cadmpeg_ir::identity_component!("intersection-pcurve-completed"),
                fin_xmt,
            )?;
            if ctx.contains_hash_set(
                &existing_pcurves,
                pcurve_id.as_str(),
                "nx completion existing pcurve lookup",
            )? {
                continue;
            }
            ctx.reserve_scoped_vec(
                &mut storage,
                &mut prepared,
                1,
                "nx completion named pcurves",
            )?;
            prepared.push((coedge_index, source_index, fin_xmt, pcurve_id, completed));
        }
    }
    let mut named = BTreeSet::new();
    for (coedge_index, source_index, fin_xmt, pcurve_id, (geometry, metadata)) in
        ctx.admit_iter(prepared, "nx completed pcurve attachment")?
    {
        // Two coedges of one fin name the same pcurve; the first one attaches it.
        if !storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut named,
                pcurve_id.try_clone_for_decode(ctx, "nx completion batch pcurve identity")?,
                "nx completion batch pcurve identities",
            )
        })? {
            continue;
        }
        let source = &sources[source_index];
        let source_offset = source
            .graph
            .get(ctx, NodeKind::Fin, fin_xmt)?
            .map_or(0, |node| cadmpeg_core::decode::u64_from_index(node.pos()));
        annotations.note(
            ctx,
            &pcurve_id,
            &source.source_stream,
            source_offset,
            Some("INTERSECTION_PCURVE"),
        )?;
        annotations
            .derived(ctx, &pcurve_id, "geometry")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, &pcurve_id, "parameter_range")
            .map_err(cadmpeg_core::CodecError::from)?;
        if metadata.fit_tolerance().is_some() {
            annotations
                .derived(ctx, &pcurve_id, "fit_tolerance")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        ctx.reserve_vec(&mut ir.model.pcurves, 1, "nx completed pcurve records")?;
        ir.model.pcurves.push(Pcurve {
            id: pcurve_id.try_clone_for_decode(ctx, "nx completed pcurve record identity")?,
            geometry,
            metadata,
        });
        if let Some(coedge) = ir
            .model
            .coedges
            .get_mut(coedge_index)
            .filter(|coedge| coedge.pcurves.is_empty())
        {
            ctx.reserve_vec(&mut coedge.pcurves, 1, "nx completed coedge pcurve uses")?;
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
        attach_completed_intersection_pcurves_for_stream_with_budget,
        complete_support_uv_with_budget_and_endpoint_witnesses, ordered_support_uv_seed_candidates,
        support_uv_lane_geometry_work_limit, support_uv_lane_matches_surface_with_budget,
        unseeded_nurbs_surface_parameters_with_index_and_budget, MAX_SUPPORT_UV_LANE_GEOMETRY_WORK,
        MAX_SUPPORT_UV_SAMPLES,
    };
    use crate::intersection::SupportUvLane;
    use cadmpeg_core::decode::{ResourceDimension, WorkBudget};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::math::Point3;

    fn ext11_assignment(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        duplicate: bool,
    ) -> Result<Option<crate::intersection::SupportUv>, cadmpeg_core::CodecError> {
        let plane = SurfaceId::mint("nx:test:surface#plane").unwrap();
        let unknown = SurfaceId::mint("nx:test:surface#unknown").unwrap();
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: plane.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                ).unwrap(),
            )),
            source_object: None,
        });
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: unknown.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        let lane = || SupportUvLane::new(vec![[0.0, 0.0], [0.125, 0.0]], 2).unwrap();
        let lanes = [Some(lane()), duplicate.then(lane)];
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(
            &ir, cadmpeg_ir::index::StandardIndex,
        );
        let budget = GeometryWorkBudget::from_context(
            ctx, cadmpeg_core::decode::u64_from_index(super::super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK),
        );
        super::assign_ext11_support_uv_to_surfaces_with_index(
            ctx, &index, [&plane, &unknown],
            &[Point3::new(0.0, 0.0, 0.0), Point3::new(125.0, 0.0, 0.0)],
            0.0, &lanes, &budget,
        )
    }

    #[test]
    fn ext11_duplicate_support_assignment_rejects_without_an_owning_copy() {
        crate::test_support::with_decode_context_over(&[], |policy| {
            policy.limits.max_retained_bytes = 0;
        }, |ctx| {
            assert!(ext11_assignment(ctx, true).unwrap().is_none());
            assert!(ctx.resource_refusal().is_none());
        });
    }

    #[test]
    fn ext11_unique_support_assignment_copies_only_the_accepted_lane() {
        let bytes = cadmpeg_core::decode::u64_from_index(
            2 * std::mem::size_of::<cadmpeg_ir::units::FiniteVector<2>>(),
        );
        crate::test_support::with_decode_context_over(&[], |policy| {
            policy.limits.max_retained_bytes = bytes;
        }, |ctx| {
            let [Some(lane), None] = ext11_assignment(ctx, false).unwrap().unwrap() else {
                panic!("only the planar support has a matching lane");
            };
            assert_eq!(lane.as_slice(), &[
                cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]).unwrap(),
                cadmpeg_ir::units::FiniteVector::new([0.125, 0.0]).unwrap(),
            ]);
            assert!(ctx.resource_refusal().is_none());
        });
    }

    #[test]
    fn ext11_accepted_lane_copy_refuses_one_byte_below_its_actual_storage() {
        let bytes = cadmpeg_core::decode::u64_from_index(
            2 * std::mem::size_of::<cadmpeg_ir::units::FiniteVector<2>>(),
        );
        crate::test_support::with_decode_context_over(&[], |policy| {
            policy.limits.max_retained_bytes = bytes - 1;
        }, |ctx| {
            let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = ext11_assignment(ctx, false) else {
                panic!("the accepted lane needs its two finite-vector slots");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "NX solved support-UV lane copy");
            assert_eq!((limit.used, limit.additional), (0, bytes));
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }

    #[test]
    fn crossed_ext11_assignment_preserves_serialized_copy_order_and_values() {
        use cadmpeg_ir::geometry::analytic::PlaneSurface;
        use cadmpeg_ir::math::Vector3;
        let surfaces = ["nx:test:surface#x", "nx:test:surface#y"]
            .map(|id| SurfaceId::mint(id).unwrap());
        let mut ir = CadIr::empty();
        for (id, axis) in surfaces.iter().zip([Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)]) {
            ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), axis).unwrap(),
                )),
                source_object: None,
            });
        }
        let lanes = [
            SupportUvLane::new(vec![[0.0, 0.0], [0.0, -0.125], [0.0, -0.25]], 3),
            SupportUvLane::new(vec![[0.0, 0.0], [0.125, 0.0]], 2),
        ];
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
        let slot = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<cadmpeg_ir::units::FiniteVector<2>>());
        let first_bytes = 3 * slot;
        let total_bytes = first_bytes + 2 * slot;
        for cap in [first_bytes - 1, total_bytes] {
            crate::test_support::with_decode_context_over(&[], |policy| {
                policy.limits.max_retained_bytes = cap;
            }, |ctx| {
                let budget = GeometryWorkBudget::from_context(ctx,
                    cadmpeg_core::decode::u64_from_index(super::super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK));
                let result = super::assign_ext11_support_uv_to_surfaces_with_index(
                    ctx, &index, [&surfaces[0], &surfaces[1]],
                    &[Point3::new(0.0, 0.0, 0.0), Point3::new(125.0, 0.0, 0.0)],
                    0.0, &lanes, &budget,
                );
                if cap == total_bytes {
                    assert_eq!(result.unwrap().unwrap(), [lanes[1].clone(), lanes[0].clone()]);
                    assert!(ctx.resource_refusal().is_none());
                } else {
                    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
                        panic!("the first serialized lane needs its three slots");
                    };
                    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                    assert_eq!(limit.operation, "NX solved support-UV lane copy");
                    assert_eq!((limit.used, limit.additional), (0, first_bytes));
                    assert_eq!(ctx.resource_refusal(), Some(limit));
                }
            });
        }
    }

    #[test]
    fn serialized_support_seed_selection_preserves_surface_walk_refusals() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
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
        for cap in 0..2 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let limit = super::serialized_support_uv_seed_candidates(&ctx, &surface, &source, 0, 0)
                .expect_err("first and later candidate work refusals propagate");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert!(
                matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let limit = super::serialized_support_uv_seed_for_side(&ctx, &surface, &source, 0)
            .expect_err("seed absence does not replace resource refusal");
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit)
        );

        let arena = DecodeArena::new();
        policy.limits.max_work_units = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(
            super::serialized_support_uv_seed_candidates(&ctx, &surface, &source, 0, 0)
                .expect("two candidates"),
            [
                Some(Point2::new(1000., 2000.)),
                Some(Point2::new(3000., 4000.)),
                None,
                None
            ]
        );
        assert_eq!(
            super::serialized_support_uv_seed_for_side(&ctx, &surface, &source, 0)
                .expect("first seed"),
            Some(Point2::new(1000., 2000.))
        );
        ctx.finish_session().expect("exact candidate visits");
    }

    fn attach_empty_model_under_policy(
        adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> Result<(), cadmpeg_core::CodecError> {
        let graph = crate::test_support::with_decode_context(|ctx| {
            crate::topology::Graph::parse(ctx, &[])
        })?;

        crate::test_support::with_decode_context_over(&[], adjust, |ctx| {
            let geometry_budget = GeometryWorkBudget::from_context(ctx, 100);
            let mut ir = CadIr::empty();
            let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
            attach_completed_intersection_pcurves_for_stream_with_budget(
                ctx,
                &mut ir,
                super::IntersectionStream {
                    graph: &graph,
                    scope: &crate::decode::ids::IdScope::stream(0),
                    coedge_start: 0,
                    procedural_start: 0,
                    source_stream: cadmpeg_ir::annotations::StreamHandle::new(
                        &cadmpeg_test_support::service_decode_context(),
                        cadmpeg_ir::stream_name!("nx:test"),
                        "fixture stream handle",
                    )
                    .unwrap(),
                    validated_endpoint_witnesses: &BTreeMap::new(),
                },
                &mut annotations,
                &geometry_budget,
            )
        })
    }

    #[test]
    fn linear_knots_refuse_retained_storage_before_construction() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_retained_bytes = 0,
            |ctx| {
                let budget = GeometryWorkBudget::from_context(ctx, 100);
                assert!(
                    matches!(super::linear_knots(&[0.0, 1.0], &budget), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "nx linear knots" && limit.dimension == ResourceDimension::RetainedBytes)
                );
            },
        );
        crate::test_support::with_decode_context(|ctx| {
            let budget = GeometryWorkBudget::from_context(ctx, 100);
            assert_eq!(
                super::linear_knots(&[0.0, 1.0], &budget).unwrap(),
                [0.0, 0.0, 1.0, 1.0]
            );
        });
    }

    #[test]
    fn completion_attachment_refuses_scope_copy_at_retained_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_retained_bytes = 0;
        };
        assert!(matches!(
            attach_empty_model_under_policy(adjust_policy),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "nx completion scope copy"
        ));
    }

    #[test]
    fn completion_attachment_refuses_source_prefixes_at_collection_limit() {
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_collection_items = 0;
        };
        assert!(matches!(
            attach_empty_model_under_policy(adjust_policy),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx completion source prefixes"
        ));
    }

    #[test]
    fn support_uv_completion_refuses_model_index_at_caller_collection_limit() {
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: SurfaceId::mint("test:model:entity#synthetic:completion-limit-surface")
                .expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("plane frame"),
            )),
            source_object: None,
        });

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let support_budget = WorkBudget::new(1);
                let geometry_budget = GeometryWorkBudget::from_context(ctx, 100);
                let mut endpoint_witness_storage = ctx
                    .reserve_scoped(0, "nx support UV endpoint witnesses")
                    .unwrap();
                let mut endpoint_witnesses = BTreeMap::new();

                let error = complete_support_uv_with_budget_and_endpoint_witnesses(
                    ctx,
                    &mut ir,
                    &[],
                    (&support_budget, &geometry_budget),
                    (&support_budget, &geometry_budget),
                    &mut endpoint_witnesses,
                    &mut endpoint_witness_storage,
                )
                .expect_err("the model identity universe exceeds zero collection slots");
                let cadmpeg_core::CodecError::ResourceLimit(first) = error else {
                    panic!("model index refusal");
                };
                assert_eq!(first.dimension, ResourceDimension::CollectionItems);
                assert_eq!(first.operation, "model identity universe slots");
                assert_eq!(ctx.resource_refusal(), Some(first));
            },
        );
    }

    #[test]
    fn support_uv_completion_propagates_geometry_work_refusal() {
        crate::test_support::with_decode_context(|ctx| {
            let support_budget = ctx.work_budget(10);
            let geometry_budget = GeometryWorkBudget::from_context(ctx, 0);
            assert!(!geometry_budget.charge());
            let mut ir = CadIr::empty();
            let mut endpoint_witness_storage = ctx
                .reserve_scoped(0, "nx support UV endpoint witnesses")
                .unwrap();
            let mut endpoint_witnesses = BTreeMap::new();

            assert!(matches!(
                complete_support_uv_with_budget_and_endpoint_witnesses(
                    ctx,
                    &mut ir,
                    &[],
                    (&support_budget, &geometry_budget),
                    (&support_budget, &geometry_budget),
                    &mut endpoint_witnesses,
                    &mut endpoint_witness_storage,
                ),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::Codec("nx adaptive geometry work")
            ));
        });
    }

    #[test]
    fn support_uv_completion_refuses_first_wave_work_limit() {
        use cadmpeg_core::decode::ResourceDimension;

        let ir = CadIr::empty();
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "nx support UV completion wave pass",
            |ctx| {
                let support_budget = ctx.work_budget(10);
                let geometry_budget = GeometryWorkBudget::from_context(ctx, 100);
                let mut ir = ir.clone();
                let mut endpoint_witness_storage =
                    ctx.reserve_scoped(0, "nx support UV endpoint witnesses")?;
                let mut endpoint_witnesses = BTreeMap::new();

                complete_support_uv_with_budget_and_endpoint_witnesses(
                    ctx,
                    &mut ir,
                    &[],
                    (&support_budget, &geometry_budget),
                    (&support_budget, &geometry_budget),
                    &mut endpoint_witnesses,
                    &mut endpoint_witness_storage,
                )
                .map(|_| ())
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "nx support UV completion wave pass"
        ));
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
            let index = cadmpeg_ir::index::ModelIndex::new_model_only(
                &ir,
                cadmpeg_ir::index::StandardIndex,
            );
            let points = vec![Point3::new(0.0, 0.0, 0.0); MAX_SUPPORT_UV_SAMPLES + 1];
            let values =
                SupportUvLane::new(vec![[0.0, 0.0]; MAX_SUPPORT_UV_SAMPLES + 1], points.len())
                    .unwrap();
            let geometry_budget = GeometryWorkBudget::from_context(
                geometry_ctx,
                cadmpeg_core::decode::u64_from_index(1),
            );

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
        crate::test_support::with_decode_context(|ctx| {
            let parent = WorkBudget::new(MAX_SUPPORT_UV_LANE_GEOMETRY_WORK * 2);
            let lane_limit = support_uv_lane_geometry_work_limit(
                ctx,
                MAX_SUPPORT_UV_SAMPLES,
                parent.remaining(),
            )
            .unwrap();
            let lane = parent.child_slice(lane_limit);

            assert_eq!(lane_limit, MAX_SUPPORT_UV_LANE_GEOMETRY_WORK);
            assert!(lane.charge_by(lane_limit));
            assert!(!lane.charge());
            assert_eq!(parent.consumed(), 0);
            assert!(!parent.exhausted());

            assert!(matches!(parent.consume_child(&lane), Ok(())));
            assert_eq!(parent.consumed(), MAX_SUPPORT_UV_LANE_GEOMETRY_WORK);
            assert!(!parent.exhausted());

            let later_lane = parent.child_slice(
                support_uv_lane_geometry_work_limit(
                    ctx,
                    MAX_SUPPORT_UV_SAMPLES,
                    parent.remaining(),
                )
                .unwrap(),
            );
            assert!(later_lane.charge());
        });
    }

    #[test]
    fn support_uv_completion_count_overflow_refuses_work() {
        crate::test_support::with_decode_context(|ctx| {
            assert!(matches!(
                super::support_uv_completion_budget_limit(ctx, usize::MAX),
                Err(cadmpeg_core::CodecError::ResourceLimit(_))
            ));
        });
    }

    #[test]
    fn support_uv_lane_count_overflow_refuses_work() {
        crate::test_support::with_decode_context(|ctx| {
            assert!(matches!(
                support_uv_lane_geometry_work_limit(ctx, usize::MAX, usize::MAX),
                Err(cadmpeg_core::CodecError::ResourceLimit(_))
            ));
        });
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
                    &cadmpeg_test_support::service_decode_context(),
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
                .expect("fixture constructor admission")
                .expect("valid test surface");
                let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs.clone()));
                let mut ir = CadIr::empty();
                ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
                    id: surface_id.clone(),
                    geometry: geometry.clone(),
                    source_object: None,
                });
                let index = cadmpeg_ir::index::ModelIndex::new_model_only(
                    &ir,
                    cadmpeg_ir::index::StandardIndex,
                );

                let fit_budget = GeometryWorkBudget::from_context(
                    geometry_ctx,
                    cadmpeg_core::decode::u64_from_index(work_limit),
                );
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
                    let first =
                        parameters.expect_err("the original slice cannot admit the complete grid");
                    assert_eq!(geometry_ctx.resource_refusal(), Some(first));
                    crate::test_support::with_decode_context(|miss_ctx| {
                        let miss_budget = GeometryWorkBudget::from_context(
                            miss_ctx,
                            cadmpeg_core::decode::u64_from_index(work_limit),
                        );
                        let refusal = unseeded_nurbs_surface_parameters_with_index_and_budget(
                            &index,
                            &surface_id,
                            &geometry,
                            &nurbs,
                            Point3::new(0.5, 0.5, 1.0),
                            FIT_TOLERANCE,
                            &miss_budget,
                        )
                        .expect_err("the original slice cannot admit the complete miss grid");
                        assert_eq!(miss_ctx.resource_refusal(), Some(refusal));
                    });
                    return;
                }
                let parameters = parameters
                    .expect("evaluator allocation succeeds")
                    .expect("coarse grid contains the exact chart point");
                assert_eq!(parameters, Point2::new(0.5, 0.5));

                let miss_budget = GeometryWorkBudget::from_context(
                    geometry_ctx,
                    cadmpeg_core::decode::u64_from_index(work_limit),
                );
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
}
