// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use super::{find_record_value, named_parameter, step_instance_id, RecordExt, ValueExt};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{nurbs_curve_parameter_domain, nurbs_curve_parameter_near_point};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{KnotVector, NurbsCurve, NurbsSurface, NurbsSurfaceAxis},
    pcurve::{Pcurve, PcurveGeometry, PcurveNurbs},
    sampled::{PolylineCurve, PolylineSamples},
    CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, DirectedParameterRange,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralGeometryError, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CurveId, IdentityKey, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{
    Angle, FiniteReal, NonNegativeLength, NonZeroLength, PositiveLength, PositiveReal,
};
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::transform::{Transform, Transform2};
use cadmpeg_ir::units::{HypotDirection2, OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::SourceObjectAssociation;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::index::{CarrierIndex, CurveIndex, PointIndex, SurfaceIndex};
use super::{opaque_record_id, StageOutcome};

const EPS_GEOMETRY_READ_COARSE_GEOMETRY: f64 = 1.0e-6;
const EPS_GEOMETRY_READ_GEOMETRY: f64 = 1.0e-9;
const EPS_GEOMETRY_READ_EXACT_GEOMETRY: f64 = 1.0e-12;

const RANGE_INFERENCE_WORK_UNITS: u64 = 4_096;

macro_rules! geometry_or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

fn attach_geometry_source(
    source: &mut Option<SourceObjectAssociation>,
    id: u64,
    name: Option<&str>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if source.is_none() {
        let name = name
            .map(|name| ctx.format_retained(format_args!("{name}"), operation))
            .transpose()?;
        *source = Some(super::step_source_association(ctx, id, name)?);
    }
    Ok(())
}

pub(super) struct GeometryData {
    pub(super) placements: BTreeMap<u64, (FinitePoint3, UnitVector3, UnitVector3)>,
    pub(super) transformation_operators: BTreeMap<u64, Transform>,
    pub(super) units: UnitScales,
}

pub(super) fn placement_transform(
    (origin, z_axis, x_axis): (FinitePoint3, UnitVector3, UnitVector3),
) -> Option<Transform> {
    let origin = origin.get();
    let z_axis = z_axis.as_raw();
    let x_axis = x_axis.as_raw();
    let y_axis = Vector3::new(
        z_axis.y * x_axis.z - z_axis.z * x_axis.y,
        z_axis.z * x_axis.x - z_axis.x * x_axis.z,
        z_axis.x * x_axis.y - z_axis.y * x_axis.x,
    );
    let placement_basis = [
        [x_axis.x, y_axis.x, z_axis.x],
        [x_axis.y, y_axis.y, z_axis.y],
        [x_axis.z, y_axis.z, z_axis.z],
    ];
    let mut rows = Transform::identity().affine_rows();
    for row in 0..3 {
        for column in 0..3 {
            rows[row][column] = placement_basis[row][column];
        }
    }
    rows[0][3] = origin.x;
    rows[1][3] = origin.y;
    rows[2][3] = origin.z;
    Transform::affine(rows)
}

/// Infer the carrier interval trimmed by each edge's endpoint vertices.
pub(super) fn infer_edge_parameter_ranges(
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut points = HashMap::new();
    for point in ctx.admit_iter(
        &(ir.model.points)[..],
        "STEP infer edge parameter ranges traversal",
    )? {
        ctx.reserve_map(&mut points, 1, "step_parameter_inference_points")?;
        points.insert(point.id.as_str(), point.position().get());
    }
    let mut vertices = HashMap::new();
    for vertex in ctx.admit_iter(
        &(ir.model.vertices)[..],
        "STEP infer edge parameter ranges traversal",
    )? {
        if let Some(point) = points.get(vertex.point.as_str()).copied() {
            ctx.reserve_map(&mut vertices, 1, "step_parameter_inference_vertices")?;
            vertices.insert(vertex.id.as_str(), point);
        }
    }
    let mut candidates = Vec::new();
    for (index, edge) in ctx
        .admit_iter(
            &(ir.model.edges)[..],
            "STEP infer edge parameter ranges traversal",
        )?
        .enumerate()
    {
        if edge.param_range().is_some() {
            continue;
        }
        let Some((curve, start, end)) = edge.curve().and_then(|curve| {
            Some((
                curve,
                vertices.get(edge.start.as_str()).copied()?,
                vertices.get(edge.end.as_str()).copied()?,
            ))
        }) else {
            continue;
        };
        ctx.reserve_vec(&mut candidates, 1, "step_parameter_inference_candidates")?;
        candidates.push((index, curve, start, end));
    }
    let work = u64_from_index(candidates.len())
        .checked_mul(RANGE_INFERENCE_WORK_UNITS)
        .ok_or_else(|| ctx.refuse_codec_limit("step_edge_parameter_inference", 0, 1))?;
    ctx.charge_work(work, "step_edge_parameter_inference")?;

    let model_index = cadmpeg_ir::index::ModelIndex::build(ir, ctx)?;
    let inferred = candidates.into_iter().try_fold(
        Vec::new(),
        |mut inferred, (edge_index, curve, start, end)| {
            let Some(geometry) = model_index
                .curves(curve.as_str(), ctx)?
                .map(|curve| &curve.geometry)
            else {
                return Ok(inferred);
            };
            let Some(solved) = geometry.solved() else {
                return Ok(inferred);
            };
            let start_seed = curve_endpoint_seed(solved, false, 0.0);
            let Some(start_parameter) =
                cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
                    ctx,
                    &model_index,
                    curve,
                    start,
                    start_seed,
                )?
            else {
                return Ok(inferred);
            };
            let end_seed = curve_endpoint_seed(solved, true, start_parameter.get());
            let Some(end_parameter) = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
                ctx,
                &model_index,
                curve,
                end,
                end_seed,
            )?
            else {
                return Ok(inferred);
            };
            if let Some(range) = edge_parameter_range(solved, start_parameter, end_parameter) {
                ctx.reserve_vec(&mut inferred, 1, "step_parameter_inference_ranges")?;
                inferred.push((edge_index, range));
            }
            Ok::<_, CodecError>(inferred)
        },
    )?;
    drop(model_index);

    for (index, range) in inferred {
        if let Some(edge) = ir.model.edges.get_mut(index) {
            let curve = edge
                .curve()
                .map(|curve| curve.try_clone_for_decode(ctx, "step_parameter_inference_edge_curve"))
                .transpose()?;
            edge.carrier = cadmpeg_ir::topology::EdgeCarrier::new(curve, Some(range))
                .map_err(CodecError::malformed)?;
        }
    }
    Ok(())
}

fn curve_endpoint_seed(geometry: &SolvedCurveGeometry, upper: bool, fallback: f64) -> f64 {
    match geometry {
        SolvedCurveGeometry::Nurbs(nurbs) if !nurbs.periodic() => {
            nurbs_curve_parameter_domain(nurbs)
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
                .map_or(
                    fallback,
                    |[lower, upper_bound]| {
                        if upper {
                            upper_bound
                        } else {
                            lower
                        }
                    },
                )
        }
        SolvedCurveGeometry::Transformed(placed) => {
            curve_endpoint_seed(placed.basis(), upper, fallback)
        }
        _ => fallback,
    }
}

fn edge_parameter_range(
    geometry: &SolvedCurveGeometry,
    start: FiniteReal,
    end: FiniteReal,
) -> Option<[f64; 2]> {
    let periodic_domain = match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => {
            Some([0.0, std::f64::consts::TAU])
        }
        SolvedCurveGeometry::Nurbs(nurbs) if nurbs.periodic() => {
            nurbs_curve_parameter_domain(nurbs)
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
        }
        SolvedCurveGeometry::Transformed(placed) => {
            return edge_parameter_range(placed.basis(), start, end);
        }
        _ => None,
    };
    let (start, end) = (start.get(), end.get());
    let Some([lower, upper]) = periodic_domain else {
        return (end > start).then_some([start, end]);
    };
    let period = upper - lower;
    if period <= 0.0 {
        return None;
    }
    if !period.is_finite() {
        let start = cadmpeg_ir::math::wrap_parameter(start, lower, upper)?.get();
        let end = cadmpeg_ir::math::wrap_parameter(end, lower, upper)?.get();
        let sweep = if end >= start {
            end - start
        } else {
            (upper - start) + (end - lower)
        };
        return (sweep > 0.0)
            .then_some(start + sweep)
            .filter(|end| end.is_finite())
            .map(|end| [start, end]);
    }
    let raw_span = end - start;
    let sweep = if raw_span.is_finite() {
        raw_span.rem_euclid(period)
    } else {
        (end.rem_euclid(period) - start.rem_euclid(period)).rem_euclid(period)
    };
    let tolerance = period * EPS_GEOMETRY_READ_GEOMETRY;
    if sweep <= 0.0 || sweep > period + tolerance {
        return None;
    }
    let normalized_start = cadmpeg_ir::math::wrap_parameter(start, lower, upper)?.get();
    let normalized_start = if (normalized_start - upper).abs() <= tolerance {
        lower
    } else {
        normalized_start
    };
    Some([normalized_start, normalized_start + sweep.min(period)])
}

pub(super) struct UnitScales {
    default_length: PositiveReal,
    default_angle: PositiveReal,
    length: BTreeMap<u64, PositiveReal>,
    angle: BTreeMap<u64, PositiveReal>,
}

impl UnitScales {
    pub(super) fn length(&self, ids: impl IntoIterator<Item = u64>) -> PositiveReal {
        ids.into_iter()
            .find_map(|id| self.length.get(&id).copied())
            .unwrap_or(self.default_length)
    }

    pub(super) fn angle(&self, ids: impl IntoIterator<Item = u64>) -> PositiveReal {
        ids.into_iter()
            .find_map(|id| self.angle.get(&id).copied())
            .unwrap_or(self.default_angle)
    }
}

fn resolve_source_curve_parameter_scales(
    exchange: &Exchange,
    unit_scales: &UnitScales,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, FiniteReal>, CodecError> {
    let mut scales = BTreeMap::new();
    for &id in ctx
        .admit_iter(
            exchange.records(),
            "STEP resolve source curve parameter scales map traversal",
        )?
        .map(|(key, _)| key)
    {
        let mut active_storage = ctx.reserve_scoped(0, "step source curve traversal storage")?;
        if let Some(scale) = active_storage.with_storage(|| {
            source_curve_parameter_scale(id, exchange, unit_scales, &mut BTreeSet::new(), ctx)
        })? {
            ctx.insert_btree_map(&mut scales, id, scale, "step_source_curve_parameter_scales")?;
        }
    }
    Ok(scales)
}

fn source_curve_parameter_scale(
    mut id: u64,
    exchange: &Exchange,
    unit_scales: &UnitScales,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<FiniteReal>, CodecError> {
    let _depth = ctx.enter_nested("step_source_curve_parameter_walk")?;
    loop {
        if active.contains(&id) {
            return Ok(None);
        }
        ctx.insert_btree_set(active, id, "step_source_curve_parameter_active")?;
        match source_curve_parameter_scale_value(ctx, id, exchange, unit_scales)? {
            CurveParameterStep::Scale(scale) => return Ok(Some(scale)),
            CurveParameterStep::Parent(parent) => id = parent,
            CurveParameterStep::Absent => return Ok(None),
        }
    }
}

enum CurveParameterStep {
    Scale(FiniteReal),
    Parent(u64),
    Absent,
}

fn source_curve_parameter_scale_value(
    ctx: &DecodeContext<'_>,
    id: u64,
    exchange: &Exchange,
    unit_scales: &UnitScales,
) -> Result<CurveParameterStep, CodecError> {
    let Some(record) = exchange.records().get(&id) else {
        return Ok(CurveParameterStep::Absent);
    };
    if record.partial(ctx, "LINE")?.is_some() {
        let Some(magnitude) = named_parameter(ctx, record, "LINE", 2)?
            .and_then(Value::reference)
            .and_then(|vector| exchange.records().get(&vector))
            .map(|vector| -> Result<_, CodecError> {
                Ok(if vector.partial(ctx, "VECTOR")?.is_some() {
                    Some(vector)
                } else {
                    None
                })
            })
            .transpose()?
            .flatten()
            .map(|vector| named_parameter(ctx, vector, "VECTOR", 2))
            .transpose()?
            .flatten()
            .and_then(Value::number)
            .and_then(PositiveReal::new)
        else {
            return Ok(CurveParameterStep::Absent);
        };
        let scale = magnitude.get() * unit_scales.length([id]).get();
        return Ok(
            FiniteReal::new(scale).map_or(CurveParameterStep::Absent, CurveParameterStep::Scale)
        );
    }
    if record.partial(ctx, "CIRCLE")?.is_some() || record.partial(ctx, "ELLIPSE")?.is_some() {
        return Ok(CurveParameterStep::Scale(FiniteReal::from(
            unit_scales.angle([id]),
        )));
    }
    if record.partial(ctx, "PARABOLA")?.is_some()
        || record.partial(ctx, "HYPERBOLA")?.is_some()
        || record.partial(ctx, "POLYLINE")?.is_some()
        || ctx.any_by(
            &record.partials[..],
            |partial| {
                Ok(matches!(
                    partial.name.as_str(),
                    "B_SPLINE_CURVE_WITH_KNOTS"
                        | "UNIFORM_CURVE"
                        | "QUASI_UNIFORM_CURVE"
                        | "BEZIER_CURVE"
                ))
            },
            "STEP source curve parameter partial traversal",
        )?
    {
        return Ok(CurveParameterStep::Scale(FiniteReal::ONE));
    }
    for name in ["CURVE_REPLICA", "TRIMMED_CURVE", "OFFSET_CURVE_3D"] {
        if let Some(parent) = named_parameter(ctx, record, name, 1)?.and_then(Value::reference) {
            if let Some(parent) = curve_carrier_record(ctx, parent, exchange)? {
                return Ok(CurveParameterStep::Parent(parent));
            }
        }
    }
    if ctx.any_by(
        &record.partials[..],
        |partial| {
            Ok(matches!(
                partial.name.as_str(),
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
            ))
        },
        "STEP source surface curve parameter traversal",
    )? {
        if let Some(parent) = surface_curve_basis(ctx, record)? {
            return Ok(CurveParameterStep::Parent(parent));
        }
    }
    Ok(CurveParameterStep::Absent)
}

pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<StageOutcome<GeometryData>, CodecError> {
    let mut losses = Vec::new();
    let scale = match length_scale(exchange, ctx)? {
        Some(scale) => scale,
        None => {
            ctx.push_vec(&mut losses, StepLossCode::DocumentLengthUnitUnresolved.note(
                    "the document length unit did not resolve; coordinates are unscaled and reported as millimetres",
                ), "step_geometry_losses")?;
            PositiveReal::ONE
        }
    };
    let angle_scale = match plane_angle_scale(exchange, ctx)? {
        Some(scale) => scale,
        None => {
            ctx.push_vec(&mut losses, StepLossCode::DocumentAngleUnitUnresolved.note(
                    "the document plane-angle unit did not resolve; angles are unscaled and reported as radians",
                ), "step_geometry_losses")?;
            PositiveReal::ONE
        }
    };
    let unit_scales = resolve_unit_scales(exchange, scale, angle_scale, &mut losses, ctx)?;
    let angle_scale = angle_scale.get();
    let source_curve_parameter_scales =
        resolve_source_curve_parameter_scales(exchange, &unit_scales, ctx)?;
    let mut typed = BTreeSet::new();
    let mut points = BTreeMap::new();
    let mut points2 = BTreeMap::new();
    let mut apll_point_names = BTreeMap::new();
    let mut directions = BTreeMap::new();
    let mut directions2 = BTreeMap::new();
    let mut vectors = BTreeMap::new();
    let mut vectors2 = BTreeMap::new();
    let mut placements = BTreeMap::new();
    let mut placements2 = BTreeMap::new();
    match linear_uncertainty(exchange, ctx)? {
        LinearUncertainty::Value(uncertainty) => ir.tolerances.linear = uncertainty,
        LinearUncertainty::Empty { unresolved } => {
            if unresolved > 0 {
                ctx.push_vec(&mut losses, StepLossCode::UncertaintyLengthUnresolved.note(format!(
                    "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT has no resolvable length measure and {unresolved} unresolved measure(s); the linear tolerance was not transferred"
                )), "step_geometry_losses")?;
            }
        }
        LinearUncertainty::Ambiguous {
            first,
            second,
            rest,
            unresolved,
        } => {
            let default_linear = ir.tolerances.linear.get();
            let listed = ctx.join_display_retained(
                [first, second]
                    .iter()
                    .chain(&rest)
                    .map(|value| format!("{:?}", value.get())),
                ", ",
                "step_uncertainty_values_text",
            )?;
            let message = ctx.format_retained(format_args!(
                "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT records give {} different linear uncertainty values in millimetres ({listed}) and {unresolved} unresolved measure(s); the linear tolerance keeps the default {default_linear:?}",
                2 + rest.len()
            ), "step_uncertainty_note_text")?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::UncertaintyLengthAmbiguous.note(message),
                "step_geometry_losses",
            )?;
        }
    }

    for entity in exchange.entities_any(
        ctx,
        &[
            "APLL_POINT",
            "APLL_POINT_WITH_SURFACE",
            "CARTESIAN_POINT",
            "DIRECTION",
        ],
    )? {
        let (id, record) = entity?;
        match entity_type(
            ctx,
            record,
            &[
                "APLL_POINT",
                "APLL_POINT_WITH_SURFACE",
                "CARTESIAN_POINT",
                "DIRECTION",
            ],
        )? {
            Some(point_type @ ("APLL_POINT" | "APLL_POINT_WITH_SURFACE")) => {
                let record_scale = unit_scales.length([id]).get();
                if let Some(position) =
                    apll_point_coordinates(ctx, record, point_type, record_scale)?
                {
                    ctx.insert_btree_map(&mut points, id, position, "step_geometry_points")?;
                    let source_name = representation_item_name(ctx, record)?
                        .map(|value| {
                            super::decode_text_charged(
                                exchange,
                                value,
                                &mut losses,
                                id,
                                "APLL point name",
                                StepLossCode::MetadataStringInvalid,
                                ctx,
                            )
                        })
                        .transpose()?
                        .flatten()
                        .filter(|name| !name.is_empty());
                    ctx.insert_btree_map(
                        &mut apll_point_names,
                        id,
                        source_name,
                        "step_geometry_apll_point_names",
                    )?;
                } else {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning.note(ctx.format_retained(
                            format_args!("{point_type} #{id} has invalid coordinates"),
                            "STEP decode text",
                        )?),
                        "step_geometry_losses",
                    )?;
                }
            }
            Some("CARTESIAN_POINT") => {
                let record_scale = unit_scales.length([id]).get();
                if let Some(position) =
                    named_coordinates(ctx, record, "CARTESIAN_POINT", 1, record_scale)?
                {
                    ctx.insert_btree_map(&mut points, id, position, "step_geometry_points")?;
                    ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
                } else if let Some(position) =
                    named_coordinates2(ctx, record, "CARTESIAN_POINT", 1)?
                {
                    ctx.insert_btree_map(&mut points2, id, position, "step_geometry_points2")?;
                    ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
                } else {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning
                            .note(format!("CARTESIAN_POINT #{id} has invalid coordinates")),
                        "step_geometry_losses",
                    )?;
                }
            }
            Some("DIRECTION") => {
                if let Some(direction) =
                    vector3(named_parameter(ctx, record, "DIRECTION", 1)?, 1.0).and_then(normalize)
                {
                    ctx.insert_btree_map(
                        &mut directions,
                        id,
                        direction,
                        "step_geometry_directions",
                    )?;
                    ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
                } else if let Some(direction) =
                    direction2(named_parameter(ctx, record, "DIRECTION", 1)?)
                {
                    ctx.insert_btree_map(
                        &mut directions2,
                        id,
                        direction,
                        "step_geometry_directions2",
                    )?;
                    ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
                } else {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning
                            .note(format!("DIRECTION #{id} is invalid or zero")),
                        "step_geometry_losses",
                    )?;
                }
            }
            _ => {}
        }
    }
    decode_tessellated_curve_sets(exchange, &unit_scales, ir, &mut typed, &mut losses, ctx)?;
    let mut point_carriers = BTreeSet::new();
    for record in ctx
        .admit_iter(exchange.records(), "STEP decode map traversal")?
        .map(|(_, value)| value)
    {
        if record.partial(ctx, "VERTEX_POINT")?.is_some() {
            if let Some(id) = vertex_point_reference(ctx, record)? {
                ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
            }
        }
        if ctx.any_by(
            &record.partials[..],
            |partial| Ok(super::representation::is_representation_name(&partial.name)),
            "STEP decode traversal",
        )? {
            if let Some(items) = representation_items(ctx, record)? {
                for id in items.filter(|id| points.contains_key(id)) {
                    ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
                }
            }
        }
        if ctx.any_by(
            &record.partials[..],
            |partial| {
                Ok(matches!(
                    partial.name.as_str(),
                    "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
                ))
            },
            "STEP decode traversal",
        )? {
            if let Some(items) =
                first_named_list(ctx, record, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])?
            {
                for id in items.filter(|id| points.contains_key(id)) {
                    ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
                }
            }
        }
        if record.partial(ctx, "POLY_LOOP")?.is_some() {
            if let Some(items) = first_named_list(ctx, record, &["POLY_LOOP"])? {
                for id in items.filter(|id| points.contains_key(id)) {
                    ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
                }
            }
        }
        if is_apll_leader_line(ctx, record)? {
            for partial in ctx.admit_iter(&record.partials[..], "STEP decode traversal")? {
                for parameter in ctx.admit_iter(
                    partial.parameters.as_slice(),
                    "STEP record parameter traversal",
                )? {
                    for id in super::reference::references(parameter, ctx) {
                        let id = id?;
                        if points.contains_key(&id) {
                            ctx.insert_btree_set(
                                &mut point_carriers,
                                id,
                                "step_geometry_point_carriers",
                            )?;
                        }
                    }
                }
            }
        }
        if let Some(item) = ctx
            .find_by(
                &record.partials[..],
                |partial| {
                    Ok(matches!(
                        partial.name.as_str(),
                        "GEOMETRIC_ITEM_SPECIFIC_USAGE" | "ITEM_IDENTIFIED_REPRESENTATION_USAGE"
                    ))
                },
                "STEP decode traversal",
            )?
            .and_then(|partial| partial.parameters.get(4))
            .and_then(Value::reference)
        {
            if points.contains_key(&item) {
                ctx.insert_btree_set(&mut point_carriers, item, "step_geometry_point_carriers")?;
            }
        }
        if let Some(id) = super::presentation::styled_item_target(ctx, record)? {
            if points.contains_key(&id) {
                ctx.insert_btree_set(&mut point_carriers, id, "step_geometry_point_carriers")?;
            }
        }
    }
    for id in point_carriers {
        let Some(position) = points.get(&id).copied() else {
            continue;
        };
        let source_name = apll_point_names.remove(&id);
        ctx.push_vec(
            &mut ir.model.points,
            Point::new(
                PointId::from(ids::data(kind!("point"), id)),
                position,
                source_name
                    .map(|name| super::step_source_association(ctx, id, name))
                    .transpose()?,
            ),
            "step_geometry_ir_points",
        )?;
    }
    for (id, record) in exchange.entities(ctx, "VECTOR")? {
        if record.partial(ctx, "VECTOR")?.is_some() {
            let record_scale = unit_scales.length([id]).get();
            let value = named_parameter(ctx, record, "VECTOR", 1)?
                .and_then(Value::reference)
                .and_then(|direction| directions.get(&direction).copied())
                .zip(named_parameter(ctx, record, "VECTOR", 2)?.and_then(Value::number))
                .map(|(direction, magnitude)| direction.as_raw().scale(magnitude * record_scale));
            let value2 = named_parameter(ctx, record, "VECTOR", 1)?
                .and_then(Value::reference)
                .and_then(|direction| directions2.get(&direction).copied())
                .zip(named_parameter(ctx, record, "VECTOR", 2)?.and_then(Value::number))
                .map(|(direction, magnitude)| {
                    let [u, v] = direction.get();
                    Point2::new(u * magnitude, v * magnitude)
                });
            if let Some(value) = value {
                ctx.insert_btree_map(&mut vectors, id, value, "step_geometry_vectors")?;
                ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            } else if let Some(value) = value2 {
                ctx.insert_btree_map(&mut vectors2, id, value, "step_geometry_vectors2")?;
                ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            } else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "VECTOR #{id} has an invalid direction or magnitude"
                    )),
                    "step_geometry_losses",
                )?;
            }
        }
    }
    for entity in exchange.entities_any(ctx, &["AXIS2_PLACEMENT_3D", "AXIS1_PLACEMENT"])? {
        let (id, record) = entity?;
        let Some(placement_type) =
            entity_type(ctx, record, &["AXIS2_PLACEMENT_3D", "AXIS1_PLACEMENT"])?
        else {
            continue;
        };
        {
            let mut inferred_reference = false;
            let placement = named_parameter(ctx, record, placement_type, 1)?
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .map(|origin| {
                    let axis = optional_direction(
                        named_parameter(ctx, record, placement_type, 2)?,
                        &directions,
                    )
                    .unwrap_or(UnitVector3::Z_AXIS);
                    let reference = match optional_direction(
                        named_parameter(ctx, record, placement_type, 3)?,
                        &directions,
                    ) {
                        Some(reference) => {
                            if let Some(reference) = orthogonal_reference(axis, reference) {
                                reference
                            } else {
                                inferred_reference = true;
                                first_projected_axis(axis).unwrap_or(UnitVector3::X_AXIS)
                            }
                        }
                        None => first_projected_axis(axis).unwrap_or(UnitVector3::X_AXIS),
                    };
                    Ok::<_, CodecError>((origin, axis, reference))
                })
                .transpose()?;
            if inferred_reference {
                ctx.push_vec(&mut losses, StepLossCode::PlacementReferenceInferred.note(format!(
                        "AXIS2_PLACEMENT_3D #{id} has a reference direction parallel to its axis; inferred an orthogonal reference"
                    )), "step_geometry_losses")?;
            }
            if let Some(placement) = placement {
                ctx.insert_btree_map(&mut placements, id, placement, "step_geometry_placements")?;
                ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            } else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning
                        .note(format!("AXIS2_PLACEMENT_3D #{id} has an invalid location")),
                    "step_geometry_losses",
                )?;
            }
        }
    }
    let mut transformation_operators = BTreeMap::new();
    for (id, record) in exchange.entities(ctx, "CARTESIAN_TRANSFORMATION_OPERATOR_3D")? {
        if let Some(transform) =
            cartesian_transformation_operator(ctx, record, &points, &directions)?
        {
            ctx.insert_btree_map(
                &mut transformation_operators,
                id,
                transform,
                "step_geometry_transformation_operators",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "CARTESIAN_TRANSFORMATION_OPERATOR_3D #{id} has invalid axes, origin, or scale"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    let mut transformation_operators2 = BTreeMap::new();
    for (id, record) in exchange.entities(ctx, "CARTESIAN_TRANSFORMATION_OPERATOR_2D")? {
        if let Some(transform) =
            cartesian_transformation_operator_2d(ctx, record, &points2, &directions2)?
        {
            ctx.insert_btree_map(
                &mut transformation_operators2,
                id,
                transform,
                "step_geometry_transformation_operators2",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "CARTESIAN_TRANSFORMATION_OPERATOR_2D #{id} has invalid axes, origin, or scale"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, record) in exchange.entities(ctx, "AXIS2_PLACEMENT_2D")? {
        if record.partial(ctx, "AXIS2_PLACEMENT_2D")?.is_none() {
            continue;
        }
        let placement = if let Some(origin) = named_parameter(ctx, record, "AXIS2_PLACEMENT_2D", 1)?
            .and_then(Value::reference)
            .and_then(|point| points2.get(&point).copied())
        {
            match named_parameter(ctx, record, "AXIS2_PLACEMENT_2D", 2)? {
                Some(Value::Reference(direction)) => directions2
                    .get(direction)
                    .copied()
                    .map(|x_axis| (origin, x_axis, x_axis.quarter_turn())),
                Some(Value::Omitted) | None => Some((
                    origin,
                    HypotDirection2::X_AXIS,
                    HypotDirection2::X_AXIS.quarter_turn(),
                )),
                _ => None,
            }
        } else {
            None
        };
        if let Some(placement) = placement {
            ctx.insert_btree_map(&mut placements2, id, placement, "step_geometry_placements2")?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning
                    .note(format!("AXIS2_PLACEMENT_2D #{id} has an invalid location")),
                "step_geometry_losses",
            )?;
        }
    }
    let mut pcurve_geometries = BTreeMap::<u64, (PcurveGeometry, BTreeSet<u64>)>::new();
    let mut pcurve_geometry_records = BTreeSet::new();
    for (_, record) in exchange.entities(ctx, "PCURVE")? {
        if record.partial(ctx, "PCURVE")?.is_none() {
            continue;
        }
        let representation_id =
            named_parameter(ctx, record, "PCURVE", 2)?.and_then(Value::reference);
        let Some(items) = representation_id
            .and_then(|representation| exchange.records().get(&representation))
            .map(|record| representation_items(ctx, record))
            .transpose()?
            .flatten()
        else {
            continue;
        };
        let pcurve_angle_scale = representation_id.map_or(angle_scale, |representation| {
            unit_scales.angle([representation]).get()
        });
        let mut decoded = None;
        let mut decoded_count = 0;
        for curve in items {
            if let Some(geometry) = decode_pcurve_geometry(
                curve,
                exchange,
                PcurveSources {
                    points: &points2,
                    vectors: &vectors2,
                    placements: &placements2,
                    transformations: &transformation_operators2,
                    angle_scale: pcurve_angle_scale,
                },
                &mut losses,
                &mut BTreeSet::new(),
                0,
                ctx,
            )? {
                decoded_count += 1;
                decoded = Some((curve, geometry));
            }
        }
        if decoded_count == 1 {
            if let Some((curve, (geometry, records))) = decoded {
                for &record in &records {
                    ctx.insert_btree_set(
                        &mut pcurve_geometry_records,
                        record,
                        "step_pcurve_geometry_records",
                    )?;
                }
                ctx.insert_btree_map(
                    &mut pcurve_geometries,
                    curve,
                    (geometry, records),
                    "step_pcurve_geometries",
                )?;
            }
        }
    }
    let mut curve_parameter_offsets = BTreeMap::<u64, f64>::new();
    for entity in exchange.entities_any(ctx, LeafCurveEntity::NAMES)? {
        let (id, record) = entity?;
        let Some(curve_kind) = LeafCurveEntity::of(ctx, record)? else {
            continue;
        };
        if pcurve_geometry_records.contains(&id) {
            continue;
        }
        if curve_kind == LeafCurveEntity::BSplineWithKnots && record.simple_name().is_none() {
            continue;
        }
        let record_scale = unit_scales.length([id]).get();
        let parameter_offset = if curve_kind == LeafCurveEntity::Ellipse {
            let first_radius = named_parameter(ctx, record, "ELLIPSE", 2)?.and_then(Value::number);
            let second_radius = named_parameter(ctx, record, "ELLIPSE", 3)?.and_then(Value::number);
            first_radius
                .zip(second_radius)
                .filter(|(first, second)| first.is_finite() && second.is_finite())
                .and_then(|(first, second)| {
                    (first < second).then_some(-std::f64::consts::FRAC_PI_2)
                })
        } else {
            None
        };
        let geometry = match curve_kind {
            LeafCurveEntity::Line => named_parameter(ctx, record, "LINE", 1)?
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .zip(
                    named_parameter(ctx, record, "LINE", 2)?
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied())
                        .and_then(normalize),
                )
                .map(|(origin, direction)| {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::new(origin, direction),
                    ))
                }),
            LeafCurveEntity::Circle => named_parameter(ctx, record, "CIRCLE", 1)?
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(ctx, record, "CIRCLE", 2)?.and_then(Value::number))
                .and_then(|((center, axis, ref_direction), radius)| {
                    let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                    let radius = PositiveLength::new(radius * record_scale)?;
                    Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                        cadmpeg_ir::geometry::analytic::CircleCurve::new(center, frame, radius),
                    )))
                }),
            LeafCurveEntity::Ellipse => named_parameter(ctx, record, "ELLIPSE", 1)?
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(ctx, record, "ELLIPSE", 2)?.and_then(Value::number))
                .zip(named_parameter(ctx, record, "ELLIPSE", 3)?.and_then(Value::number))
                .and_then(
                    |(((center, axis, reference_direction), first_radius), second_radius)| {
                        let first_radius = first_radius * record_scale;
                        let second_radius = second_radius * record_scale;
                        let (major_direction, major_radius, minor_radius) =
                            if first_radius >= second_radius {
                                (*reference_direction.as_raw(), first_radius, second_radius)
                            } else {
                                // STEP ELLIPSE stores two ordered semiaxes;
                                // neither position is required to be the
                                // longer one. The IR ellipse is canonicalized
                                // around its semi-major direction.
                                (
                                    axis.as_raw().cross(*reference_direction.as_raw()),
                                    second_radius,
                                    first_radius,
                                )
                            };
                        let major_direction = UnitVector3::new(major_direction)?;
                        let frame = OrthonormalFrame3::from_units(axis, major_direction)?;
                        let major_radius = PositiveLength::new(major_radius)?;
                        let minor_radius = PositiveLength::new(minor_radius)?;
                        cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                            center,
                            frame,
                            major_radius,
                            minor_radius,
                        )
                        .ok()
                        .map(SolvedCurveGeometry::Ellipse)
                        .map(CurveGeometry::Solved)
                    },
                ),
            LeafCurveEntity::Parabola => named_parameter(ctx, record, "PARABOLA", 1)?
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(ctx, record, "PARABOLA", 2)?.and_then(Value::number))
                .and_then(|((vertex, axis, major_direction), focal_distance)| {
                    let frame = OrthonormalFrame3::from_units(axis, major_direction)?;
                    let focal_distance = PositiveLength::new(focal_distance * record_scale)?;
                    Some(CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
                        cadmpeg_ir::geometry::analytic::ParabolaCurve::new(
                            vertex,
                            frame,
                            focal_distance,
                        ),
                    )))
                }),
            LeafCurveEntity::Hyperbola => named_parameter(ctx, record, "HYPERBOLA", 1)?
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(ctx, record, "HYPERBOLA", 2)?.and_then(Value::number))
                .zip(named_parameter(ctx, record, "HYPERBOLA", 3)?.and_then(Value::number))
                .and_then(
                    |(((center, axis, major_direction), major_radius), minor_radius)| {
                        let frame = OrthonormalFrame3::from_units(axis, major_direction)?;
                        let major_radius = PositiveLength::new(major_radius * record_scale)?;
                        let minor_radius = PositiveLength::new(minor_radius * record_scale)?;
                        Some(CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
                            cadmpeg_ir::geometry::analytic::HyperbolaCurve::new(
                                center,
                                frame,
                                major_radius,
                                minor_radius,
                            ),
                        )))
                    },
                ),
            LeafCurveEntity::Polyline => polyline(id, record, &points, &mut losses, ctx)?
                .map(SolvedCurveGeometry::Nurbs)
                .map(CurveGeometry::Solved),
            LeafCurveEntity::BSplineWithKnots
            | LeafCurveEntity::UniformCurve
            | LeafCurveEntity::QuasiUniformCurve
            | LeafCurveEntity::BezierCurve => nurbs_curve(id, record, &points, &mut losses, ctx)?
                .map(SolvedCurveGeometry::Nurbs)
                .map(CurveGeometry::Solved),
        };
        if let Some(geometry) = geometry {
            if let Some(offset) = parameter_offset {
                ctx.insert_btree_map(
                    &mut curve_parameter_offsets,
                    id,
                    offset,
                    "step_geometry_curve_parameter_offsets",
                )?;
            }
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: CurveId::from(ids::data(kind!("curve"), id)),
                    geometry,
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("{} #{id} has invalid geometry", curve_kind.name()),
                    "STEP decode text",
                )?),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, record) in exchange.entities(ctx, "B_SPLINE_CURVE_WITH_KNOTS")? {
        if record.partial(ctx, "B_SPLINE_CURVE_WITH_KNOTS")?.is_none()
            || record.simple_name() == Some("B_SPLINE_CURVE_WITH_KNOTS")
            || pcurve_geometry_records.contains(&id)
        {
            continue;
        }
        if let Some(nurbs) = nurbs_curve(id, record, &points, &mut losses, ctx)? {
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: CurveId::from(ids::data(kind!("curve"), id)),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "B_SPLINE_CURVE_WITH_KNOTS #{id} has invalid geometry"
                )),
                "step_geometry_losses",
            )?;
        }
    }

    // STEP geometry is a graph, not an ordered stream. Resolve all deferred
    // curve constructors to a fixpoint so nested or forward references do not
    // disappear merely because their source record has a larger instance id.
    let mut carrier_index = CarrierIndex::from_ir(ir, ctx)?;
    let mut deferred_ids = Vec::new();
    for entity in exchange
        .entities_any(
            ctx,
            &[
                "CURVE_REPLICA",
                "TRIMMED_CURVE",
                "COMPOSITE_CURVE",
                "BOUNDARY_CURVE",
                "OUTER_BOUNDARY_CURVE",
                "OFFSET_CURVE_3D",
            ],
        )?
        .filter(|entry| match entry {
            Ok((id, _)) => !pcurve_geometry_records.contains(id),
            Err(_) => true,
        })
    {
        let (id, _) = entity?;
        ctx.push_vec(&mut deferred_ids, id, "step_deferred_curve_ids")?;
    }
    let mut deferred_queue = VecDeque::from(deferred_ids);
    let mut waiting_on = HashMap::<u64, Vec<u64>>::new();
    while let Some(id) = deferred_queue.pop_front() {
        if carrier_index.curves.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if let Some(parent_reference_step) = record
            .partial(ctx, "CURVE_REPLICA")?
            .map(|_| named_parameter(ctx, record, "CURVE_REPLICA", 1))
            .transpose()?
            .flatten()
            .and_then(Value::reference)
        {
            let Some(parent_step) = curve_carrier_record(ctx, parent_reference_step, exchange)?
            else {
                continue;
            };
            let Some(operator_step) =
                named_parameter(ctx, record, "CURVE_REPLICA", 2)?.and_then(Value::reference)
            else {
                continue;
            };
            let Some(parent_index) = carrier_index.curves.get(&parent_step).copied() else {
                defer_geometry_dependency(
                    &mut waiting_on,
                    parent_step,
                    id,
                    ctx,
                    "step_deferred_curve_groups",
                    "step_deferred_curve_members",
                )?;
                continue;
            };
            let Some(transform) = transformation_operators.get(&operator_step).copied() else {
                continue;
            };
            let Some(basis) = ir
                .model
                .curves
                .get(parent_index.0)
                .and_then(|curve| curve.geometry.solved())
            else {
                continue;
            };
            let basis = basis.try_clone_for_decode(ctx, "step_curve_replica_basis")?;
            let Ok(placed) = cadmpeg_ir::geometry::PlacedCurve::try_new(Box::new(basis), transform)
            else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "CURVE_REPLICA #{id} nests past the admitted inline basis depth"
                    )),
                    "step_geometry_losses",
                )?;
                continue;
            };
            let geometry = SolvedCurveGeometry::Transformed(placed);
            let curve_index = CurveIndex(ir.model.curves.len());
            let curve = CurveId::from(ids::data(kind!("curve"), id));
            let procedural = ProceduralCurve::new(
                ProceduralCurveId::from(ids::construction(kind!("curve_replica"), id)),
                ProceduralCurveDefinition::Replica {
                    source: CurveId::from(ids::data(kind!("curve"), parent_step)),
                    transform,
                },
            );
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                    geometry: CurveGeometry::Solved(geometry),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            let _attached = ir.model.add_procedural_curve(ctx, &curve, procedural)?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
            if let Some(offset) = curve_parameter_offsets.get(&parent_step).copied() {
                ctx.insert_btree_map(
                    &mut curve_parameter_offsets,
                    id,
                    offset,
                    "step_geometry_curve_parameter_offsets",
                )?;
            }
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            ctx.insert_btree_set(&mut typed, operator_step, "step_geometry_typed_ids")?;
            wake_deferred_dependents(
                id,
                &mut waiting_on,
                &mut deferred_queue,
                ctx,
                "step_deferred_curve_queue",
            )?;
            continue;
        }
        if let Some(parameters) = entity_parameters(ctx, record, "TRIMMED_CURVE")? {
            let Some((basis_reference_step, sense, master_representation)) =
                trimmed_curve_attributes(parameters)
            else {
                continue;
            };
            let Some(basis_step) = curve_carrier_record(ctx, basis_reference_step, exchange)?
            else {
                continue;
            };
            if !carrier_index.curves.contains_key(&basis_step) {
                defer_geometry_dependency(
                    &mut waiting_on,
                    basis_step,
                    id,
                    ctx,
                    "step_deferred_curve_groups",
                    "step_deferred_curve_members",
                )?;
                continue;
            }
            let curve = CurveId::from(ids::data(kind!("curve"), id));
            let basis = CurveId::from(ids::data(kind!("curve"), basis_step));
            let Some(geometry) = carrier_index
                .curves
                .get(&basis_step)
                .and_then(|index| ir.model.curves.get(index.0))
                .map(|candidate| &candidate.geometry)
            else {
                continue;
            };
            let Some(solved_geometry) = geometry.solved() else {
                continue;
            };
            let record_scale = unit_scales.length([id]);
            let record_angle_scale = unit_scales.angle([id]).get();
            let parameter_offset = curve_parameter_offsets
                .get(&basis_step)
                .copied()
                .unwrap_or(0.0);
            let linear_parameter_scale = line_parameter_scale(
                exchange,
                basis_reference_step,
                record_scale,
                &mut losses,
                ctx,
            )?;
            let (start, end) = {
                let mut trim_context = TrimParameterContext {
                    points: &points,
                    geometry,
                    angle_scale: record_angle_scale,
                    linear_parameter_scale: linear_parameter_scale.get(),
                    parameter_offset,
                    tolerance: ir.tolerances.linear.get(),
                    master_representation,
                    record_id: id,
                    losses: &mut losses,
                    ctx,
                };
                (
                    match parameters.get(2) {
                        Some(value) => trim_parameter(value, &mut trim_context)?,
                        None => None,
                    },
                    match parameters.get(3) {
                        Some(value) => trim_parameter(value, &mut trim_context)?,
                        None => None,
                    },
                )
            };
            let Some((start, end)) = start.zip(end) else {
                continue;
            };
            let parameter_range = trimmed_curve_parameter_range(geometry, start, end, sense);
            let procedural =
                match cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction::try_new(
                    basis,
                    parameter_range,
                    sense,
                    None,
                )
                .and_then(|admitted_payload| {
                    let mut definition = ProceduralCurveDefinition::Subset(admitted_payload);
                    definition.set_legacy_cache(cadmpeg_ir::geometry::LegacyCache::new(
                        cadmpeg_ir::geometry::FitTolerance::ZERO,
                    ))?;
                    Ok(ProceduralCurve::new(
                        ProceduralCurveId::from(ids::construction(kind!("trimmed_curve"), id)),
                        definition,
                    ))
                }) {
                    Ok(procedural) => procedural,
                    Err(error) => {
                        ctx.push_vec(
                            &mut losses,
                            StepLossCode::DecodeWarning.note(ctx.format_retained(
                                format_args!("TRIMMED_CURVE #{id}: {error}"),
                                "STEP decode text",
                            )?),
                            "step_geometry_losses",
                        )?;
                        continue;
                    }
                };
            let curve_index = CurveIndex(ir.model.curves.len());
            let copied_geometry =
                solved_geometry.try_clone_for_decode(ctx, "step_trim_curve_carrier")?;
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                    geometry: CurveGeometry::Solved(copied_geometry),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;

            let _attached = ir.model.add_procedural_curve(ctx, &curve, procedural)?;

            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
            if parameter_offset != 0.0 {
                ctx.insert_btree_map(
                    &mut curve_parameter_offsets,
                    id,
                    parameter_offset,
                    "step_geometry_curve_parameter_offsets",
                )?;
            }
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            wake_deferred_dependents(
                id,
                &mut waiting_on,
                &mut deferred_queue,
                ctx,
                "step_deferred_curve_queue",
            )?;
            continue;
        }
        if composite_curve_parameters(ctx, record)?.is_some() {
            let mut missing = false;
            composite_curve_dependencies(record, exchange, ctx, &mut |dependency| {
                if !carrier_index.curves.contains_key(&dependency) {
                    defer_geometry_dependency(
                        &mut waiting_on,
                        dependency,
                        id,
                        ctx,
                        "step_deferred_curve_groups",
                        "step_deferred_curve_members",
                    )?;
                    missing = true;
                }
                Ok(())
            })?;
            if missing {
                continue;
            }
            let Some((segments, self_intersect)) =
                composite_curve(record, exchange, &carrier_index, ctx)?
            else {
                continue;
            };
            let curve = CurveId::from(ids::data(kind!("curve"), id));
            for &(segment, _) in &segments {
                ctx.insert_btree_set(&mut typed, segment, "step_geometry_typed_ids")?;
            }
            let mut model_segments = Vec::new();
            for (_, segment) in segments {
                ctx.push_vec(
                    &mut model_segments,
                    segment,
                    "step_composite_curve_model_segments",
                )?;
            }
            let segments =
                match cadmpeg_ir::geometry::CompositeCurveSegments::try_from(model_segments) {
                    Ok(segments) => segments,
                    Err(error) => {
                        ctx.push_vec(
                            &mut losses,
                            StepLossCode::DecodeWarning.note(ctx.format_retained(
                                format_args!("COMPOSITE_CURVE #{id}: {error}"),
                                "STEP decode text",
                            )?),
                            "step_geometry_losses",
                        )?;
                        continue;
                    }
                };
            let curve_index = CurveIndex(ir.model.curves.len());
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                        segments,
                        self_intersect,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            wake_deferred_dependents(
                id,
                &mut waiting_on,
                &mut deferred_queue,
                ctx,
                "step_deferred_curve_queue",
            )?;
            continue;
        }
        let Some(parameters) = entity_parameters(ctx, record, "OFFSET_CURVE_3D")? else {
            continue;
        };
        let source_reference_step = parameters.get(1).and_then(Value::reference);
        let source_step = source_reference_step
            .map(|source| curve_carrier_record(ctx, source, exchange))
            .transpose()?
            .flatten();
        let source = source_step.map(|source| CurveId::from(ids::data(kind!("curve"), source)));
        let distance = parameters.get(2).and_then(Value::number);
        let self_intersect = parameters
            .get(3)
            .and_then(|value| logical_value(value).ok());
        let reference_direction = parameters
            .get(4)
            .and_then(Value::reference)
            .and_then(|direction| directions.get(&direction).copied());
        let Some((source, distance, self_intersect, reference_direction)) = source
            .zip(distance)
            .zip(self_intersect)
            .zip(reference_direction)
            .map(|(((source, distance), self_intersect), direction)| {
                (source, distance, self_intersect, direction)
            })
        else {
            continue;
        };
        let Some(source_step) = source_step else {
            continue;
        };
        if !carrier_index.curves.contains_key(&source_step) {
            defer_geometry_dependency(
                &mut waiting_on,
                source_step,
                id,
                ctx,
                "step_deferred_curve_groups",
                "step_deferred_curve_members",
            )?;
            continue;
        }
        let Some(geometry) = carrier_index
            .curves
            .get(&source_step)
            .and_then(|index| ir.model.curves.get(index.0))
            .and_then(|candidate| candidate.geometry.solved())
        else {
            continue;
        };
        let curve = CurveId::from(ids::data(kind!("curve"), id));
        let curve_index = CurveIndex(ir.model.curves.len());
        let copied_geometry = geometry.try_clone_for_decode(ctx, "step_offset_curve_carrier")?;
        let procedural =
            match cadmpeg_ir::geometry::curve_payloads::SpatialOffsetCurveConstruction::try_from_parts(
                source,
                distance * unit_scales.length([id]).get(),
                reference_direction,
                self_intersect,
            )
            .map(|admitted_payload| {
                ProceduralCurve::new(
                    ProceduralCurveId::from(ids::construction(kind!("offset_curve"), id)),
                    ProceduralCurveDefinition::SpatialOffset(admitted_payload),
                )
            }) {
                Ok(procedural) => procedural,
                Err(error) => {
                    ctx.push_vec(&mut losses, StepLossCode::DecodeWarning
                            .note(ctx.format_retained(format_args!("curve construction #{id}: {error}"), "STEP decode text")?), "step_geometry_losses")?;
                    continue;
                }
            };
        ctx.push_vec(
            &mut ir.model.curves,
            Curve {
                id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                geometry: CurveGeometry::Solved(copied_geometry),
                source_object: None,
            },
            "step_geometry_ir_curves",
        )?;
        let _attached = ir.model.add_procedural_curve(ctx, &curve, procedural)?;
        ctx.insert_hash_map(
            &mut carrier_index.curves,
            id,
            curve_index,
            "step_geometry_curve_index",
        )?;
        if let Some(offset) = curve_parameter_offsets.get(&source_step).copied() {
            ctx.insert_btree_map(
                &mut curve_parameter_offsets,
                id,
                offset,
                "step_geometry_curve_parameter_offsets",
            )?;
        }
        ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        wake_deferred_dependents(
            id,
            &mut waiting_on,
            &mut deferred_queue,
            ctx,
            "step_deferred_curve_queue",
        )?;
    }
    for (id, _) in exchange.entities(ctx, "CURVE_REPLICA")? {
        if !carrier_index.curves.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "CURVE_REPLICA #{id} has invalid or unresolved parent/operator"
                )),
                "step_geometry_losses",
            )?;
            let curve_index = CurveIndex(ir.model.curves.len());
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: CurveId::from(ids::data(kind!("curve"), id)),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&id)
                            .map(|record| opaque_record_id(id, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
        }
    }
    for (id, _) in exchange
        .entities(ctx, "TRIMMED_CURVE")?
        .filter(|(id, _)| !pcurve_geometry_records.contains(id))
    {
        if !carrier_index.curves.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "TRIMMED_CURVE #{id} has invalid or unresolved basis/trim selectors"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for entity in exchange.entities_any(
        ctx,
        &["COMPOSITE_CURVE", "BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"],
    )? {
        let (id, record) = entity?;
        if !carrier_index.curves.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "{} #{id} has invalid, cyclic, or unresolved segments",
                        record.simple_name().unwrap_or("COMPOSITE_CURVE")
                    ),
                    "STEP decode text",
                )?),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, _) in exchange.entities(ctx, "OFFSET_CURVE_3D")? {
        if !carrier_index.curves.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "OFFSET_CURVE_3D #{id} has invalid or unresolved basis parameters"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for entity in exchange
        .entities_any(
            ctx,
            &[
                "TRIMMED_CURVE",
                "COMPOSITE_CURVE",
                "BOUNDARY_CURVE",
                "OUTER_BOUNDARY_CURVE",
                "OFFSET_CURVE_3D",
            ],
        )?
        .filter(|entry| match entry {
            Ok((id, _)) => !pcurve_geometry_records.contains(id),
            Err(_) => true,
        })
    {
        let (id, _) = entity?;
        if !carrier_index.curves.contains_key(&id) {
            let curve = CurveId::from(ids::data(kind!("curve"), id));
            let curve_index = CurveIndex(ir.model.curves.len());
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: curve.try_clone_for_decode(ctx, "step_curve_identity_copy")?,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&id)
                            .map(|record| opaque_record_id(id, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "retained unresolved deferred curve #{id} as an unknown carrier"
                )),
                "step_geometry_losses",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                id,
                curve_index,
                "step_geometry_curve_index",
            )?;
        }
    }
    for entity in
        exchange.entities_any(ctx, &["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"])?
    {
        let (id, record) = entity?;
        let Some(basis) = surface_curve_basis(ctx, record)? else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "{} #{id} has no decoded 3D curve",
                        record.simple_name().unwrap_or("SURFACE_CURVE")
                    ),
                    "STEP decode text",
                )?),
                "step_geometry_losses",
            )?;
            continue;
        };
        if carrier_index.curves.contains_key(&basis) {
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "{} #{id} has no decoded 3D curve",
                        record.simple_name().unwrap_or("SURFACE_CURVE")
                    ),
                    "STEP decode text",
                )?),
                "step_geometry_losses",
            )?;
        }
    }

    for entity in exchange.entities_any(
        ctx,
        &["SURFACE_OF_LINEAR_EXTRUSION", "SURFACE_OF_REVOLUTION"],
    )? {
        let (id, record) = entity?;
        let (surface_type, definition) = match entity_type(ctx,
            record,
            &["SURFACE_OF_LINEAR_EXTRUSION", "SURFACE_OF_REVOLUTION"],
        )? {
            Some(surface_type @ "SURFACE_OF_LINEAR_EXTRUSION") => (surface_type,
                named_parameter(ctx, record, "SURFACE_OF_LINEAR_EXTRUSION", 1)?
                    .and_then(Value::reference)
                    .filter(|curve| carrier_index.curves.contains_key(curve))
                    .map(|curve| CurveId::from(ids::data(kind!("curve"), curve)))
                    .zip(
                        named_parameter(ctx, record, "SURFACE_OF_LINEAR_EXTRUSION", 2)?
                            .and_then(Value::reference)
                            .and_then(|vector| vectors.get(&vector).copied()),
                    )
                    .map(|(directrix, direction)| {
                        cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                            directrix, direction,
                        )
                        .map(ProceduralSurfaceDefinition::LinearSweep)
                    })
            ),
            Some(surface_type @ "SURFACE_OF_REVOLUTION") => (surface_type, named_parameter(ctx, record, "SURFACE_OF_REVOLUTION", 1)?
                .and_then(Value::reference)
                .filter(|curve| carrier_index.curves.contains_key(curve))
                .map(|curve| CurveId::from(ids::data(kind!("curve"), curve)))
                .zip(
                    named_parameter(ctx, record, "SURFACE_OF_REVOLUTION", 2)?
                        .and_then(Value::reference)
                        .and_then(|placement| placements.get(&placement).copied()),
                )
                .map(|(directrix, (axis_origin, axis_direction, _))| {
                    let construction =
                        cadmpeg_ir::geometry::surface_payloads::AxisRevolutionSurfaceConstruction::from_parts(
                            directrix,
                            axis_origin,
                            axis_direction,
                        );
                    Ok(ProceduralSurfaceDefinition::AxisRevolution(construction))
                })),
            _ => continue,
        };
        let Some(definition) = definition else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "{surface_type} #{id} has an unresolved directrix, vector, or axis"
                    ),
                    "STEP decode text",
                )?),
                "step_geometry_losses",
            )?;
            continue;
        };
        let definition = match definition {
            Ok(definition) => definition,
            Err(error) => {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(ctx.format_retained(
                        format_args!("procedural surface #{id}: {error}"),
                        "STEP decode text",
                    )?),
                    "step_geometry_losses",
                )?;
                continue;
            }
        };
        let surface = SurfaceId::from(ids::data(kind!("surface"), id));
        ctx.push_vec(
            &mut ir.model.surfaces,
            Surface {
                id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
            },
            "step_geometry_ir_surfaces",
        )?;
        let _attached = ir.model.add_procedural_surface(
            ctx,
            &surface,
            ProceduralSurface::new(
                ProceduralSurfaceId::from(ids::construction(kind!("swept_surface"), id)),
                definition,
                None,
            ),
        )?;
        ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
    }

    for entity in exchange.entities_any(ctx, LeafSurfaceEntity::NAMES)? {
        let (id, record) = entity?;
        let Some(surface_kind) = LeafSurfaceEntity::of(ctx, record)? else {
            continue;
        };
        let surface_type = surface_kind.name();
        if surface_kind == LeafSurfaceEntity::BSplineWithKnots && record.simple_name().is_none() {
            continue;
        }
        let record_scale = unit_scales.length([id]).get();
        let record_angle_scale = unit_scales.angle([id]).get();
        let placement = named_parameter(ctx, record, surface_type, 1)?
            .and_then(Value::reference)
            .and_then(|placement| placements.get(&placement).copied());
        let geometry = match surface_kind {
            LeafSurfaceEntity::Plane => placement.and_then(|(origin, normal, u_axis)| {
                let frame = OrthonormalFrame3::from_units(normal, u_axis)?;
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::new(origin, frame),
                )))
            }),
            LeafSurfaceEntity::Cylindrical => placement
                .zip(
                    named_parameter(ctx, record, "CYLINDRICAL_SURFACE", 2)?.and_then(Value::number),
                )
                .and_then(|((origin, axis, ref_direction), radius)| {
                    let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                    let radius = PositiveLength::new(radius * record_scale)?;
                    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                        cadmpeg_ir::geometry::analytic::CylinderSurface::new(origin, frame, radius),
                    )))
                }),
            // ISO 10303-42 `conical_surface` holds `semi_angle` in `(0, pi/2)`.
            // Every finite value comes through, because the IR cone uses the
            // chart STEP states and reproduces the surface the record names at
            // any slope. Each arm of this match refuses what its IR carrier
            // refuses and nothing more: the cone radius stops at
            // `NonNegativeLength`, which is the entity's own `radius >= 0`, and
            // the toroidal arm below serves `TOROIDAL_SURFACE` and
            // `DEGENERATE_TOROIDAL_SURFACE` without separating their radius
            // rules. A refused carrier withholds the one surface with a
            // `DecodeWarning` note and leaves the rest of the decode standing.
            LeafSurfaceEntity::Conical => placement
                .zip(named_parameter(ctx, record, "CONICAL_SURFACE", 2)?.and_then(Value::number))
                .zip(named_parameter(ctx, record, "CONICAL_SURFACE", 3)?.and_then(Value::number))
                .and_then(|(((origin, axis, ref_direction), radius), half_angle)| {
                    let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                    let radius = NonNegativeLength::new(radius * record_scale)?;
                    let half_angle = Angle::new(half_angle * record_angle_scale)?;
                    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                        cadmpeg_ir::geometry::analytic::ConeSurface::new(
                            origin,
                            frame,
                            radius,
                            PositiveReal::ONE,
                            half_angle,
                        ),
                    )))
                }),
            LeafSurfaceEntity::Spherical => placement
                .zip(named_parameter(ctx, record, "SPHERICAL_SURFACE", 2)?.and_then(Value::number))
                .and_then(|((center, axis, ref_direction), radius)| {
                    let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                    let radius = NonZeroLength::new(radius * record_scale)?;
                    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                        cadmpeg_ir::geometry::analytic::SphereSurface::new(center, frame, radius),
                    )))
                }),
            LeafSurfaceEntity::Toroidal | LeafSurfaceEntity::DegenerateToroidal => placement
                .zip(named_parameter(ctx, record, surface_type, 2)?.and_then(Value::number))
                .zip(named_parameter(ctx, record, surface_type, 3)?.and_then(Value::number))
                .and_then(
                    |(((center, axis, ref_direction), major_radius), minor_radius)| {
                        let frame = OrthonormalFrame3::from_units(axis, ref_direction)?;
                        let major_radius = PositiveLength::new(major_radius * record_scale)?;
                        let minor_radius = NonZeroLength::new(minor_radius * record_scale)?;
                        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                            cadmpeg_ir::geometry::analytic::TorusSurface::new(
                                center,
                                frame,
                                major_radius,
                                minor_radius,
                            ),
                        )))
                    },
                ),
            LeafSurfaceEntity::BSplineWithKnots
            | LeafSurfaceEntity::UniformSurface
            | LeafSurfaceEntity::QuasiUniformSurface
            | LeafSurfaceEntity::BezierSurface => {
                nurbs_surface(id, record, &points, &mut losses, ctx)?
                    .map(SolvedSurfaceGeometry::Nurbs)
                    .map(SurfaceGeometry::Solved)
            }
        };
        if let Some(geometry) = geometry {
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: SurfaceId::from(ids::data(kind!("surface"), id)),
                    geometry,
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("{surface_type} #{id} has invalid geometry"),
                    "STEP decode text",
                )?),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, record) in exchange.entities(ctx, "B_SPLINE_SURFACE_WITH_KNOTS")? {
        if record
            .partial(ctx, "B_SPLINE_SURFACE_WITH_KNOTS")?
            .is_none()
            || record.simple_name() == Some("B_SPLINE_SURFACE_WITH_KNOTS")
        {
            continue;
        }
        if let Some(nurbs) = nurbs_surface(id, record, &points, &mut losses, ctx)? {
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: SurfaceId::from(ids::data(kind!("surface"), id)),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "B_SPLINE_SURFACE_WITH_KNOTS #{id} has invalid geometry"
                )),
                "step_geometry_losses",
            )?;
        }
    }

    // Surface constructors form the same kind of dependency graph as curves.
    // Resolve replicas in the same fixpoint as trims, bounded surfaces, and
    // offsets so a forward or nested replica cannot become an opaque carrier.
    carrier_index = CarrierIndex::from_ir(ir, ctx)?;
    let mut deferred_surface_ids = Vec::new();
    for entity in exchange.entities_any(
        ctx,
        &[
            "CURVE_BOUNDED_SURFACE",
            "OFFSET_SURFACE",
            "RECTANGULAR_TRIMMED_SURFACE",
            "SURFACE_REPLICA",
        ],
    )? {
        let (id, _) = entity?;
        ctx.push_vec(&mut deferred_surface_ids, id, "step_deferred_surface_ids")?;
    }
    let mut deferred_surface_queue = VecDeque::from(deferred_surface_ids);
    let mut surface_waiting_on = HashMap::<u64, Vec<u64>>::new();
    let (mut worklist_scale_index, mut worklist_scale_storage) = SurfaceScaleIndex::build(ir, ctx)?;
    let mut surface_start = ir.model.surfaces.len();
    let mut procedural_start = ir.model.procedural_surfaces.len();
    while let Some(id) = deferred_surface_queue.pop_front() {
            worklist_scale_storage.with_storage(|| -> Result<(), CodecError> {
                for (offset, surface) in ctx.admit_iter(
                    &ir.model.surfaces[surface_start..], "step surface scale append",
                )?.enumerate() {
                    worklist_scale_index.add_surface(surface, surface_start + offset, ctx)?;
                }
                for (offset, procedural) in ctx.admit_iter(
                    &ir.model.procedural_surfaces[procedural_start..], "step surface scale append",
                )?.enumerate() {
                    worklist_scale_index.add_procedural(procedural, procedural_start + offset, ctx)?;
                }
                Ok(())
            })?;
        surface_start = ir.model.surfaces.len();
        procedural_start = ir.model.procedural_surfaces.len();
        if carrier_index.surfaces.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let resolved = if record
            .partial(ctx, "RECTANGULAR_TRIMMED_SURFACE")?
            .is_some()
        {
            let Some(parameters) = entity_parameters(ctx, record, "RECTANGULAR_TRIMMED_SURFACE")?
            else {
                continue;
            };
            let record_scale = unit_scales.length([id]).get();
            let record_angle_scale = unit_scales.angle([id]).get();
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            let Some(mut parameter_ranges) = parameters
                .get(2)
                .and_then(Value::number)
                .zip(parameters.get(3).and_then(Value::number))
                .zip(
                    parameters
                        .get(4)
                        .and_then(Value::number)
                        .zip(parameters.get(5).and_then(Value::number)),
                )
                .map(|((u1, u2), (v1, v2))| [[u1, u2], [v1, v2]])
            else {
                continue;
            };
            let Some((u_sense, v_sense)) = parameters
                .get(6)
                .and_then(Value::logical)
                .zip(parameters.get(7).and_then(Value::logical))
            else {
                continue;
            };
            if !ctx.all_by(
                parameter_ranges.as_slice(),
                |range| Ok(range.iter().all(|parameter| parameter.is_finite())),
                "STEP surface parameter range traversal",
            )? || parameter_ranges[0][0] == parameter_ranges[0][1]
                || parameter_ranges[1][0] == parameter_ranges[1][1]
            {
                continue;
            }
            let Some(geometry) = carrier_index
                .surfaces
                .get(&support_step)
                .and_then(|index| ir.model.surfaces.get(index.0))
                .map(|surface| &surface.geometry)
            else {
                defer_geometry_dependency(
                    &mut surface_waiting_on,
                    support_step,
                    id,
                    ctx,
                    "step_deferred_surface_groups",
                    "step_deferred_surface_members",
                )?;
                continue;
            };
            let Some(solved_geometry) = geometry.solved() else {
                continue;
            };

            let Some(parameter_scales) = procedural_surface_parameter_scales(
                ir,
                &worklist_scale_index,
                &SurfaceId::from(ids::data(kind!("surface"), support_step)),
                geometry,
                record_scale,
                record_angle_scale,
                &source_curve_parameter_scales,
                ctx,
            )?
            else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                    "RECTANGULAR_TRIMMED_SURFACE #{id} has no established support parameterization"
                )),
                    "step_geometry_losses",
                )?;
                continue;
            };
            for (range, parameter_scale) in parameter_ranges.iter_mut().zip(parameter_scales) {
                range[0] *= parameter_scale;
                range[1] *= parameter_scale;
            }
            for ((range, sense), domain) in parameter_ranges
                .iter_mut()
                .zip([u_sense, v_sense])
                .zip(surface_periodic_domains(solved_geometry))
            {
                if let Some(domain) = domain {
                    if sense && range[1] < range[0] {
                        range[1] = shift_periodic_parameter(range[1], domain);
                    } else if !sense && range[0] < range[1] {
                        range[0] = shift_periodic_parameter(range[0], domain);
                    }
                }
            }
            let [[u_start, u_end], [v_start, v_end]] = parameter_ranges;
            let [Some(u_start), Some(u_end), Some(v_start), Some(v_end)] = [
                FiniteReal::new(u_start),
                FiniteReal::new(u_end),
                FiniteReal::new(v_start),
                FiniteReal::new(v_end),
            ] else {
                continue;
            };
            let parameter_ranges = [[u_start, u_end], [v_start, v_end]];
            let surface = SurfaceId::from(ids::data(kind!("surface"), id));
            let copied_geometry =
                solved_geometry.try_clone_for_decode(ctx, "step_trim_surface_carrier")?;
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                    geometry: SurfaceGeometry::Solved(copied_geometry),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            let _attached = ir.model.add_procedural_surface(ctx, &surface, match (|| {
                    let ranges = parameter_ranges.map(|range| {
                        DirectedParameterRange::from_finite_endpoints(range).map_err(|_| {
                            ProceduralGeometryError::Payload(
                                "surface subset ranges are not finite and non-zero",
                            )
                        })
                    });
                    let [u_range, v_range] = ranges;
                    Ok::<_, ProceduralGeometryError>(
                        cadmpeg_ir::geometry::surface_payloads::SubsetSurfaceConstruction::from_parts(
                            SurfaceId::from(ids::data(kind!("surface"), support_step)),
                            [u_range?, v_range?],
                            Some(u_sense),
                            Some(v_sense),
                            None,
                        ),
                    )
                })()
                .map(|admitted_payload| {
                    ProceduralSurface::new(
                        ProceduralSurfaceId::from(ids::construction(
                            kind!("rectangular_trimmed_surface"),
                            id,
                        )),
                        ProceduralSurfaceDefinition::Subset(admitted_payload),
                        None,
                    )
                }) {
                    Ok(surface) => surface,
                    Err(error) => {
                        ctx.push_vec(&mut losses, StepLossCode::DecodeWarning
                                .note(ctx.format_retained(format_args!("procedural surface #{id}: {error}"), "STEP decode text")?), "step_geometry_losses")?;
                        continue;
                    }
                })?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                SurfaceIndex(ir.model.surfaces.len() - 1),
                "step_geometry_surface_index",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            true
        } else if record.partial(ctx, "CURVE_BOUNDED_SURFACE")?.is_some() {
            let surface = SurfaceId::from(ids::data(kind!("surface"), id));
            let Some(parameters) = entity_parameters(ctx, record, "CURVE_BOUNDED_SURFACE")? else {
                continue;
            };
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            let Some(support_index) = carrier_index.surfaces.get(&support_step).copied() else {
                defer_geometry_dependency(
                    &mut surface_waiting_on,
                    support_step,
                    id,
                    ctx,
                    "step_deferred_surface_groups",
                    "step_deferred_surface_members",
                )?;
                continue;
            };
            let support = SurfaceId::from(ids::data(kind!("surface"), support_step));
            let boundary_steps = if let Some(values) = parameters.get(2).and_then(Value::list) {
                ctx.all_by(
                    values,
                    |value| Ok(value.reference().is_some()),
                    "STEP bounded surface boundary reference validation",
                )?
                .then_some(values)
            } else {
                None
            };
            let mut boundaries = boundary_steps.map(|_| Vec::new());
            if let (Some(values), Some(boundaries)) = (boundary_steps, boundaries.as_mut()) {
                for boundary in ctx
                    .admit_iter(&values[..], "STEP decode traversal")?
                    .filter_map(Value::reference)
                {
                    ctx.push_vec(
                        boundaries,
                        CurveId::from(ids::data(kind!("curve"), boundary)),
                        "step_curve_bounded_boundaries",
                    )?;
                }
            }
            let mut boundary_pcurve_set = BTreeSet::new();
            for boundary in ctx
                .admit_iter(
                    boundary_steps.unwrap_or_default(),
                    "STEP surface boundary pcurve traversal",
                )?
                .filter_map(Value::reference)
            {
                boundary_pcurve_steps(boundary, support_step, exchange, ctx, &mut |pcurve| {
                    ctx.insert_btree_set(
                        &mut boundary_pcurve_set,
                        PcurveId::from(ids::data(kind!("pcurve"), pcurve)),
                        "step_curve_bounded_pcurve_set",
                    )?;
                    Ok(())
                })?;
            }
            let mut boundary_pcurves = Vec::new();
            for pcurve in boundary_pcurve_set {
                ctx.push_vec(&mut boundary_pcurves, pcurve, "step_curve_bounded_pcurves")?;
            }
            let implicit_outer = parameters.get(3).and_then(Value::logical);
            let Some((boundaries, implicit_outer, geometry)) = ir
                .model
                .surfaces
                .get(support_index.0)
                .and_then(|surface| surface.geometry.solved())
                .zip(boundaries)
                .zip(implicit_outer)
                .map(|((geometry, boundaries), implicit_outer)| {
                    (boundaries, implicit_outer, geometry)
                })
            else {
                continue;
            };
            if boundaries.is_empty()
                || ctx
                    .find_map(
                        boundaries.as_slice(),
                        |curve| -> Result<Option<()>, CodecError> {
                            Ok((!step_instance_id(ctx, curve.as_str())?
                                .is_some_and(|id| carrier_index.curves.contains_key(&id)))
                            .then_some(()))
                        },
                        "STEP bounded surface carrier validation",
                    )?
                    .is_some()
            {
                continue;
            }
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            let copied_geometry =
                geometry.try_clone_for_decode(ctx, "step_curve_bounded_surface_carrier")?;
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                    geometry: SurfaceGeometry::Solved(copied_geometry),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            let _attached = ir.model.add_procedural_surface(
                ctx,
                &surface,
                ProceduralSurface::new(
                    ProceduralSurfaceId::from(ids::construction(
                        kind!("curve_bounded_surface"),
                        id,
                    )),
                    ProceduralSurfaceDefinition::CurveBounded {
                        support,
                        boundaries,
                        boundary_pcurves,
                        implicit_outer,
                    },
                    None,
                ),
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            true
        } else if record.partial(ctx, "OFFSET_SURFACE")?.is_some() {
            let surface = SurfaceId::from(ids::data(kind!("surface"), id));
            let Some(parameters) = entity_parameters(ctx, record, "OFFSET_SURFACE")? else {
                continue;
            };
            let record_scale = unit_scales.length([id]).get();
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            if !carrier_index.surfaces.contains_key(&support_step) {
                defer_geometry_dependency(
                    &mut surface_waiting_on,
                    support_step,
                    id,
                    ctx,
                    "step_deferred_surface_groups",
                    "step_deferred_surface_members",
                )?;
                continue;
            }
            let support = SurfaceId::from(ids::data(kind!("surface"), support_step));
            let distance = parameters.get(2).and_then(Value::number);
            let self_intersect = parameters
                .get(3)
                .and_then(|value| logical_value(value).ok());
            let Some((distance, self_intersect)) = distance.zip(self_intersect) else {
                continue;
            };
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: None,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            let _attached = ir.model.add_procedural_surface(ctx, &surface, match cadmpeg_ir::geometry::surface_payloads::ParallelOffsetSurfaceConstruction::try_new(support, distance * record_scale, self_intersect).map(|admitted_payload| ProceduralSurface::new(
                    ProceduralSurfaceId::from(ids::construction(kind!("offset_surface"), id)),
                    ProceduralSurfaceDefinition::ParallelOffset(admitted_payload),
                    None,
                )) {
                    Ok(surface) => surface,
                    Err(error) => {
                        ctx.push_vec(&mut losses, StepLossCode::DecodeWarning.note(ctx.format_retained(format_args!("procedural surface #{id}: {error}"), "STEP decode text")?), "step_geometry_losses")?;
                        continue;
                    }
                })?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            true
        } else if record.partial(ctx, "SURFACE_REPLICA")?.is_some() {
            let Some(parent_step) =
                named_parameter(ctx, record, "SURFACE_REPLICA", 1)?.and_then(Value::reference)
            else {
                continue;
            };
            let Some(operator_step) =
                named_parameter(ctx, record, "SURFACE_REPLICA", 2)?.and_then(Value::reference)
            else {
                continue;
            };
            let Some(parent_index) = carrier_index.surfaces.get(&parent_step).copied() else {
                defer_geometry_dependency(
                    &mut surface_waiting_on,
                    parent_step,
                    id,
                    ctx,
                    "step_deferred_surface_groups",
                    "step_deferred_surface_members",
                )?;
                continue;
            };
            let Some(transform) = transformation_operators.get(&operator_step).copied() else {
                continue;
            };
            let Some(basis) = ir
                .model
                .surfaces
                .get(parent_index.0)
                .and_then(|surface| surface.geometry.solved())
            else {
                continue;
            };
            let basis = basis.try_clone_for_decode(ctx, "step_surface_replica_basis")?;
            let Ok(placed) =
                cadmpeg_ir::geometry::PlacedSurface::try_new(Box::new(basis), transform)
            else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "SURFACE_REPLICA #{id} nests past the admitted inline basis depth"
                    )),
                    "step_geometry_losses",
                )?;
                continue;
            };
            let geometry = SolvedSurfaceGeometry::Transformed(placed);
            let surface = SurfaceId::from(ids::data(kind!("surface"), id));
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface.try_clone_for_decode(ctx, "step_surface_identity_copy")?,
                    geometry: SurfaceGeometry::Solved(geometry),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            let _attached = ir.model.add_procedural_surface(
                ctx,
                &surface,
                ProceduralSurface::new(
                    ProceduralSurfaceId::from(ids::construction(kind!("surface_replica"), id)),
                    ProceduralSurfaceDefinition::Replica {
                        source: SurfaceId::from(ids::data(kind!("surface"), parent_step)),
                        transform,
                    },
                    None,
                ),
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
            ctx.insert_btree_set(&mut typed, operator_step, "step_geometry_typed_ids")?;
            true
        } else {
            false
        };
        if resolved {
            wake_deferred_dependents(
                id,
                &mut surface_waiting_on,
                &mut deferred_surface_queue,
                ctx,
                "step_deferred_surface_queue",
            )?;
        }
    }
    drop(worklist_scale_index);
    drop(worklist_scale_storage);
    for (id, _) in exchange.entities(ctx, "SURFACE_REPLICA")? {
        if !carrier_index.surfaces.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "SURFACE_REPLICA #{id} has invalid or unresolved parent/operator"
                )),
                "step_geometry_losses",
            )?;
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: SurfaceId::from(ids::data(kind!("surface"), id)),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&id)
                            .map(|record| opaque_record_id(id, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
        }
    }
    for (id, _) in exchange.entities(ctx, "RECTANGULAR_TRIMMED_SURFACE")? {
        if !carrier_index.surfaces.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                "RECTANGULAR_TRIMMED_SURFACE #{id} has invalid or unresolved basis/trim selectors"
            )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, _) in exchange.entities(ctx, "CURVE_BOUNDED_SURFACE")? {
        if !carrier_index.surfaces.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "CURVE_BOUNDED_SURFACE #{id} has invalid or unresolved support/boundaries"
                )),
                "step_geometry_losses",
            )?;
        }
    }
    for (id, _) in exchange.entities(ctx, "OFFSET_SURFACE")? {
        if !carrier_index.surfaces.contains_key(&id) {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "OFFSET_SURFACE #{id} has invalid or unresolved support parameters"
                )),
                "step_geometry_losses",
            )?;
        }
    }

    for record in exchange
        .entities(ctx, "EDGE_CURVE")?
        .map(|(_, record)| record)
    {
        let Some(curve_step) = edge_curve_geometry_reference(ctx, record)?
            .map(|curve| curve_carrier_record(ctx, curve, exchange))
            .transpose()?
            .flatten()
        else {
            continue;
        };
        if !carrier_index.curves.contains_key(&curve_step) {
            let curve_index = CurveIndex(ir.model.curves.len());
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: CurveId::from(ids::data(kind!("curve"), curve_step)),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&curve_step)
                            .map(|record| opaque_record_id(curve_step, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_curves",
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "retained undecoded topology curve #{curve_step} as an unknown carrier"
                )),
                "step_geometry_losses",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.curves,
                curve_step,
                curve_index,
                "step_geometry_curve_index",
            )?;
        }
    }
    for entity in exchange.entities_any(
        ctx,
        &[
            "CURVE_BOUNDED_SURFACE",
            "OFFSET_SURFACE",
            "RECTANGULAR_TRIMMED_SURFACE",
        ],
    )? {
        let (id, _) = entity?;
        let surface = SurfaceId::from(ids::data(kind!("surface"), id));
        if !carrier_index.surfaces.contains_key(&id) {
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: surface,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&id)
                            .map(|record| opaque_record_id(id, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "retained unresolved deferred surface #{id} as an unknown carrier"
                )),
                "step_geometry_losses",
            )?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                id,
                surface_index,
                "step_geometry_surface_index",
            )?;
        }
    }
    for (&face_id, face) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
        if !ctx.any_by(
            &face.partials[..],
            |partial| {
                Ok(matches!(
                    partial.name.as_str(),
                    "ADVANCED_FACE" | "FACE_SURFACE"
                ))
            },
            "STEP decode traversal",
        )? {
            continue;
        }
        let Some(surface_step) = face_surface_reference(ctx, face)? else {
            continue;
        };
        if !carrier_index.surfaces.contains_key(&surface_step) {
            let surface_index = SurfaceIndex(ir.model.surfaces.len());
            ctx.push_vec(
                &mut ir.model.surfaces,
                Surface {
                    id: SurfaceId::from(ids::data(kind!("surface"), surface_step)),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: exchange
                            .records()
                            .get(&surface_step)
                            .map(|record| opaque_record_id(surface_step, record, ctx))
                            .transpose()?,
                    }),
                    source_object: None,
                },
                "step_geometry_ir_surfaces",
            )?;
            ctx.push_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "retained undecoded face surface #{surface_step} from face #{face_id} as an unknown carrier"
            )), "step_geometry_losses")?;
            ctx.insert_hash_map(
                &mut carrier_index.surfaces,
                surface_step,
                surface_index,
                "step_geometry_surface_index",
            )?;
        }
    }
    let mut surface_parameter_scales = BTreeMap::new();
    let (scale_index, _scale_index_workspace) = SurfaceScaleIndex::build(ir, ctx)?;
    for surface in ctx.admit_iter(&ir.model.surfaces[..], "STEP decode traversal")? {
        let Some(id) = step_instance_id(ctx, surface.id.as_str())? else {
            continue;
        };
        if let Some(scales) = procedural_surface_parameter_scales(
            ir,
            &scale_index,
            &surface.id,
            &surface.geometry,
            unit_scales.length([id]).get(),
            unit_scales.angle([id]).get(),
            &source_curve_parameter_scales,
            ctx,
        )? {
            ctx.insert_btree_map(
                &mut surface_parameter_scales,
                id,
                scales,
                "step_surface_parameter_scales",
            )?;
        }
    }
    for (id, record) in exchange.entities(ctx, "PCURVE")? {
        if record.partial(ctx, "PCURVE")?.is_none() {
            continue;
        }
        let surface_step = named_parameter(ctx, record, "PCURVE", 1)?.and_then(Value::reference);
        let representation = named_parameter(ctx, record, "PCURVE", 2)?
            .and_then(Value::reference)
            .and_then(|representation| exchange.records().get(&representation));
        let curve_steps = representation
            .map(|record| representation_items(ctx, record))
            .transpose()?
            .flatten()
            .into_iter()
            .flatten();
        let mut decoded = None;
        let mut decoded_count = 0;
        for curve in curve_steps {
            if let Some(geometry) = pcurve_geometries.get(&curve) {
                decoded = Some((curve, geometry));
                decoded_count += 1;
            }
        }
        let decoded = if decoded_count == 1 { decoded } else { None };
        let Some((curve_step, (geometry, geometry_records))) = surface_step
            .filter(|surface| carrier_index.surfaces.contains_key(surface))
            .and(decoded)
        else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning
                    .note(format!("PCURVE #{id} has no decoded surface or 2D curve")),
                "step_geometry_losses",
            )?;
            continue;
        };
        let Some(scales) = surface_step.and_then(|surface| surface_parameter_scales.get(&surface))
        else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "PCURVE #{id} has no established owning surface parameterization"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let geometry = geometry.try_clone_for_decode(ctx, "step_pcurve_carrier_copy")?;
        let Ok(geometry) = geometry.scaled_coordinates_owned(ctx, *scales)? else {
            ctx.push_vec(&mut losses, StepLossCode::DecodeWarning.note(format!(
                "PCURVE #{id} has a 2D carrier that cannot be scaled into the owning surface parameter units"
            )), "step_geometry_losses")?;
            continue;
        };
        ctx.push_vec(
            &mut ir.model.pcurves,
            Pcurve {
                id: PcurveId::from(ids::data(kind!("pcurve"), id)),
                geometry,
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
            },
            "step_geometry_ir_pcurves",
        )?;
        ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        if let Some(representation) =
            named_parameter(ctx, record, "PCURVE", 2)?.and_then(Value::reference)
        {
            ctx.insert_btree_set(&mut typed, representation, "step_geometry_typed_ids")?;
        }
        ctx.insert_btree_set(&mut typed, curve_step, "step_geometry_typed_ids")?;
        for &record in geometry_records {
            ctx.insert_btree_set(&mut typed, record, "step_geometry_typed_ids")?;
        }
    }

    // Curve-bounded surfaces resolve before the PCURVE pass because their 3D
    // boundaries do not depend on parameter-space geometry. Remove candidate
    // references whose pcurve carrier did not decode.
    let mut decoded_pcurve_steps = BTreeSet::new();
    for pcurve in ctx.admit_iter(&ir.model.pcurves[..], "STEP decode traversal")? {
        if let Some(id) = step_instance_id(ctx, pcurve.id.as_str())? {
            ctx.insert_btree_set(&mut decoded_pcurve_steps, id, "step_decoded_pcurve_steps")?;
        }
    }
    for surface in &mut ir.model.procedural_surfaces {
        surface.edit_definition(|definition| -> Result<(), CodecError> {
            let ProceduralSurfaceDefinition::CurveBounded {
                boundary_pcurves, ..
            } = definition
            else {
                return Ok(());
            };
            ctx.retain_vec(
                boundary_pcurves,
                |pcurve| {
                    Ok(step_instance_id(ctx, pcurve.as_str())?
                        .is_some_and(|id| decoded_pcurve_steps.contains(&id)))
                },
                "STEP decoded boundary pcurve retention",
            )
        })?;
    }

    for (id, record) in exchange.entities(ctx, "DEGENERATE_TOROIDAL_SURFACE")? {
        let Some(select_outer) = named_parameter(ctx, record, "DEGENERATE_TOROIDAL_SURFACE", 4)?
            .and_then(|value| logical_value(value).ok().flatten())
        else {
            if carrier_index.surfaces.contains_key(&id) {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "DEGENERATE_TOROIDAL_SURFACE #{id} has invalid sheet selection"
                    )),
                    "step_geometry_losses",
                )?;
            }
            continue;
        };
        let surface = SurfaceId::from(ids::data(kind!("surface"), id));
        if !carrier_index.surfaces.contains_key(&id) {
            continue;
        }
        let _attached = ir.model.add_procedural_surface(
            ctx,
            &surface,
            ProceduralSurface::new(
                ProceduralSurfaceId::from(ids::construction(kind!("degenerate_torus"), id)),
                ProceduralSurfaceDefinition::DegenerateTorus { select_outer },
                None,
            ),
        )?;
    }

    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
        if ctx.any_by(
            &record.partials[..],
            |partial| {
                Ok(matches!(
                    partial.name.as_str(),
                    "LENGTH_UNIT"
                        | "NAMED_UNIT"
                        | "SI_UNIT"
                        | "CONVERSION_BASED_UNIT"
                        | "MEASURE_WITH_UNIT"
                        | "LENGTH_MEASURE_WITH_UNIT"
                        | "PLANE_ANGLE_MEASURE_WITH_UNIT"
                        | "UNCERTAINTY_MEASURE_WITH_UNIT"
                        | "GEOMETRIC_REPRESENTATION_CONTEXT"
                        | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                        | "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT"
                        | "REPRESENTATION_CONTEXT"
                ))
            },
            "STEP decode traversal",
        )? || entity_type(ctx, record, &["SHAPE_REPRESENTATION"])?.is_some()
        {
            ctx.insert_btree_set(&mut typed, id, "step_geometry_typed_ids")?;
        }
    }
    Ok(StageOutcome {
        value: GeometryData {
            placements,
            transformation_operators,
            units: unit_scales,
        },
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

fn decode_tessellated_curve_sets(
    exchange: &Exchange,
    unit_scales: &UnitScales,
    ir: &mut CadIr,
    typed: &mut BTreeSet<u64>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (&id, record) in ctx.admit_iter(
        exchange.records(),
        "STEP decode tessellated curve sets traversal",
    )? {
        if record.partial(ctx, "TESSELLATED_CURVE_SET")?.is_none() {
            continue;
        }
        let Some(coordinates_id) =
            tessellated_curve_parameter(ctx, record, 0)?.and_then(ValueExt::reference)
        else {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "TESSELLATED_CURVE_SET #{id} has no COORDINATES_LIST reference"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let Some(coordinates_record) = exchange.records().get(&coordinates_id) else {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                "TESSELLATED_CURVE_SET #{id} references missing COORDINATES_LIST #{coordinates_id}"
            )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let scale = unit_scales.length([coordinates_id]).get();
        let Some(vertices) = coordinate_rows(coordinates_record, scale, ctx)? else {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "TESSELLATED_CURVE_SET #{id} has invalid COORDINATES_LIST #{coordinates_id}"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let Some(strips) = tessellated_line_strips(
            tessellated_curve_parameter(ctx, record, 1)?,
            vertices.len(),
            ctx,
        )?
        else {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(format!(
                    "TESSELLATED_CURVE_SET #{id} has invalid line strips"
                )),
                "step_geometry_losses",
            )?;
            continue;
        };
        let source_name = representation_item_name(ctx, record)?
            .map(|value| {
                super::decode_text_charged(
                    exchange,
                    value,
                    losses,
                    id,
                    "tessellated curve name",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .filter(|name| !name.is_empty());
        for (strip_index, indices) in strips.into_iter().enumerate() {
            let curve_key = if strip_index == 0 {
                IdentityKey::from(id)
            } else {
                IdentityKey::from(id)
                    .dash(key_word!("strip"))
                    .dash(strip_index)
            };
            let mut points = Vec::new();
            for index in indices {
                ctx.push_vec(&mut points, vertices[index], "step_curve_strip_points")?;
            }
            let Ok(points) = points.try_into() else {
                continue;
            };
            let Some(polyline) = PolylineCurve::from_checked_samples(
                PolylineSamples::Unparameterized { points },
                0.0,
                ctx,
            )?
            .ok() else {
                continue;
            };
            let source_name = source_name
                .as_ref()
                .map(|name| {
                    ctx.format_retained(format_args!("{name}"), "step_curve_strip_source_name")
                })
                .transpose()?;
            ctx.push_vec(
                &mut ir.model.curves,
                Curve {
                    id: CurveId::from(ids::data(kind!("curve"), curve_key)),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline)),
                    source_object: Some(super::step_source_association(ctx, id, source_name)?),
                },
                "step_geometry_ir_curves",
            )?;
        }
        for source_id in [id, coordinates_id] {
            ctx.insert_btree_set(typed, source_id, "step_geometry_typed_ids")?;
        }
    }
    Ok(())
}

fn tessellated_curve_parameter<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    index: usize,
) -> Result<Option<&'a Value>, CodecError> {
    let offset = usize::from(record.partials.len() == 1);
    Ok(record
        .partial(ctx, "TESSELLATED_CURVE_SET")?
        .and_then(|partial| partial.parameters.get(index + offset)))
}

fn tessellated_line_strips(
    value: Option<&Value>,
    point_count: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Vec<usize>>>, CodecError> {
    let Some(strips) = value.and_then(Value::list) else {
        return Ok(None);
    };
    if strips.is_empty() {
        return Ok(None);
    }
    let mut decoded = Vec::new();
    for strip in strips {
        let Some(values) = strip.list() else {
            return Ok(None);
        };
        if values.len() < 2 {
            return Ok(None);
        }
        let mut indices = Vec::new();
        for value in values {
            let Some(index) = value
                .integer()
                .and_then(|value| usize::try_from(value).ok())
                .and_then(|value| value.checked_sub(1))
            else {
                return Ok(None);
            };
            if index >= point_count {
                return Ok(None);
            }
            ctx.push_vec(&mut indices, index, "step_curve_strip_indices")?;
        }
        ctx.push_vec(&mut decoded, indices, "step_curve_strips")?;
    }
    Ok(Some(decoded))
}

fn face_surface_reference(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    if record.partials.len() == 1
        && matches!(record.simple_name(), Some("ADVANCED_FACE" | "FACE_SURFACE"))
    {
        return Ok(record.parameter(2).and_then(Value::reference));
    }
    for partial in ctx
        .admit_iter(&record.partials[..], "STEP face surface partial traversal")?
        .rev()
        .filter(|partial| matches!(partial.name.as_str(), "ADVANCED_FACE" | "FACE_SURFACE"))
    {
        if let Some(reference) = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(Value::reference(value)),
            "STEP face surface parameter traversal",
        )? {
            return Ok(Some(reference));
        }
    }
    Ok(None)
}

pub(super) fn associate_free_geometric_set_members(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for set in ctx
        .admit_iter(
            exchange.records(),
            "STEP associate free geometric set members map traversal",
        )?
        .map(|(_, value)| value)
    {
        let Some(set_type) = entity_type(ctx, set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])?
        else {
            continue;
        };
        let Some(members) = named_parameter(ctx, set, set_type, 1)?.and_then(Value::list) else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            let name = exchange
                .records()
                .get(&member)
                .map(|record| representation_item_name(ctx, record))
                .transpose()?
                .flatten()
                .map(|value| {
                    super::decode_text_charged(
                        exchange,
                        value,
                        losses,
                        member,
                        "geometric-set member name",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
                .filter(|name| !name.is_empty());
            if let Some(index) = index.curves.get(&member) {
                if owned.curves.contains(index) {
                    continue;
                }
                attach_geometry_source(
                    &mut ir.model.curves[index.0].source_object,
                    member,
                    name.as_deref(),
                    ctx,
                    "step_geometric_set_association_name_copy",
                )?;
            }
            if let Some(index) = index.points.get(&member).map(|point| &point.index) {
                if owned.points.contains(index) {
                    continue;
                }
                attach_geometry_source(
                    &mut ir.model.points[index.0].source_object,
                    member,
                    name.as_deref(),
                    ctx,
                    "step_geometric_set_association_name_copy",
                )?;
            }
            if let Some(index) = index.surfaces.get(&member) {
                if owned.surfaces.contains(index) {
                    continue;
                }
                attach_geometry_source(
                    &mut ir.model.surfaces[index.0].source_object,
                    member,
                    name.as_deref(),
                    ctx,
                    "step_geometric_set_association_name_copy",
                )?;
            }
        }
    }
    Ok(())
}

/// Associate carriers that are listed directly by a STEP representation but
/// are not owned by committed topology. A representation item is an explicit
/// source owner for free geometry; without this association the generic IR
/// reachability check would misclassify a valid standalone carrier as an
/// orphan.
pub(super) fn associate_free_representation_members(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for representation in ctx
        .admit_iter(
            exchange.records(),
            "STEP associate free representation members map traversal",
        )?
        .map(|(_, value)| value)
    {
        if !ctx.any_by(
            &representation.partials[..],
            |partial| Ok(super::representation::is_representation_name(&partial.name)),
            "STEP representation association partial traversal",
        )? {
            continue;
        }
        let Some(items) = representation_items(ctx, representation)? else {
            continue;
        };
        for member in items {
            let source_name = exchange
                .records()
                .get(&member)
                .map(|record| representation_item_name(ctx, record))
                .transpose()?
                .flatten()
                .map(|value| {
                    super::decode_text_charged(
                        exchange,
                        value,
                        losses,
                        member,
                        "representation member name",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
                .filter(|name| !name.is_empty());
            if let Some(index) = index.curves.get(&member) {
                if !owned.curves.contains(index) {
                    attach_geometry_source(
                        &mut ir.model.curves[index.0].source_object,
                        member,
                        source_name.as_deref(),
                        ctx,
                        "step_representation_association_name_copy",
                    )?;
                }
            }
            if let Some(index) = index.points.get(&member).map(|point| &point.index) {
                if !owned.points.contains(index) {
                    attach_geometry_source(
                        &mut ir.model.points[index.0].source_object,
                        member,
                        source_name.as_deref(),
                        ctx,
                        "step_representation_association_name_copy",
                    )?;
                }
            }
            if let Some(index) = index.surfaces.get(&member) {
                if !owned.surfaces.contains(index) {
                    attach_geometry_source(
                        &mut ir.model.surfaces[index.0].source_object,
                        member,
                        source_name.as_deref(),
                        ctx,
                        "step_representation_association_name_copy",
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Associate geometry that is owned by presentation records rather than by a
/// shape representation. A style is a source owner even when its style
/// assignment has no surface colour, and an annotation plane owns each
/// referenced surface used to construct that plane.
pub(super) fn associate_free_presentation_carriers(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (&style_id, record) in
        ctx.admit_iter(exchange.records(), "STEP presentation source traversal")?
    {
        if let Some(target) = super::presentation::styled_item_target(ctx, record)? {
            associate_presentation_carrier(
                exchange,
                ir,
                index,
                owned,
                (target, style_id),
                losses,
                ctx,
            )?;
        }
    }
    for (plane_id, plane) in exchange.entities(ctx, "ANNOTATION_PLANE")? {
        for partial in ctx.admit_iter(
            &(plane.partials)[..],
            "STEP associate free presentation carriers traversal",
        )? {
            for parameter in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP record parameter traversal",
            )? {
                for target in super::reference::references(parameter, ctx) {
                    let target = target?;
                    if index.surfaces.contains_key(&target) {
                        associate_presentation_carrier(
                            exchange,
                            ir,
                            index,
                            owned,
                            (target, plane_id),
                            losses,
                            ctx,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn associate_presentation_carrier(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    (target, source_id): (u64, u64),
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let name = exchange
        .records()
        .get(&target)
        .map(|record| representation_item_name(ctx, record))
        .transpose()?
        .flatten()
        .map(|value| {
            super::decode_text_charged(
                exchange,
                value,
                losses,
                target,
                "presentation carrier name",
                StepLossCode::MetadataStringInvalid,
                ctx,
            )
        })
        .transpose()?
        .flatten()
        .filter(|name| !name.is_empty());
    if let Some(index) = index.curves.get(&target) {
        if !owned.curves.contains(index) {
            attach_geometry_source(
                &mut ir.model.curves[index.0].source_object,
                source_id,
                name.as_deref(),
                ctx,
                "step_presentation_association_name_copy",
            )?;
        }
    }
    if let Some(index) = index.points.get(&target).map(|point| &point.index) {
        if !owned.points.contains(index) {
            attach_geometry_source(
                &mut ir.model.points[index.0].source_object,
                source_id,
                name.as_deref(),
                ctx,
                "step_presentation_association_name_copy",
            )?;
        }
    }
    if let Some(index) = index.surfaces.get(&target) {
        if !owned.surfaces.contains(index) {
            attach_geometry_source(
                &mut ir.model.surfaces[index.0].source_object,
                source_id,
                name.as_deref(),
                ctx,
                "step_presentation_association_name_copy",
            )?;
        }
    }
    Ok(())
}

fn representation_items<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<impl Iterator<Item = u64> + 'a>, CodecError> {
    for partial in ctx.admit_iter(
        &record.partials[..],
        "STEP geometry representation partial traversal",
    )? {
        if let Some(items) = ctx.find_map(
            partial.parameters.as_slice(),
            |value| Ok(Value::list(value)),
            "STEP geometry representation parameter traversal",
        )? {
            return Ok(Some(
                ctx.admit_iter(items, "STEP geometry representation item traversal")?
                    .filter_map(Value::reference),
            ));
        }
    }
    Ok(None)
}

fn representation_item_name<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a Value>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(record.parameter(0));
    }
    Ok(record
        .partial(ctx, "REPRESENTATION_ITEM")?
        .and_then(|partial| partial.parameters.first()))
}

fn entity_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    name: &str,
) -> Result<Option<&'a [Value]>, CodecError> {
    Ok(ctx
        .find_map(
            &record.partials[..],
            |partial| -> Result<Option<_>, CodecError> {
                Ok((ctx.equal(
                    partial.name.as_str(),
                    name,
                    "STEP entity parameters equality",
                )?)
                .then_some(partial))
            },
            "STEP geometry entity parameter partial traversal",
        )?
        .map(|partial| partial.parameters.as_slice()))
}

fn transformation_parameter<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    name: &str,
    index: usize,
) -> Result<Option<&'a Value>, CodecError> {
    let Some(partial) = ctx.find_map(
        &record.partials[..],
        |partial| -> Result<Option<_>, CodecError> {
            Ok((ctx.equal(
                partial.name.as_str(),
                name,
                "STEP transformation parameter equality",
            )?)
            .then_some(partial))
        },
        "STEP transformation attribute partial traversal",
    )?
    else {
        return Ok(None);
    };
    let parameters = &partial.parameters;
    let (attribute_count, offset) = match (name, parameters.len()) {
        ("CARTESIAN_TRANSFORMATION_OPERATOR_3D", 6) => (5, 1),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_3D", 8) => (5, 3),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_2D", 5) => (4, 1),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_2D", 7) => (4, 3),
        _ => return Ok(None),
    };
    Ok((index < attribute_count).then(|| &parameters[offset + index]))
}

/// One leaf curve carrier entity dispatched by the STEP reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeafCurveEntity {
    Line,
    Circle,
    Ellipse,
    Parabola,
    Hyperbola,
    Polyline,
    BSplineWithKnots,
    UniformCurve,
    QuasiUniformCurve,
    BezierCurve,
}

impl LeafCurveEntity {
    const NAMES: &'static [&'static str] = &[
        "LINE",
        "CIRCLE",
        "ELLIPSE",
        "PARABOLA",
        "HYPERBOLA",
        "POLYLINE",
        "B_SPLINE_CURVE_WITH_KNOTS",
        "UNIFORM_CURVE",
        "QUASI_UNIFORM_CURVE",
        "BEZIER_CURVE",
    ];

    fn of(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<Option<Self>, CodecError> {
        Ok(
            entity_type(ctx, record, Self::NAMES)?.and_then(|name| match name {
                "LINE" => Some(Self::Line),
                "CIRCLE" => Some(Self::Circle),
                "ELLIPSE" => Some(Self::Ellipse),
                "PARABOLA" => Some(Self::Parabola),
                "HYPERBOLA" => Some(Self::Hyperbola),
                "POLYLINE" => Some(Self::Polyline),
                "B_SPLINE_CURVE_WITH_KNOTS" => Some(Self::BSplineWithKnots),
                "UNIFORM_CURVE" => Some(Self::UniformCurve),
                "QUASI_UNIFORM_CURVE" => Some(Self::QuasiUniformCurve),
                "BEZIER_CURVE" => Some(Self::BezierCurve),
                _ => None,
            }),
        )
    }

    fn name(self) -> &'static str {
        match self {
            Self::Line => "LINE",
            Self::Circle => "CIRCLE",
            Self::Ellipse => "ELLIPSE",
            Self::Parabola => "PARABOLA",
            Self::Hyperbola => "HYPERBOLA",
            Self::Polyline => "POLYLINE",
            Self::BSplineWithKnots => "B_SPLINE_CURVE_WITH_KNOTS",
            Self::UniformCurve => "UNIFORM_CURVE",
            Self::QuasiUniformCurve => "QUASI_UNIFORM_CURVE",
            Self::BezierCurve => "BEZIER_CURVE",
        }
    }
}

/// One leaf surface carrier entity dispatched by the STEP reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeafSurfaceEntity {
    Plane,
    Cylindrical,
    Conical,
    Spherical,
    DegenerateToroidal,
    Toroidal,
    BSplineWithKnots,
    UniformSurface,
    QuasiUniformSurface,
    BezierSurface,
}

impl LeafSurfaceEntity {
    const NAMES: &'static [&'static str] = &[
        "PLANE",
        "CYLINDRICAL_SURFACE",
        "CONICAL_SURFACE",
        "SPHERICAL_SURFACE",
        "DEGENERATE_TOROIDAL_SURFACE",
        "TOROIDAL_SURFACE",
        "B_SPLINE_SURFACE_WITH_KNOTS",
        "UNIFORM_SURFACE",
        "QUASI_UNIFORM_SURFACE",
        "BEZIER_SURFACE",
    ];

    fn of(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<Option<Self>, CodecError> {
        Ok(
            entity_type(ctx, record, Self::NAMES)?.and_then(|name| match name {
                "PLANE" => Some(Self::Plane),
                "CYLINDRICAL_SURFACE" => Some(Self::Cylindrical),
                "CONICAL_SURFACE" => Some(Self::Conical),
                "SPHERICAL_SURFACE" => Some(Self::Spherical),
                "DEGENERATE_TOROIDAL_SURFACE" => Some(Self::DegenerateToroidal),
                "TOROIDAL_SURFACE" => Some(Self::Toroidal),
                "B_SPLINE_SURFACE_WITH_KNOTS" => Some(Self::BSplineWithKnots),
                "UNIFORM_SURFACE" => Some(Self::UniformSurface),
                "QUASI_UNIFORM_SURFACE" => Some(Self::QuasiUniformSurface),
                "BEZIER_SURFACE" => Some(Self::BezierSurface),
                _ => None,
            }),
        )
    }

    fn name(self) -> &'static str {
        match self {
            Self::Plane => "PLANE",
            Self::Cylindrical => "CYLINDRICAL_SURFACE",
            Self::Conical => "CONICAL_SURFACE",
            Self::Spherical => "SPHERICAL_SURFACE",
            Self::DegenerateToroidal => "DEGENERATE_TOROIDAL_SURFACE",
            Self::Toroidal => "TOROIDAL_SURFACE",
            Self::BSplineWithKnots => "B_SPLINE_SURFACE_WITH_KNOTS",
            Self::UniformSurface => "UNIFORM_SURFACE",
            Self::QuasiUniformSurface => "QUASI_UNIFORM_SURFACE",
            Self::BezierSurface => "BEZIER_SURFACE",
        }
    }
}

fn entity_type(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    names: &[&'static str],
) -> Result<Option<&'static str>, CodecError> {
    for &name in ctx.admit_iter(names, "STEP geometry entity name traversal")? {
        if record.partial(ctx, name)?.is_some() {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

fn is_apll_leader_line(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<bool, CodecError> {
    ctx.any_by(
        &record.partials[..],
        |partial| {
            Ok(matches!(
                partial.name.as_str(),
                "ANNOTATION_PLACEHOLDER_LEADER_LINE"
                    | "ANNOTATION_TO_ANNOTATION_LEADER_LINE"
                    | "ANNOTATION_TO_MODEL_LEADER_LINE"
                    | "AUXILIARY_LEADER_LINE"
            ))
        },
        "STEP APLL leader partial traversal",
    )
}

fn first_named_list<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    names: &[&str],
) -> Result<Option<impl Iterator<Item = u64> + 'a>, CodecError> {
    let mut selected = None;
    for partial in ctx.admit_iter(&record.partials[..], "STEP named list partial traversal")? {
        if ctx
            .find_map(
                names,
                |name| -> Result<Option<_>, CodecError> {
                    Ok((ctx.equal(
                        partial.name.as_str(),
                        name,
                        "STEP first named list equality",
                    )?)
                    .then_some(()))
                },
                "STEP named list name traversal",
            )?
            .is_some()
        {
            selected = Some(partial);
            break;
        }
    }
    let Some(partial) = selected else {
        return Ok(None);
    };
    for items in ctx
        .admit_iter(
            partial.parameters.as_slice(),
            "STEP named list parameter traversal",
        )?
        .filter_map(Value::list)
    {
        if ctx.all_by(
            items,
            |item| Ok(item.reference().is_some()),
            "STEP named list reference validation traversal",
        )? {
            return Ok(Some(
                ctx.admit_iter(items, "STEP named list reference traversal")?
                    .filter_map(Value::reference),
            ));
        }
    }
    Ok(None)
}

fn vertex_point_reference(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    let Some(partial) = record.partial(ctx, "VERTEX_POINT")? else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(Value::reference(value)),
        "STEP vertex point reference traversal",
    )
}

fn edge_curve_geometry_reference(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(record.parameter(3).and_then(Value::reference));
    }
    let Some(partial) = record.partial(ctx, "EDGE_CURVE")? else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(Value::reference(value)),
        "STEP edge geometry reference traversal",
    )
}

#[derive(Clone, Copy)]
enum TrimMasterRepresentation {
    Parameter,
    Cartesian,
    Unspecified,
}

struct TrimParameterContext<'a> {
    points: &'a BTreeMap<u64, FinitePoint3>,
    geometry: &'a CurveGeometry,
    angle_scale: f64,
    linear_parameter_scale: f64,
    parameter_offset: f64,
    tolerance: f64,
    master_representation: TrimMasterRepresentation,
    record_id: u64,
    losses: &'a mut Vec<LossNote>,
    ctx: &'a DecodeContext<'a>,
}

fn trimmed_curve_attributes(parameters: &[Value]) -> Option<(u64, bool, TrimMasterRepresentation)> {
    let basis = parameters.get(1).and_then(Value::reference)?;
    let sense = parameters.get(4).and_then(Value::logical)?;
    let master_representation = match parameters.get(5).and_then(Value::enumeration)? {
        "PARAMETER" => TrimMasterRepresentation::Parameter,
        "CARTESIAN" => TrimMasterRepresentation::Cartesian,
        "UNSPECIFIED" => TrimMasterRepresentation::Unspecified,
        _ => return None,
    };
    Some((basis, sense, master_representation))
}

fn surface_curve_basis(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(record.parameter(1).and_then(Value::reference));
    }
    let partial = record
        .partial(ctx, "SURFACE_CURVE")?
        .map_or_else(
            || record.partial(ctx, "SEAM_CURVE"),
            |partial| Ok(Some(partial)),
        )?
        .map_or_else(
            || record.partial(ctx, "INTERSECTION_CURVE"),
            |partial| Ok(Some(partial)),
        )?;
    let Some(partial) = partial else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(Value::reference(value)),
        "STEP surface curve basis reference traversal",
    )
}

pub(super) struct OwnedCarriers {
    curves: HashSet<CurveIndex>,
    surfaces: HashSet<SurfaceIndex>,
    points: HashSet<PointIndex>,
}

pub(super) fn topology_owned_carriers(
    ir: &CadIr,
    index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<OwnedCarriers, CodecError> {
    let mut curves = HashSet::new();
    for curve in ctx
        .admit_iter(
            &(ir.model.edges)[..],
            "STEP topology owned carriers traversal",
        )?
        .filter_map(|edge| edge.curve())
        .chain(
            ctx.admit_iter(
                &(ir.model.coedges)[..],
                "STEP topology owned carriers chain traversal",
            )?
            .filter_map(|coedge| coedge.use_curve.as_ref().map(|use_| &use_.curve)),
        )
        .map(|curve| step_instance_id(ctx, curve.as_str()))
        .filter_map(Result::transpose)
        .map(|id| id.map(|id| index.curves.get(&id).copied()))
        .filter_map(Result::transpose)
    {
        ctx.insert_hash_set(&mut curves, curve?, "step_owned_curve_carriers")?;
    }
    let mut surfaces = HashSet::new();
    for surface in ctx
        .admit_iter(
            &(ir.model.faces)[..],
            "STEP topology owned carriers traversal",
        )?
        .map(|face| step_instance_id(ctx, face.surface.as_str()))
        .filter_map(Result::transpose)
        .map(|id| id.map(|id| index.surfaces.get(&id).copied()))
        .filter_map(Result::transpose)
    {
        ctx.insert_hash_set(&mut surfaces, surface?, "step_owned_surface_carriers")?;
    }
    let mut points = HashSet::new();
    for point in ctx
        .admit_iter(
            &(ir.model.vertices)[..],
            "STEP topology owned carriers traversal",
        )?
        .map(|vertex| step_instance_id(ctx, vertex.point.as_str()))
        .filter_map(Result::transpose)
        .map(|id| id.map(|id| index.points.get(&id).map(|point| &point.index).copied()))
        .filter_map(Result::transpose)
    {
        ctx.insert_hash_set(&mut points, point?, "step_owned_point_carriers")?;
    }
    Ok(OwnedCarriers {
        curves,
        surfaces,
        points,
    })
}

pub(super) fn associate_topology_carriers(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (edge_id, edge) in exchange.entities(ctx, "EDGE_CURVE")? {
        let Some(curve_step) = edge_curve_geometry_reference(ctx, edge)?
            .map(|curve| curve_carrier_record(ctx, curve, exchange))
            .transpose()?
            .flatten()
        else {
            continue;
        };
        let Some(index) = index.curves.get(&curve_step) else {
            continue;
        };
        if owned.curves.contains(index) {
            continue;
        }
        if ir.model.curves[index.0].source_object.is_none() {
            ir.model.curves[index.0].source_object =
                Some(super::step_source_association(ctx, edge_id, None)?);
        }
    }
    for entity in exchange.entities_any(ctx, &["ADVANCED_FACE", "FACE_SURFACE"])? {
        let (face_id, face) = entity?;
        let Some(surface_step) = face_surface_reference(ctx, face)? else {
            continue;
        };
        let Some(index) = index.surfaces.get(&surface_step) else {
            continue;
        };
        if owned.surfaces.contains(index) {
            continue;
        }
        if ir.model.surfaces[index.0].source_object.is_none() {
            ir.model.surfaces[index.0].source_object =
                Some(super::step_source_association(ctx, face_id, None)?);
        }
    }
    for (vertex_id, vertex) in exchange.entities(ctx, "VERTEX_POINT")? {
        let Some(point_step) = vertex_point_reference(ctx, vertex)? else {
            continue;
        };
        let Some(index) = index.points.get(&point_step).map(|point| &point.index) else {
            continue;
        };
        if owned.points.contains(index) {
            continue;
        }
        if ir.model.points[index.0].source_object.is_none() {
            ir.model.points[index.0].source_object =
                Some(super::step_source_association(ctx, vertex_id, None)?);
        }
    }
    Ok(())
}

/// Associate the basis carrier owned by each valid replica with that replica.
///
/// A replica is the carrier used by topology, while its basis remains a
/// separate IR geometry entry because the transformed geometry stores the
/// basis inline. The basis is still a real STEP dependency and must not be
/// reported as an unowned carrier by generic IR validation.
pub(super) fn associate_replica_bases(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (replica_id, record) in exchange.entities(ctx, "CURVE_REPLICA")? {
        let Some(parent_id) =
            named_parameter(ctx, record, "CURVE_REPLICA", 1)?.and_then(Value::reference)
        else {
            continue;
        };
        let Some(parent_index) = index.curves.get(&parent_id).copied() else {
            continue;
        };
        if ir.model.curves[parent_index.0].source_object.is_none() {
            ir.model.curves[parent_index.0].source_object =
                Some(super::step_source_association(ctx, replica_id, None)?);
        }
    }
    for (replica_id, record) in exchange.entities(ctx, "SURFACE_REPLICA")? {
        let Some(parent_id) =
            named_parameter(ctx, record, "SURFACE_REPLICA", 1)?.and_then(Value::reference)
        else {
            continue;
        };
        let Some(parent_index) = index.surfaces.get(&parent_id).copied() else {
            continue;
        };
        if ir.model.surfaces[parent_index.0].source_object.is_none() {
            ir.model.surfaces[parent_index.0].source_object =
                Some(super::step_source_association(ctx, replica_id, None)?);
        }
    }
    Ok(())
}

/// Associate surfaces referenced only as PCURVE supports with their STEP
/// PCURVE records. The canonical pcurve stores its parameter-space geometry
/// inline, so this source association preserves reachability of the separate
/// support carrier.
pub(super) fn associate_pcurve_supports(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut owned_pcurves = BTreeSet::new();
    for coedge in ctx.admit_iter(&ir.model.coedges[..], "STEP owned coedge pcurve traversal")? {
        for use_ in ctx.admit_iter(
            coedge.pcurves.as_slice(),
            "STEP owned coedge pcurve use traversal",
        )? {
            ctx.insert_btree_set(
                &mut owned_pcurves,
                use_.pcurve.as_str(),
                "step_owned_pcurve_supports",
            )?;
        }
    }
    for loop_ in ctx.admit_iter(&ir.model.loops[..], "STEP owned loop pcurve traversal")? {
        for pcurve in ctx.admit_iter(
            loop_
                .singular_vertex()
                .map(|(_, pcurves)| pcurves)
                .unwrap_or_default(),
            "STEP owned singular vertex pcurve traversal",
        )? {
            ctx.insert_btree_set(
                &mut owned_pcurves,
                pcurve.pcurve.as_str(),
                "step_owned_pcurve_supports",
            )?;
        }
        for use_ in ctx.admit_iter(
            loop_.anchored_vertex_uses(),
            "STEP owned anchored vertex traversal",
        )? {
            for pcurve in ctx.admit_iter(
                use_.pcurves.as_slice(),
                "STEP owned anchored vertex pcurve traversal",
            )? {
                ctx.insert_btree_set(
                    &mut owned_pcurves,
                    pcurve.pcurve.as_str(),
                    "step_owned_pcurve_supports",
                )?;
            }
        }
    }
    for surface in ctx.admit_iter(
        &ir.model.procedural_surfaces[..],
        "STEP owned procedural surface traversal",
    )? {
        if let ProceduralSurfaceDefinition::CurveBounded {
            boundary_pcurves, ..
        } = surface.definition()
        {
            for pcurve in ctx.admit_iter(
                boundary_pcurves.as_slice(),
                "STEP owned boundary pcurve traversal",
            )? {
                ctx.insert_btree_set(
                    &mut owned_pcurves,
                    pcurve.as_str(),
                    "step_owned_pcurve_supports",
                )?;
            }
        }
    }

    for (pcurve_id, record) in exchange.entities(ctx, "PCURVE")? {
        let pcurve_identity = ids::data(kind!("pcurve"), pcurve_id);
        if !ctx.contains_btree_set(
            &owned_pcurves,
            pcurve_identity.as_str(),
            "STEP owned pcurves membership",
        )? {
            continue;
        }
        let Some(surface_id) =
            named_parameter(ctx, record, "PCURVE", 1)?.and_then(Value::reference)
        else {
            continue;
        };
        let Some(surface_index) = index.surfaces.get(&surface_id).copied() else {
            continue;
        };
        if ir.model.surfaces[surface_index.0].source_object.is_none() {
            ir.model.surfaces[surface_index.0].source_object =
                Some(super::step_source_association(ctx, pcurve_id, None)?);
        }
    }
    Ok(())
}

/// Associate surfaces listed by retained `SURFACE_CURVE` records.
///
/// The associated-geometry list normally contains PCURVE records, but some
/// producers write the surface carriers directly. The IR stores the 3D basis
/// curve rather than the STEP wrapper, so this pass projects the wrapper's
/// surface dependencies onto the retained IR surfaces. Only wrappers that
/// participate in topology or an explicit free-geometry/presentation owner
/// are considered; an unrelated record with the same basis curve is not an
/// ownership proof.
pub(super) fn associate_surface_curve_supports(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let retained = retained_surface_curve_ids(exchange, index, owned, ctx)?;
    for entity in
        exchange.entities_any(ctx, &["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"])?
    {
        let (surface_curve_id, record) = entity?;
        if !retained.contains(&surface_curve_id) {
            continue;
        }
        if let Some(curve_index) = surface_curve_basis(ctx, record)?
            .and_then(|basis| index.curves.get(&basis))
            .copied()
        {
            if ir.model.curves[curve_index.0].source_object.is_none() {
                ir.model.curves[curve_index.0].source_object =
                    Some(super::step_source_association(ctx, surface_curve_id, None)?);
            }
        }
        surface_curve_supports(ctx, record, exchange, index, &mut |surface_id| {
            let Some(surface_index) = index.surfaces.get(&surface_id).copied() else {
                return Ok(());
            };
            if ir.model.surfaces[surface_index.0].source_object.is_none() {
                ir.model.surfaces[surface_index.0].source_object =
                    Some(super::step_source_association(ctx, surface_curve_id, None)?);
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn retained_surface_curve_ids(
    exchange: &Exchange,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut retained = BTreeSet::new();

    for (_, edge) in exchange.entities(ctx, "EDGE_CURVE")? {
        let Some(surface_curve) = edge_curve_geometry_reference(ctx, edge)? else {
            continue;
        };
        let Some(record) = exchange.records().get(&surface_curve) else {
            continue;
        };
        if !is_surface_curve_record(ctx, record)? {
            continue;
        }
        let Some(basis) = surface_curve_basis(ctx, record)? else {
            continue;
        };
        if index
            .curves
            .get(&basis)
            .is_some_and(|curve| owned.curves.contains(curve))
        {
            ctx.insert_btree_set(
                &mut retained,
                surface_curve,
                "step_retained_surface_curve_ids",
            )?;
        }
    }

    for set in ctx
        .admit_iter(
            exchange.records(),
            "STEP retained surface curve ids map traversal",
        )?
        .map(|(_, value)| value)
    {
        let Some(set_type) = entity_type(ctx, set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])?
        else {
            continue;
        };
        let Some(members) = named_parameter(ctx, set, set_type, 1)?.and_then(Value::list) else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            if decoded_surface_curve(ctx, member, exchange, index)? {
                ctx.insert_btree_set(&mut retained, member, "step_retained_surface_curve_ids")?;
            }
        }
    }

    for representation in ctx
        .admit_iter(
            exchange.records(),
            "STEP retained surface curve ids map traversal",
        )?
        .map(|(_, value)| value)
    {
        if !ctx.any_by(
            &representation.partials[..],
            |partial| Ok(super::representation::is_representation_name(&partial.name)),
            "STEP representation association partial traversal",
        )? {
            continue;
        }
        let Some(items) = representation_items(ctx, representation)? else {
            continue;
        };
        for item in items {
            if decoded_surface_curve(ctx, item, exchange, index)? {
                ctx.insert_btree_set(&mut retained, item, "step_retained_surface_curve_ids")?;
            }
        }
    }

    for record in ctx
        .admit_iter(
            exchange.records(),
            "STEP retained surface curve ids map traversal",
        )?
        .map(|(_, value)| value)
    {
        if let Some(target) = super::presentation::styled_item_target(ctx, record)? {
            if decoded_surface_curve(ctx, target, exchange, index)? {
                ctx.insert_btree_set(&mut retained, target, "step_retained_surface_curve_ids")?;
            }
        }
    }
    for (_, plane) in exchange.entities(ctx, "ANNOTATION_PLANE")? {
        for partial in ctx.admit_iter(
            &(plane.partials)[..],
            "STEP retained surface curve ids traversal",
        )? {
            for parameter in ctx.admit_iter(
                partial.parameters.as_slice(),
                "STEP record parameter traversal",
            )? {
                for target in super::reference::references(parameter, ctx) {
                    let target = target?;
                    if decoded_surface_curve(ctx, target, exchange, index)? {
                        ctx.insert_btree_set(
                            &mut retained,
                            target,
                            "step_retained_surface_curve_ids",
                        )?;
                    }
                }
            }
        }
    }

    Ok(retained)
}

fn decoded_surface_curve(
    ctx: &DecodeContext<'_>,
    id: u64,
    exchange: &Exchange,
    index: &CarrierIndex,
) -> Result<bool, CodecError> {
    let Some(record) = exchange.records().get(&id) else {
        return Ok(false);
    };
    Ok(is_surface_curve_record(ctx, record)?
        && surface_curve_basis(ctx, record)?.is_some_and(|basis| index.curves.contains_key(&basis)))
}

fn is_surface_curve_record(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<bool, CodecError> {
    ctx.any_by(
        &record.partials[..],
        |partial| {
            Ok(matches!(
                partial.name.as_str(),
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
            ))
        },
        "STEP surface curve partial traversal",
    )
}

fn surface_curve_supports(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    exchange: &Exchange,
    index: &CarrierIndex,
    visitor: &mut impl FnMut(u64) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let values = surface_curve_associated_geometry(ctx, record)?.unwrap_or_default();
    for associated in ctx
        .admit_iter(values, "STEP surface curve associated reference traversal")?
        .filter_map(Value::reference)
    {
        let surface = exchange
            .records()
            .get(&associated)
            .map(|record| named_parameter(ctx, record, "PCURVE", 1))
            .transpose()?
            .flatten()
            .and_then(Value::reference)
            .or_else(|| {
                index
                    .surfaces
                    .contains_key(&associated)
                    .then_some(associated)
            });
        if let Some(surface) = surface.filter(|surface| index.surfaces.contains_key(surface)) {
            visitor(surface)?;
        }
    }
    Ok(())
}

fn surface_curve_associated_geometry<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, CodecError> {
    if record.partials.len() == 1 {
        return Ok(record.parameter(2).and_then(Value::list));
    }
    let partial = record
        .partial(ctx, "SURFACE_CURVE")?
        .map_or_else(
            || record.partial(ctx, "SEAM_CURVE"),
            |partial| Ok(Some(partial)),
        )?
        .map_or_else(
            || record.partial(ctx, "INTERSECTION_CURVE"),
            |partial| Ok(Some(partial)),
        )?;
    let Some(partial) = partial else {
        return Ok(None);
    };
    ctx.find_map(
        partial.parameters.as_slice(),
        |value| Ok(Value::list(value)),
        "STEP surface curve associated parameter traversal",
    )
}

fn resolve_unit_scales(
    exchange: &Exchange,
    default_length: PositiveReal,
    default_angle: PositiveReal,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<UnitScales, CodecError> {
    let mut length_candidates = BTreeMap::<u64, Vec<PositiveReal>>::new();
    let mut angle_candidates = BTreeMap::<u64, Vec<PositiveReal>>::new();
    for (&representation_id, representation) in
        ctx.admit_iter(exchange.records(), "STEP resolve unit scales traversal")?
    {
        if !is_representation_record(ctx, representation)? {
            continue;
        }
        let Some(context_id) = representation_context(ctx, representation)? else {
            continue;
        };
        let (length, angle) = context_unit_scales(context_id, exchange, ctx)?;
        if length.is_none() && angle.is_none() {
            continue;
        }
        if let Some(length) = length {
            add_unit_candidate(
                &mut length_candidates,
                representation_id,
                length,
                ctx,
                "step_length_candidate_groups",
                "step_length_candidate_values",
            )?;
        }
        if let Some(angle) = angle {
            add_unit_candidate(
                &mut angle_candidates,
                representation_id,
                angle,
                ctx,
                "step_angle_candidate_groups",
                "step_angle_candidate_values",
            )?;
        }
        let Some(items) = representation_items(ctx, representation)? else {
            continue;
        };
        let mut members = BTreeSet::new();
        for item in items {
            collect_unit_scope_members(item, exchange, &mut members, &mut BTreeSet::new(), ctx)?;
        }
        for member in members {
            if let Some(length) = length {
                add_unit_candidate(
                    &mut length_candidates,
                    member,
                    length,
                    ctx,
                    "step_length_candidate_groups",
                    "step_length_candidate_values",
                )?;
            }
            if let Some(angle) = angle {
                add_unit_candidate(
                    &mut angle_candidates,
                    member,
                    angle,
                    ctx,
                    "step_angle_candidate_groups",
                    "step_angle_candidate_values",
                )?;
            }
        }
    }
    let length =
        finalize_unit_candidates(length_candidates, default_length, "length", losses, ctx)?;
    let angle =
        finalize_unit_candidates(angle_candidates, default_angle, "plane-angle", losses, ctx)?;
    Ok(UnitScales {
        default_length,
        default_angle,
        length,
        angle,
    })
}

fn add_unit_candidate(
    candidates: &mut BTreeMap<u64, Vec<PositiveReal>>,
    id: u64,
    scale: PositiveReal,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    value_operation: &'static str,
) -> Result<(), CodecError> {
    ctx.admit_btree_entry(candidates, &id, group_operation)?;
    let values = candidates.entry(id).or_default();
    ctx.push_vec(values, scale, value_operation)
}

fn finalize_unit_candidates(
    candidates: BTreeMap<u64, Vec<PositiveReal>>,
    default: PositiveReal,
    dimension: &str,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, PositiveReal>, CodecError> {
    let mut selected = BTreeMap::new();
    let mut ambiguous = 0;
    for (id, values) in candidates {
        match unique_scale(ctx, &values)? {
            Some(scale) if scale != default => {
                ctx.insert_btree_map(&mut selected, id, scale, "step_unit_selected_scales")?;
            }
            Some(_) => {}
            None => ambiguous += 1,
        }
    }
    if ambiguous > 0 {
        ctx.push_vec(losses, StepLossCode::ConflictingRepresentationUnits.note(ctx.format_retained(format_args!(
                "{ambiguous} geometry record(s) belong to representations with conflicting {dimension} units; source-order unit selection was not applied"
            ), "STEP finalize_unit_candidates text")?), "step_geometry_losses")?;
    }
    Ok(selected)
}

fn unique_scale(
    ctx: &DecodeContext<'_>,
    values: &[PositiveReal],
) -> Result<Option<PositiveReal>, CodecError> {
    let Some(first) = values.first().copied() else {
        return Ok(None);
    };
    Ok(ctx
        .all_by(
            values,
            |value| Ok(same_scale(*value, first)),
            "STEP unique unit scale traversal",
        )?
        .then_some(first))
}

fn same_scale(left: PositiveReal, right: PositiveReal) -> bool {
    let left = left.get();
    let right = right.get();
    let tolerance = EPS_GEOMETRY_READ_EXACT_GEOMETRY * left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= tolerance
}

fn is_representation_record(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<bool, CodecError> {
    ctx.any_by(
        &record.partials[..],
        |partial| Ok(super::representation::is_representation_name(&partial.name)),
        "STEP is representation record traversal",
    )
}

fn representation_context(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    for partial in ctx
        .admit_iter(
            &record.partials[..],
            "STEP representation context partial traversal",
        )?
        .filter(|partial| super::representation::is_representation_name(&partial.name))
    {
        if let Some(reference) = ctx.find_map(
            partial.parameters.as_slice().iter().rev(),
            |value| Ok(Value::reference(value)),
            "STEP representation context reference traversal",
        )? {
            return Ok(Some(reference));
        }
    }
    Ok(None)
}

fn context_unit_scales(
    id: u64,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<(Option<PositiveReal>, Option<PositiveReal>), CodecError> {
    let Some(context) = exchange.records().get(&id) else {
        return Ok((None, None));
    };
    let Some(units) = context
        .partial(ctx, "GLOBAL_UNIT_ASSIGNED_CONTEXT")?
        .and_then(|partial| partial.parameters.first())
        .and_then(Value::list)
    else {
        return Ok((None, None));
    };
    let mut length_values = Vec::new();
    let mut angle_values = Vec::new();
    for unit in units.iter().filter_map(Value::reference) {
        if let Some(scale) = unit_scale_mm(unit, exchange, &mut BTreeSet::new(), ctx)? {
            ctx.push_vec(&mut length_values, scale, "step_context_length_scales")?;
        }
        if let Some(scale) = unit_scale_radians(unit, exchange, &mut BTreeSet::new(), ctx)? {
            ctx.push_vec(&mut angle_values, scale, "step_context_angle_scales")?;
        }
    }
    let length = unique_scale(ctx, &length_values)?;
    let angle = unique_scale(ctx, &angle_values)?;
    Ok((length, angle))
}

fn collect_unit_scope_members(
    id: u64,
    exchange: &Exchange,
    members: &mut BTreeSet<u64>,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _root_depth = ctx.enter_nested("step_unit_scope_walk")?;
    let mut pending = Vec::new();
    let mut next = Some(id);
    while let Some(current) = next {
        let _nested = (current != id)
            .then(|| ctx.enter_nested("step_unit_scope_walk"))
            .transpose()?;
        if ctx.insert_btree_set(active, current, "step_unit_scope_active")? {
            if let Some(record) = exchange.records().get(&current) {
                if !is_unit_record(ctx, record)? && !is_representation_context_record(ctx, record)?
                {
                    ctx.insert_btree_set(members, current, "step_unit_scope_members")?;
                    if record.partial(ctx, "PCURVE")?.is_none() {
                        if let Some(mapped) = record.partial(ctx, "MAPPED_ITEM")? {
                            // The mapping source keeps the units of its mapped representation.
                            // Only the mapping target belongs to this context.
                            if let Some(target) =
                                mapped.parameters.last().and_then(Value::reference)
                            {
                                ctx.push_vec(&mut pending, target, "step_unit_scope_pending")?;
                            }
                        } else {
                            for partial in ctx.admit_iter(
                                &(record.partials)[..],
                                "STEP collect unit scope members traversal",
                            )? {
                                for parameter in ctx.admit_iter(
                                    partial.parameters.as_slice(),
                                    "STEP record parameter traversal",
                                )? {
                                    for reference in super::reference::references(parameter, ctx) {
                                        let reference = reference?;
                                        let Some(referenced) = exchange.records().get(&reference)
                                        else {
                                            continue;
                                        };
                                        if is_representation_record(ctx, referenced)?
                                            || is_unit_record(ctx, referenced)?
                                            || is_representation_context_record(ctx, referenced)?
                                        {
                                            continue;
                                        }
                                        ctx.push_vec(
                                            &mut pending,
                                            reference,
                                            "step_unit_scope_pending",
                                        )?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        next = pending.pop();
    }
    Ok(())
}

fn is_unit_record(ctx: &DecodeContext<'_>, record: &RawRecord) -> Result<bool, CodecError> {
    ctx.any_by(
        &record.partials[..],
        |partial| {
            Ok(matches!(
                partial.name.as_str(),
                "LENGTH_UNIT"
                    | "PLANE_ANGLE_UNIT"
                    | "SOLID_ANGLE_UNIT"
                    | "NAMED_UNIT"
                    | "SI_UNIT"
                    | "CONVERSION_BASED_UNIT"
            ))
        },
        "STEP is unit record traversal",
    )
}

fn is_representation_context_record(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<bool, CodecError> {
    ctx.any_by(
        &record.partials[..],
        |partial| {
            Ok(matches!(
                partial.name.as_str(),
                "REPRESENTATION_CONTEXT"
                    | "GEOMETRIC_REPRESENTATION_CONTEXT"
                    | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                    | "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT"
            ))
        },
        "STEP is representation context record traversal",
    )
}

fn length_scale(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    document_unit_scale(exchange, UnitScaleKind::Length, ctx)
}

fn plane_angle_scale(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    document_unit_scale(exchange, UnitScaleKind::Angle, ctx)
}

#[derive(Clone, Copy)]
enum UnitScaleKind {
    Length,
    Angle,
}

impl UnitScaleKind {
    fn partial(self) -> &'static str {
        match self {
            Self::Length => "LENGTH_UNIT",
            Self::Angle => "PLANE_ANGLE_UNIT",
        }
    }

    fn resolve(
        self,
        id: u64,
        exchange: &Exchange,
        active: &mut BTreeSet<u64>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<PositiveReal>, CodecError> {
        match self {
            Self::Length => unit_scale_mm(id, exchange, active, ctx),
            Self::Angle => unit_scale_radians(id, exchange, active, ctx),
        }
    }
}

fn document_unit_scale(
    exchange: &Exchange,
    kind: UnitScaleKind,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    let dimension_partial = kind.partial();
    let mut context_scales = Vec::new();
    let mut has_context_unit = false;

    for record in ctx
        .admit_iter(exchange.records(), "STEP document unit scale map traversal")?
        .map(|(_, value)| value)
    {
        let Some(units) = record
            .partial(ctx, "GLOBAL_UNIT_ASSIGNED_CONTEXT")?
            .and_then(|partial| partial.parameters.first())
            .and_then(Value::list)
        else {
            continue;
        };
        let mut unit_ids = Vec::new();
        for id in ctx
            .admit_iter(units, "STEP document unit reference traversal")?
            .filter_map(Value::reference)
        {
            if exchange
                .records()
                .get(&id)
                .map(|unit| unit.partial(ctx, dimension_partial))
                .transpose()?
                .flatten()
                .is_some()
            {
                ctx.push_vec(&mut unit_ids, id, "step_document_unit_ids")?;
            }
        }
        if unit_ids.is_empty() {
            continue;
        }
        has_context_unit = true;
        let mut scales = Vec::new();
        for id in unit_ids {
            let Some(scale) = kind.resolve(id, exchange, &mut BTreeSet::new(), ctx)? else {
                return Ok(None);
            };
            ctx.push_vec(&mut scales, scale, "step_document_unit_scales")?;
        }
        let Some(scale) = unique_scale(ctx, &scales)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut context_scales, scale, "step_document_context_scales")?;
    }

    if has_context_unit {
        return Ok(unique_scale(ctx, &context_scales)?);
    }

    // STEP assigns units to representation contexts, not to the document.
    // This branch is CADIR salvage for an unscoped dimension: accept only a
    // scale to which every unit occurrence in the exchange resolves.
    let mut scales = Vec::new();
    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP document unit scale traversal")? {
        if record.partial(ctx, dimension_partial)?.is_none() {
            continue;
        }
        let Some(scale) = kind.resolve(id, exchange, &mut BTreeSet::new(), ctx)? else {
            return Ok(None);
        };
        ctx.push_vec(&mut scales, scale, "step_document_fallback_scales")?;
    }
    Ok(unique_scale(ctx, &scales)?)
}

pub(super) fn unit_scale_radians(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    unit_scale_radians_inner(id, exchange, active, 0, ctx)
}

fn unit_scale_radians_inner(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    if depth >= 256 {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("step_angle_unit_scale_walk")?;
    if active.contains(&id) {
        return Ok(None);
    }
    ctx.insert_btree_set(active, id, "step_angle_unit_active")?;
    let result = (|| -> Result<Option<f64>, CodecError> {
        let Some(record) = exchange.records().get(&id) else {
            return Ok(None);
        };
        if let Some(unit) = record.partial(ctx, "SI_UNIT")? {
            if unit.parameters.get(1).and_then(Value::enumeration) == Some("RADIAN") {
                let prefix = match unit.parameters.first() {
                    Some(Value::Omitted) => Some(1.0),
                    Some(Value::Enumeration(prefix)) => si_prefix(prefix),
                    _ => None,
                };
                Ok(prefix)
            } else {
                Ok(None)
            }
        } else if let Some(unit) = record.partial(ctx, "CONVERSION_BASED_UNIT")? {
            let Some(factor_id) = unit.parameters.get(1).and_then(Value::reference) else {
                return Ok(None);
            };
            let Some(factor) = exchange.records().get(&factor_id) else {
                return Ok(None);
            };
            let Some(value) =
                find_record_value(factor, ctx, |value| Ok(ValueExt::typed_number(value)))?
            else {
                return Ok(None);
            };
            let Some(base_id) =
                find_record_value(factor, ctx, |value| Ok(Value::reference(value)))?
            else {
                return Ok(None);
            };
            Ok(
                unit_scale_radians_inner(base_id, exchange, active, depth + 1, ctx)?
                    .map(|base| value * base.get()),
            )
        } else {
            Ok(None)
        }
    })();
    active.remove(&id);
    Ok(result?.and_then(PositiveReal::new))
}

pub(super) fn unit_scale_mm(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    unit_scale_mm_inner(id, exchange, active, 0, ctx)
}

fn unit_scale_mm_inner(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PositiveReal>, CodecError> {
    if depth >= 256 {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("step_length_unit_scale_walk")?;
    if active.contains(&id) {
        return Ok(None);
    }
    ctx.insert_btree_set(active, id, "step_length_unit_active")?;
    let result = (|| -> Result<Option<f64>, CodecError> {
        let Some(record) = exchange.records().get(&id) else {
            return Ok(None);
        };
        if let Some(unit) = record.partial(ctx, "SI_UNIT")? {
            if unit.parameters.get(1).and_then(Value::enumeration) == Some("METRE") {
                let prefix = match unit.parameters.first() {
                    Some(Value::Omitted) => Some(1.0),
                    Some(Value::Enumeration(prefix)) => si_prefix(prefix),
                    _ => None,
                };
                Ok(prefix.map(|prefix| prefix * 1000.0))
            } else {
                Ok(None)
            }
        } else if let Some(unit) = record.partial(ctx, "CONVERSION_BASED_UNIT")? {
            let Some(factor_id) = unit.parameters.get(1).and_then(Value::reference) else {
                return Ok(None);
            };
            let Some(factor) = exchange.records().get(&factor_id) else {
                return Ok(None);
            };
            let Some(value) =
                find_record_value(factor, ctx, |value| Ok(ValueExt::typed_number(value)))?
            else {
                return Ok(None);
            };
            let Some(base_id) = find_record_value(factor, ctx, |value| Ok(value.reference()))?
            else {
                return Ok(None);
            };
            Ok(
                unit_scale_mm_inner(base_id, exchange, active, depth + 1, ctx)?
                    .map(|base| value * base.get()),
            )
        } else {
            Ok(None)
        }
    })();
    active.remove(&id);
    Ok(result?.and_then(PositiveReal::new))
}

const SI_MICRO: f64 = EPS_GEOMETRY_READ_COARSE_GEOMETRY;
const SI_NANO: f64 = EPS_GEOMETRY_READ_GEOMETRY;
const SI_PICO: f64 = EPS_GEOMETRY_READ_EXACT_GEOMETRY;

fn si_prefix(prefix: &str) -> Option<f64> {
    Some(match prefix {
        "EXA" => 1e18,
        "PETA" => 1e15,
        "TERA" => 1e12,
        "GIGA" => 1e9,
        "MEGA" => 1e6,
        "KILO" => 1e3,
        "HECTO" => 1e2,
        "DECA" => 1e1,
        "DECI" => 1e-1,
        "CENTI" => 1e-2,
        "MILLI" => 1e-3,
        "MICRO" => SI_MICRO,
        "NANO" => SI_NANO,
        "PICO" => SI_PICO,
        "FEMTO" => 1e-15,
        "ATTO" => 1e-18,
        _ => return None,
    })
}

/// Resolve one `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` to the linear uncertainty
/// candidates it contributes, in millimetres, and the number of its measures
/// that did not resolve.
///
/// STEP scopes an uncertainty to its representation context, so each context
/// contributes for itself. One `distance_accuracy_value` name makes that value
/// the only contribution of the context. Every other context contributes each
/// of its resolvable length measures. `linear_uncertainty` merges the equal
/// contributions of all contexts and decides what a disagreement means.
fn context_length_uncertainties(
    context: &RawRecord,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<(Vec<PositiveLength>, usize), CodecError> {
    let Some(references) = context
        .partial(ctx, "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")?
        .and_then(|partial| partial.parameters.first())
        .and_then(Value::list)
    else {
        return Ok((Vec::new(), 0));
    };
    let mut measures = Vec::new();
    let mut named_count = 0;
    let mut named_value = None;
    let mut unresolved = 0;
    for uncertainty_id in references.iter().filter_map(Value::reference) {
        let Some(measure) = exchange.records().get(&uncertainty_id) else {
            unresolved += 1;
            continue;
        };
        let Some(value) =
            find_record_value(measure, ctx, |value| Ok(ValueExt::typed_number(value)))?
        else {
            unresolved += 1;
            continue;
        };
        let Some(unit) = find_record_value(measure, ctx, |value| Ok(Value::reference(value)))?
        else {
            unresolved += 1;
            continue;
        };
        if let Some(scale) = unit_scale_mm(unit, exchange, &mut BTreeSet::new(), ctx)? {
            let Some(result) = PositiveLength::new(value * scale.get()) else {
                unresolved += 1;
                continue;
            };
            // The CADIR convention applies to the name attribute, not
            // the optional description attribute.
            let named_distance_accuracy = measure
                .partial(ctx, "UNCERTAINTY_MEASURE_WITH_UNIT")?
                .and_then(|partial| partial.parameters.get(2))
                .map(|value| string_value(value, exchange, ctx))
                .transpose()?
                .flatten()
                .map(|name| {
                    ctx.eq_ignore_ascii_case(
                        name.as_str(),
                        "distance_accuracy_value",
                        "STEP distance accuracy name case equality",
                    )
                })
                .transpose()?
                .unwrap_or(false);
            if named_distance_accuracy {
                named_count += 1;
                named_value = Some(result);
            }
            ctx.push_vec(&mut measures, result, "step_uncertainty_context_measures")?;
        } else if unit_scale_radians(unit, exchange, &mut BTreeSet::new(), ctx)?.is_none() {
            unresolved += 1;
        }
    }

    if named_count == 1 {
        measures.clear();
        if let Some(value) = named_value {
            measures.push(value);
        }
    }
    Ok((measures, unresolved))
}

/// The document projection of the per-context linear uncertainty candidates.
enum LinearUncertainty {
    /// One distinct candidate, in millimetres.
    Value(PositiveLength),
    /// No candidate, with the number of measures that did not resolve.
    Empty { unresolved: usize },
    /// Several distinct candidates in millimetres, sorted and without
    /// duplicates, with the number of measures that did not resolve.
    Ambiguous {
        first: PositiveLength,
        second: PositiveLength,
        rest: Vec<PositiveLength>,
        unresolved: usize,
    },
}

fn linear_uncertainty(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<LinearUncertainty, CodecError> {
    let mut candidates: Vec<PositiveLength> = Vec::new();
    let mut unresolved = 0;
    for (_, context) in exchange.entities(ctx, "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")? {
        let (context_candidates, context_unresolved) =
            context_length_uncertainties(context, exchange, ctx)?;
        unresolved += context_unresolved;
        for candidate in context_candidates {
            // Exact equality: the candidates come from one file, so equal
            // declarations corroborate each other and are not a conflict.
            if !candidates.contains(&candidate) {
                ctx.push_vec(
                    &mut candidates,
                    candidate,
                    "step_uncertainty_distinct_candidates",
                )?;
            }
        }
    }
    ctx.stable_sort_by_key(
        &mut candidates,
        |value| value.get(),
        f64::total_cmp,
        "step_uncertainty_candidate_sort",
    )?;

    Ok(match candidates.len() {
        0 => LinearUncertainty::Empty { unresolved },
        1 => LinearUncertainty::Value(candidates[0]),
        _ => {
            ctx.charge_work(
                u64_from_index(candidates.len()),
                "step_uncertainty_projection",
            )?;
            let first = candidates.remove(0);
            ctx.charge_work(
                u64_from_index(candidates.len()),
                "step_uncertainty_projection",
            )?;
            let second = candidates.remove(0);
            LinearUncertainty::Ambiguous {
                first,
                second,
                rest: candidates,
                unresolved,
            }
        }
    })
}

fn string_value(
    value: &Value,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(None);
    };
    match exchange.decode_string_with_context(bytes, ctx) {
        Ok(value) => Ok(Some(value)),
        Err(crate::strings::StringDecodeFailure::Invalid(_)) => Ok(None),
        Err(crate::strings::StringDecodeFailure::Resource(error)) => Err(error),
    }
}

fn trim_parameter(
    value: &Value,
    context: &mut TrimParameterContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let (parameter, cartesian) = match value {
        Value::List(values) => (
            context.ctx.find_by(
                &values[..],
                |value| Ok(is_parameter_trim_value(value)),
                "STEP trim parameter traversal",
            )?,
            context.ctx.find_by(
                &values[..],
                |value| Ok(matches!(value, Value::Reference(_))),
                "STEP trim parameter traversal",
            )?,
        ),
        value if is_parameter_trim_value(value) => (Some(value), None),
        Value::Reference(_) => (None, Some(value)),
        _ => (None, None),
    };
    select_trim_parameter(parameter, cartesian, context)
}

fn trimmed_curve_parameter_range(
    geometry: &CurveGeometry,
    start: f64,
    end: f64,
    sense: bool,
) -> [f64; 2] {
    let mut start = start;
    let mut end = end;
    // A closed STEP curve may cross its parameter seam. Move the endpoint
    // that follows the declared traversal onto the next parameter branch
    // before projecting the directed trim onto the IR's ordered interval.
    if let Some(domain) = curve_periodic_domain(geometry) {
        if sense && end < start {
            end = shift_periodic_parameter(end, domain);
        } else if !sense && start < end {
            start = shift_periodic_parameter(start, domain);
        }
    }
    let range = if sense { [start, end] } else { [end, start] };
    if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    }
}

fn curve_periodic_domain(geometry: &CurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
            Some([0.0, std::f64::consts::TAU])
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) if curve.periodic() => {
            let [lower, upper] = nurbs_curve_parameter_domain(curve)?.endpoints();
            (upper > lower).then_some([lower, upper])
        }
        _ => None,
    }
}

fn is_parameter_trim_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) | Value::Real(_) => true,
        Value::Typed(name, _) => name == "PARAMETER_VALUE",
        _ => false,
    }
}

fn trim_parameter_value(
    value: &Value,
    context: &TrimParameterContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let _depth = context.ctx.enter_nested("step_trim_parameter_value_walk")?;
    let Some(geometry) = context.geometry.solved() else {
        return Ok(None);
    };
    let scale = parameter_scale(
        geometry,
        context.angle_scale,
        context.linear_parameter_scale,
        context.ctx,
    )?;
    match value {
        Value::Integer(value) => Ok(cadmpeg_core::convert::f64_from_i64(*value)
            .map(|value| scale * value + context.parameter_offset)),
        Value::Real(value) => Ok(Some(scale * value.get() + context.parameter_offset)),
        Value::Typed(name, value) if name == "PARAMETER_VALUE" => {
            trim_parameter_value(value, context)
        }
        _ => Ok(None),
    }
}

fn trim_cartesian_parameter(
    value: &Value,
    context: &TrimParameterContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let Value::Reference(id) = value else {
        return Ok(None);
    };
    let Some(point) = context.points.get(id) else {
        return Ok(None);
    };
    let Some(geometry) = context.geometry.solved() else {
        return Ok(None);
    };
    curve_parameter_at_point(context.ctx, geometry, point.get(), context.tolerance)
}

fn select_trim_parameter(
    parameter: Option<&Value>,
    cartesian: Option<&Value>,
    context: &mut TrimParameterContext<'_>,
) -> Result<Option<f64>, CodecError> {
    match context.master_representation {
        TrimMasterRepresentation::Parameter => {
            if let Some(value) = parameter {
                trim_parameter_value(value, context)
            } else {
                if cartesian.is_some() {
                    (context.ctx).push_vec(context.losses, StepLossCode::DecodeWarning.note(format!(
                            "TRIMMED_CURVE #{} fell back to a Cartesian trim selector because master_representation is .PARAMETER.",
                            context.record_id
                        )), "step_trim_parameter_fallback_losses")?;
                }
                match cartesian {
                    Some(value) => trim_cartesian_parameter(value, context),
                    None => Ok(None),
                }
            }
        }
        TrimMasterRepresentation::Cartesian => {
            if let Some(value) = cartesian {
                trim_cartesian_parameter(value, context)
            } else {
                if parameter.is_some() {
                    (context.ctx).push_vec(context.losses, StepLossCode::DecodeWarning.note(format!(
                            "TRIMMED_CURVE #{} fell back to a parameter trim selector because master_representation is .CARTESIAN.",
                            context.record_id
                        )), "step_trim_parameter_fallback_losses")?;
                }
                match parameter {
                    Some(value) => trim_parameter_value(value, context),
                    None => Ok(None),
                }
            }
        }
        TrimMasterRepresentation::Unspecified => {
            if let Some(value) = parameter {
                trim_parameter_value(value, context)
            } else {
                match cartesian {
                    Some(value) => trim_cartesian_parameter(value, context),
                    None => Ok(None),
                }
            }
        }
    }
}

fn parameter_scale(
    geometry: &SolvedCurveGeometry,
    angle_scale: f64,
    linear_parameter_scale: f64,
    ctx: &DecodeContext<'_>,
) -> Result<f64, CodecError> {
    let _depth = ctx.enter_nested("step_trim_parameter_scale_walk")?;
    match geometry {
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => Ok(angle_scale),
        SolvedCurveGeometry::Line(_) => Ok(linear_parameter_scale),
        SolvedCurveGeometry::Transformed(placed) => {
            parameter_scale(placed.basis(), angle_scale, linear_parameter_scale, ctx)
        }
        SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Nurbs(_)
        | SolvedCurveGeometry::Polyline(_)
        | SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => Ok(1.0),
    }
}

fn line_parameter_scale(
    exchange: &Exchange,
    curve: u64,
    length_scale: PositiveReal,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<PositiveReal, CodecError> {
    fn inherited_parent(
        ctx: &DecodeContext<'_>,
        record: &RawRecord,
    ) -> Result<Option<u64>, CodecError> {
        if record.partial(ctx, "CURVE_REPLICA")?.is_some() {
            return Ok(
                named_parameter(ctx, record, "CURVE_REPLICA", 1)?.and_then(ValueExt::reference)
            );
        }
        if record.partial(ctx, "TRIMMED_CURVE")?.is_some() {
            return Ok(
                named_parameter(ctx, record, "TRIMMED_CURVE", 1)?.and_then(ValueExt::reference)
            );
        }
        if record.partial(ctx, "OFFSET_CURVE_3D")?.is_some() {
            return Ok(
                named_parameter(ctx, record, "OFFSET_CURVE_3D", 1)?.and_then(ValueExt::reference)
            );
        }
        if ctx.any_by(
            &record.partials[..],
            |partial| {
                Ok(matches!(
                    partial.name.as_str(),
                    "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
                ))
            },
            "STEP inherited curve parent traversal",
        )? {
            return Ok(surface_curve_basis(ctx, record)?);
        }
        Ok(None)
    }

    fn resolve(
        exchange: &Exchange,
        curve: u64,
        length_scale: PositiveReal,
        losses: &mut Vec<LossNote>,
        visiting: &mut BTreeSet<u64>,
        ctx: &DecodeContext<'_>,
    ) -> Result<PositiveReal, CodecError> {
        if visiting.contains(&curve) {
            return Ok(length_scale);
        }
        let _depth = ctx.enter_nested("step_line_parameter_scale_walk")?;
        ctx.insert_btree_set(visiting, curve, "step_line_parameter_scale_active")?;
        let Some(record) = exchange.records().get(&curve) else {
            visiting.remove(&curve);
            return Ok(length_scale);
        };
        let result = if record.partial(ctx, "LINE")?.is_some() {
            if let Some(scale) = named_parameter(ctx, record, "LINE", 2)?
                .and_then(ValueExt::reference)
                .and_then(|vector| exchange.records().get(&vector))
                .map(|record| -> Result<_, CodecError> {
                    Ok(if record.partial(ctx, "VECTOR")?.is_some() {
                        Some(record)
                    } else {
                        None
                    })
                })
                .transpose()?
                .flatten()
                .map(|record| named_parameter(ctx, record, "VECTOR", 2))
                .transpose()?
                .flatten()
                .and_then(ValueExt::number)
                .and_then(PositiveReal::new)
                .and_then(|magnitude| PositiveReal::new(magnitude.get() * length_scale.get()))
            {
                Ok(scale)
            } else {
                ctx.push_vec(losses, StepLossCode::LineParameterScaleUnresolved.note(format!(
                        "LINE #{curve} parameter scale did not resolve; the document length scale was used"
                    )), "step_line_parameter_scale_losses")?;
                Ok(length_scale)
            }
        } else if let Some(parent) = inherited_parent(ctx, record)? {
            resolve(exchange, parent, length_scale, losses, visiting, ctx)
        } else {
            Ok(length_scale)
        };
        visiting.remove(&curve);
        result
    }

    resolve(
        exchange,
        curve,
        length_scale,
        losses,
        &mut BTreeSet::new(),
        ctx,
    )
}

fn orthogonal_reference(axis: UnitVector3, reference: UnitVector3) -> Option<UnitVector3> {
    let axis = axis.as_raw();
    let reference = reference.as_raw();
    let projection = axis.dot(*reference);
    normalize(Vector3::new(
        reference.x - projection * axis.x,
        reference.y - projection * axis.y,
        reference.z - projection * axis.z,
    ))
}

fn first_projected_axis(axis: UnitVector3) -> Option<UnitVector3> {
    project_axis(default_reference_axis(axis), axis)
}

fn curve_parameter_at_point(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedCurveGeometry,
    point: Point3,
    tolerance: f64,
) -> Result<Option<f64>, CodecError> {
    let offset =
        |origin: Point3| Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            Ok(Some(offset(origin).dot(direction)))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radial = offset(center);
            let y_axis = axis.cross(*ref_direction);
            Ok(Some(radial.dot(y_axis).atan2(radial.dot(*ref_direction))))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            let radial = offset(center);
            let minor_direction = axis.cross(*major_direction);
            Ok(Some(
                (radial.dot(minor_direction) / minor_radius)
                    .atan2(radial.dot(*major_direction) / major_radius),
            ))
        }
        SolvedCurveGeometry::Nurbs(curve) => {
            let Some(domain) = nurbs_curve_parameter_domain(curve)
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
            else {
                return Ok(None);
            };
            nurbs_curve_parameter_near_point(
                ctx,
                curve,
                point,
                tolerance,
                (domain[0] + domain[1]) * 0.5,
            )
            .map(|parameter| parameter.map(FiniteReal::get))
        }
        SolvedCurveGeometry::Transformed(placed) => {
            let Some(inverse) = placed.transform().try_inverse_affine().ok() else {
                return Ok(None);
            };
            let Some(mapped) = inverse.apply_point(point) else {
                return Ok(None);
            };
            curve_parameter_at_point(ctx, placed.basis(), mapped.get(), tolerance)
        }
        _ => Ok(None),
    }
}

type CompositeCurveData = (Vec<(u64, CompositeCurveSegment)>, Option<bool>);

fn defer_geometry_dependency(
    waiting_on: &mut HashMap<u64, Vec<u64>>,
    dependency: u64,
    id: u64,
    ctx: &DecodeContext<'_>,
    group_operation: &'static str,
    item_operation: &'static str,
) -> Result<(), CodecError> {
    if !waiting_on.contains_key(&dependency) {
        ctx.reserve_map(waiting_on, 1, group_operation)?;
    }
    let dependents = waiting_on.entry(dependency).or_default();
    ctx.push_vec(dependents, id, item_operation)
}

fn wake_deferred_dependents(
    id: u64,
    waiting_on: &mut HashMap<u64, Vec<u64>>,
    queue: &mut VecDeque<u64>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(dependents) = waiting_on.remove(&id) {
        for dependent in dependents {
            ctx.push_back(queue, dependent, operation)?;
        }
    }
    Ok(())
}

fn composite_curve_dependencies(
    record: &RawRecord,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
    visitor: &mut impl FnMut(u64) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let values = composite_curve_parameters(ctx, record)?
        .and_then(|(parameters, offset)| parameters.get(offset))
        .and_then(Value::list)
        .unwrap_or_default();
    for curve in ctx
        .admit_iter(values, "STEP composite curve dependency traversal")?
        .filter_map(Value::reference)
        .filter_map(|segment| exchange.records().get(&segment))
        .map(|record| composite_curve_segment_parameters(ctx, record))
        .filter_map(Result::transpose)
    {
        let parameters = curve?;
        let Some(curve) = parameters.get(2).and_then(Value::reference) else {
            continue;
        };
        if let Some(curve) = curve_carrier_record(ctx, curve, exchange)? {
            visitor(curve)?;
        }
    }
    Ok(())
}

fn composite_curve(
    record: &RawRecord,
    exchange: &Exchange,
    decoded: &CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<Option<CompositeCurveData>, CodecError> {
    let Some((parameters, offset)) = composite_curve_parameters(ctx, record)? else {
        return Ok(None);
    };
    let Some(values) = parameters.get(offset).and_then(Value::list) else {
        return Ok(None);
    };
    let mut segments = Vec::new();
    for value in ctx.admit_iter(values, "STEP composite curve segment traversal")? {
        let Some(id) = value.reference() else {
            return Ok(None);
        };
        let Some(record) = exchange.records().get(&id) else {
            return Ok(None);
        };
        let Some(parameters) = composite_curve_segment_parameters(ctx, record)? else {
            return Ok(None);
        };
        let fields = (|| {
            let transition = match parameters.first()?.enumeration()? {
                "DISCONTINUOUS" => CompositeCurveTransition::Discontinuous,
                "CONTINUOUS" => CompositeCurveTransition::Continuous,
                "CONTSAMEGRADIENT" => CompositeCurveTransition::ContSameGradient,
                "CONTSAMEGRADIENTSAMECURVATURE" => {
                    CompositeCurveTransition::ContSameGradientSameCurvature
                }
                _ => return None,
            };
            Some((id, parameters, transition, parameters.get(2)?.reference()?))
        })();
        let Some((id, parameters, transition, curve_step)) = fields else {
            return Ok(None);
        };
        let Some(curve_step) = curve_carrier_record(ctx, curve_step, exchange)? else {
            return Ok(None);
        };
        if !decoded.curves.contains_key(&curve_step) {
            return Ok(None);
        }
        let Some(same_sense) = parameters.get(1).and_then(Value::logical) else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut segments,
            (
                id,
                CompositeCurveSegment {
                    curve: CurveId::from(ids::data(kind!("curve"), curve_step)),
                    same_sense,
                    transition,
                },
            ),
            "step_composite_curve_segments",
        )?;
    }
    let Some(self_intersect) = parameters
        .get(offset + 1)
        .and_then(|value| logical_value(value).ok())
    else {
        return Ok(None);
    };
    Ok(Some((segments, self_intersect)))
}

fn composite_curve_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<(&'a [Value], usize)>, CodecError> {
    for name in ["COMPOSITE_CURVE", "BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"] {
        if let Some(partial) = record.partial(ctx, name)? {
            return Ok(Some((
                partial.parameters.as_slice(),
                usize::from(record.partials.len() == 1),
            )));
        }
    }
    Ok(None)
}

fn composite_curve_segment_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, CodecError> {
    Ok(record
        .partial(ctx, "COMPOSITE_CURVE_SEGMENT")?
        .map(|partial| partial.parameters.as_slice()))
}

fn boundary_pcurve_steps(
    boundary: u64,
    support: u64,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
    visitor: &mut impl FnMut(u64) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let values = exchange
        .records()
        .get(&boundary)
        .map(|record| composite_curve_parameters(ctx, record))
        .transpose()?
        .flatten()
        .and_then(|(parameters, offset)| parameters.get(offset))
        .and_then(Value::list)
        .unwrap_or_default();
    for curve in ctx
        .admit_iter(values, "STEP boundary pcurve segment traversal")?
        .filter_map(Value::reference)
        .filter_map(|segment| exchange.records().get(&segment))
        .map(|record| composite_curve_segment_parameters(ctx, record))
        .filter_map(Result::transpose)
    {
        let parameters = curve?;
        let Some(curve) = parameters
            .get(2)
            .and_then(Value::reference)
            .and_then(|curve| exchange.records().get(&curve))
        else {
            continue;
        };
        if !is_surface_curve_record(ctx, curve)? {
            continue;
        }
        for pcurve in surface_curve_pcurves(ctx, curve)? {
            if exchange
                .records()
                .get(&pcurve)
                .map(|record| named_parameter(ctx, record, "PCURVE", 1))
                .transpose()?
                .flatten()
                .and_then(Value::reference)
                == Some(support)
            {
                visitor(pcurve)?;
            }
        }
    }
    Ok(())
}

fn surface_curve_pcurves<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<impl Iterator<Item = u64> + 'a, CodecError> {
    let value = if record.partials.len() == 1 {
        record.parameter(2)
    } else {
        record
            .partial(ctx, "SURFACE_CURVE")?
            .map_or_else(
                || record.partial(ctx, "SEAM_CURVE"),
                |partial| Ok(Some(partial)),
            )?
            .map_or_else(
                || record.partial(ctx, "INTERSECTION_CURVE"),
                |partial| Ok(Some(partial)),
            )?
            .and_then(|partial| partial.parameters.get(1))
    };
    let mut values = value.and_then(Value::list).unwrap_or_default();
    if !ctx.all_by(
        values,
        |value| Ok(value.reference().is_some()),
        "STEP surface pcurve reference validation traversal",
    )? {
        values = &[];
    }
    Ok(ctx
        .admit_iter(values, "STEP surface pcurve reference traversal")?
        .filter_map(Value::reference))
}

fn logical_value(value: &Value) -> Result<Option<bool>, ()> {
    match value {
        Value::Enumeration(value) if value == "T" => Ok(Some(true)),
        Value::Enumeration(value) if value == "F" => Ok(Some(false)),
        Value::Enumeration(value) if value == "U" => Ok(None),
        _ => Err(()),
    }
}

fn periodic_value(
    value: Option<&Value>,
    field: &str,
    record_id: u64,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<bool>, CodecError> {
    let Some(value) = value.and_then(|value| logical_value(value).ok()) else {
        return Ok(None);
    };
    match value {
        Some(value) => Ok(Some(value)),
        None => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "{field} #{record_id} has UNKNOWN periodicity; decoded as non-periodic"
                    ),
                    "STEP periodic_value text",
                )?),
                "step_geometry_losses",
            )?;
            Ok(Some(false))
        }
    }
}

fn coordinate_rows(
    record: &RawRecord,
    scale: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<FinitePoint3>>, CodecError> {
    for partial in ctx.admit_iter(&record.partials[..], "STEP coordinate rows traversal")? {
        for rows in ctx
            .admit_iter(
                partial.parameters.as_slice(),
                "STEP record parameter traversal",
            )?
            .filter_map(ValueExt::list)
        {
            let mut vertices = Vec::new();
            let mut valid = true;
            for row in ctx.admit_iter(rows, "STEP coordinate rows borrowed traversal")? {
                let point = (|| {
                    let values = row.list()?;
                    if values.len() != 3 {
                        return None;
                    }
                    FinitePoint3::new(Point3::new(
                        values[0].number()? * scale,
                        values[1].number()? * scale,
                        values[2].number()? * scale,
                    ))
                })();
                let Some(point) = point else {
                    valid = false;
                    break;
                };
                ctx.push_vec(&mut vertices, point, "step_curve_coordinate_rows")?;
            }
            if valid && !vertices.is_empty() {
                return Ok(Some(vertices));
            }
        }
    }
    Ok(None)
}

fn named_coordinates(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    name: &str,
    index: usize,
    scale: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    let Some(values) = named_parameter(ctx, record, name, index)?.and_then(Value::list) else {
        return Ok(None);
    };
    if values.len() != 3 {
        return Ok(None);
    }
    Ok((|| {
        FinitePoint3::new(Point3::new(
            values[0].number()? * scale,
            values[1].number()? * scale,
            values[2].number()? * scale,
        ))
    })())
}

fn apll_point_coordinates(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    point_type: &str,
    scale: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    let mut values = if record.partials.len() == 1 {
        named_parameter(ctx, record, point_type, 1)?.and_then(Value::list)
    } else {
        None
    };
    if record.partials.len() != 1 {
        for (name, index) in [
            ("CARTESIAN_POINT", 0),
            ("CARTESIAN_POINT", 1),
            (point_type, 0),
            (point_type, 1),
        ] {
            values = named_parameter(ctx, record, name, index)?.and_then(Value::list);
            if values.is_some() {
                break;
            }
        }
    }
    let Some(values) = values else {
        return Ok(None);
    };
    if values.len() != 3 {
        return Ok(None);
    }
    Ok((|| {
        FinitePoint3::new(Point3::new(
            values[0].number()? * scale,
            values[1].number()? * scale,
            values[2].number()? * scale,
        ))
    })())
}

fn named_coordinates2(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    name: &str,
    index: usize,
) -> Result<Option<Point2>, CodecError> {
    let Some(values) = named_parameter(ctx, record, name, index)?.and_then(Value::list) else {
        return Ok(None);
    };
    if values.len() != 2 {
        return Ok(None);
    }
    Ok((|| {
        Some(Point2::new(values[0].number()?, values[1].number()?))
    })())
}

fn direction2(value: Option<&Value>) -> Option<HypotDirection2> {
    let values = value?.list()?;
    if values.len() != 2 {
        return None;
    }
    HypotDirection2::normalized_with_length([values[0].number()?, values[1].number()?])
        .map(|(direction, _)| direction)
}

fn vector3(value: Option<&Value>, scale: f64) -> Option<Vector3> {
    let values = value?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Vector3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

#[derive(Clone, Copy)]
enum DefaultNurbsKnotKind {
    Uniform,
    QuasiUniform,
    Bezier,
}

struct NurbsCurveDefinition {
    degree: u32,
    control_points: Vec<u64>,
    knots: KnotVector,
    weights: Option<Vec<f64>>,
    periodic: bool,
}

fn nurbs_curve_definition(
    id: u64,
    record: &RawRecord,
    losses: &mut Vec<LossNote>,
    periodicity_field: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<NurbsCurveDefinition>, CodecError> {
    let (base, offset) = if record.partials.len() > 1 {
        (geometry_or_none!(record.partial(ctx, "B_SPLINE_CURVE")?), 0)
    } else {
        let base = geometry_or_none!([
            "B_SPLINE_CURVE_WITH_KNOTS",
            "UNIFORM_CURVE",
            "QUASI_UNIFORM_CURVE",
            "BEZIER_CURVE",
        ]
        .into_iter()
        .map(|name| record.partial(ctx, name))
        .filter_map(Result::transpose)
        .next()
        .transpose()?);
        (base, 1)
    };
    let degree = geometry_or_none!(base
        .parameters
        .get(offset)
        .and_then(Value::integer)
        .and_then(|degree| u32::try_from(degree).ok()));
    let control_points = geometry_or_none!(references(
        geometry_or_none!(base.parameters.get(offset + 1)),
        ctx
    )?);
    if usize::try_from(degree)
        .ok()
        .is_none_or(|degree| degree >= control_points.len())
    {
        return Ok(None);
    }
    let periodic = geometry_or_none!(periodic_value(
        base.parameters.get(offset + 3),
        periodicity_field,
        id,
        losses,
        ctx
    )?);
    let degree_usize = geometry_or_none!(usize::try_from(degree).ok());
    let expected_knots = geometry_or_none!(control_points
        .len()
        .checked_add(degree_usize)
        .and_then(|count| count.checked_add(1)));
    let knots = if let Some(knot_leaf) = record.partial(ctx, "B_SPLINE_CURVE_WITH_KNOTS")? {
        let tail = geometry_or_none!(knot_leaf.parameters.len().checked_sub(3));
        geometry_or_none!(expand_knots(
            geometry_or_none!(knot_leaf.parameters.get(tail)),
            geometry_or_none!(knot_leaf.parameters.get(tail + 1)),
            expected_knots,
            ctx,
        )?)
    } else {
        let kind = if record.partial(ctx, "UNIFORM_CURVE")?.is_some() {
            DefaultNurbsKnotKind::Uniform
        } else if record.partial(ctx, "QUASI_UNIFORM_CURVE")?.is_some() {
            DefaultNurbsKnotKind::QuasiUniform
        } else if record.partial(ctx, "BEZIER_CURVE")?.is_some() {
            DefaultNurbsKnotKind::Bezier
        } else {
            return Ok(None);
        };
        geometry_or_none!(default_nurbs_knots(
            control_points.len(),
            degree,
            kind,
            ctx
        )?)
    };
    if knots.len() != expected_knots {
        return Ok(None);
    }
    let weights = if let Some(leaf) = record.partial(ctx, "RATIONAL_B_SPLINE_CURVE")? {
        Some(geometry_or_none!(numbers(
            geometry_or_none!(leaf.parameters.first()),
            ctx
        )?))
    } else {
        None
    };
    Ok(Some(NurbsCurveDefinition {
        degree,
        control_points,
        knots,
        weights,
        periodic,
    }))
}

fn default_nurbs_knots(
    control_point_count: usize,
    degree: u32,
    kind: DefaultNurbsKnotKind,
    ctx: &DecodeContext<'_>,
) -> Result<Option<KnotVector>, CodecError> {
    let degree = geometry_or_none!(usize::try_from(degree).ok());
    let expected = geometry_or_none!(control_point_count
        .checked_add(degree)
        .and_then(|count| count.checked_add(1)));
    let mut knots = Vec::new();
    match kind {
        DefaultNurbsKnotKind::Uniform => {
            for index in 0..expected {
                let index = geometry_or_none!(cadmpeg_core::convert::f64_from_index(index));
                let degree = geometry_or_none!(cadmpeg_core::convert::f64_from_index(degree));
                let knot = geometry_or_none!(FiniteReal::new(index - degree));
                ctx.push_vec(&mut knots, knot, "step_default_nurbs_knots")?;
            }
        }
        DefaultNurbsKnotKind::QuasiUniform => {
            let distinct_count = geometry_or_none!(control_point_count
                .checked_sub(degree)
                .and_then(|count| count.checked_add(1)));
            for index in 0..distinct_count {
                let multiplicity = if index == 0 || index + 1 == distinct_count {
                    geometry_or_none!(degree.checked_add(1))
                } else {
                    1
                };
                let knot = geometry_or_none!(FiniteReal::new(geometry_or_none!(
                    cadmpeg_core::convert::f64_from_index(index)
                )));
                for _ in 0..multiplicity {
                    ctx.push_vec(&mut knots, knot, "step_default_nurbs_knots")?;
                }
            }
        }
        DefaultNurbsKnotKind::Bezier => {
            if degree == 0 {
                return Ok(None);
            }
            let segment_count = geometry_or_none!(control_point_count.checked_sub(1));
            if segment_count % degree != 0 {
                return Ok(None);
            }
            let segment_count = segment_count / degree;
            let distinct_count = geometry_or_none!(segment_count.checked_add(1));
            for index in 0..distinct_count {
                let multiplicity = if index == 0 || index + 1 == distinct_count {
                    geometry_or_none!(degree.checked_add(1))
                } else {
                    degree
                };
                let knot = geometry_or_none!(FiniteReal::new(geometry_or_none!(
                    cadmpeg_core::convert::f64_from_index(index)
                )));
                for _ in 0..multiplicity {
                    ctx.push_vec(&mut knots, knot, "step_default_nurbs_knots")?;
                }
            }
        }
    }
    if knots.len() != expected {
        return Ok(None);
    }
    Ok(KnotVector::from_finite_lanes(ctx, knots)?.ok())
}

fn nurbs_curve(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, FinitePoint3>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<NurbsCurve>, CodecError> {
    let definition = geometry_or_none!(nurbs_curve_definition(
        id,
        record,
        losses,
        "B_SPLINE_CURVE",
        ctx
    )?);
    let mut control_points = Vec::new();
    for id in definition.control_points {
        let point = geometry_or_none!(points.get(&id).copied());
        ctx.push_vec(
            &mut control_points,
            point,
            "step_nurbs_curve_control_points",
        )?;
    }
    let curve = NurbsCurve::from_lanes(
        ctx,
        definition.degree,
        definition.knots,
        control_points,
        definition.weights,
        definition.periodic,
    )?;
    match curve {
        Ok(curve) => Ok(Some(curve)),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("B_SPLINE_CURVE #{id} is not a curve carrier: {error}"),
                    "STEP nurbs_curve text",
                )?),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

fn nurbs_pcurve(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PcurveGeometry>, CodecError> {
    let definition = geometry_or_none!(nurbs_curve_definition(
        id,
        record,
        losses,
        "B_SPLINE_CURVE pcurve",
        ctx
    )?);
    let mut control_points = Vec::new();
    for id in definition.control_points {
        let point = geometry_or_none!(points.get(&id).copied());
        ctx.push_vec(
            &mut control_points,
            point,
            "step_nurbs_pcurve_control_points",
        )?;
    }
    let pcurve = PcurveNurbs::from_lanes(
        ctx,
        definition.degree,
        definition.knots,
        control_points,
        definition.weights,
        definition.periodic,
    )?;
    match pcurve {
        Ok(nurbs) => Ok(Some(PcurveGeometry::Nurbs { nurbs })),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("B_SPLINE_CURVE pcurve #{id} is not a pcurve carrier: {error}"),
                    "STEP nurbs_pcurve text",
                )?),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

#[derive(Clone, Copy)]
struct PcurveSources<'a> {
    points: &'a BTreeMap<u64, Point2>,
    vectors: &'a BTreeMap<u64, Point2>,
    placements: &'a BTreeMap<u64, (Point2, HypotDirection2, HypotDirection2)>,
    transformations: &'a BTreeMap<u64, Transform2>,
    angle_scale: f64,
}

fn decode_pcurve_geometry(
    id: u64,
    exchange: &Exchange,
    sources: PcurveSources<'_>,
    losses: &mut Vec<LossNote>,
    active: &mut BTreeSet<u64>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(PcurveGeometry, BTreeSet<u64>)>, CodecError> {
    let PcurveSources {
        points,
        vectors,
        placements,
        transformations,
        angle_scale,
    } = sources;
    if depth >= 256 || active.contains(&id) {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("step_pcurve_geometry_walk")?;
    ctx.insert_btree_set(active, id, "step_pcurve_geometry_active")?;
    let result = (|| -> Result<Option<_>, CodecError> {
        let record = geometry_or_none!(exchange.records().get(&id));
        let mut records = BTreeSet::new();
        ctx.insert_btree_set(&mut records, id, "step_pcurve_source_records")?;
        let geometry = if ctx.any_by(
            &(record.partials)[..],
            |partial| {
                Ok(matches!(
                    partial.name.as_str(),
                    "B_SPLINE_CURVE_WITH_KNOTS"
                        | "UNIFORM_CURVE"
                        | "QUASI_UNIFORM_CURVE"
                        | "BEZIER_CURVE"
                ))
            },
            "STEP decode pcurve geometry traversal",
        )? {
            geometry_or_none!(nurbs_pcurve(id, record, points, losses, ctx)?)
        } else {
            let curve_type = geometry_or_none!(entity_type(
                ctx,
                record,
                &[
                    "LINE",
                    "CIRCLE",
                    "ELLIPSE",
                    "PARABOLA",
                    "HYPERBOLA",
                    "POLYLINE",
                    "CURVE_REPLICA",
                    "TRIMMED_CURVE",
                    "OFFSET_CURVE_2D",
                    "UNIFORM_CURVE",
                    "QUASI_UNIFORM_CURVE",
                    "BEZIER_CURVE",
                ],
            )?);
            match curve_type {
                "LINE" => {
                    let origin = geometry_or_none!(named_parameter(ctx, record, "LINE", 1)?
                        .and_then(Value::reference)
                        .and_then(|point| points.get(&point).copied()));
                    let direction = geometry_or_none!(named_parameter(ctx, record, "LINE", 2)?
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied()));
                    PcurveGeometry::Line(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(origin, direction).ok()
                    ))
                }
                "CIRCLE" => {
                    let placement = geometry_or_none!(
                        named_parameter(ctx, record, "CIRCLE", 1)?.and_then(Value::reference)
                    );
                    let (center, x_axis, y_axis) =
                        geometry_or_none!(placements.get(&placement).copied());
                    let radius = geometry_or_none!(
                        named_parameter(ctx, record, "CIRCLE", 2)?.and_then(Value::number)
                    );
                    ctx.insert_btree_set(&mut records, placement, "step_pcurve_source_records")?;
                    PcurveGeometry::Circle(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::CirclePcurve::from_parts(
                            geometry_or_none!(cadmpeg_ir::units::FinitePoint2::new(center)),
                            cadmpeg_ir::units::FinitePoint2::from(x_axis),
                            cadmpeg_ir::units::FinitePoint2::from(y_axis),
                            geometry_or_none!(PositiveReal::new(radius)),
                        )
                    ))
                }
                "ELLIPSE" => {
                    let placement = geometry_or_none!(
                        named_parameter(ctx, record, "ELLIPSE", 1)?.and_then(Value::reference)
                    );
                    let (center, x_axis, y_axis) =
                        geometry_or_none!(placements.get(&placement).copied());
                    let major_radius = geometry_or_none!(named_parameter(
                        ctx, record, "ELLIPSE", 2
                    )?
                    .and_then(Value::number));
                    let minor_radius = geometry_or_none!(named_parameter(
                        ctx, record, "ELLIPSE", 3
                    )?
                    .and_then(Value::number));
                    ctx.insert_btree_set(&mut records, placement, "step_pcurve_source_records")?;
                    PcurveGeometry::Ellipse(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::EllipsePcurve::from_parts(
                            geometry_or_none!(cadmpeg_ir::units::FinitePoint2::new(center)),
                            cadmpeg_ir::units::FinitePoint2::from(x_axis),
                            cadmpeg_ir::units::FinitePoint2::from(y_axis),
                            geometry_or_none!(PositiveReal::new(major_radius)),
                            geometry_or_none!(PositiveReal::new(minor_radius)),
                        )
                    ))
                }
                "PARABOLA" => {
                    let placement = geometry_or_none!(
                        named_parameter(ctx, record, "PARABOLA", 1)?.and_then(Value::reference)
                    );
                    let (vertex, x_axis, y_axis) =
                        geometry_or_none!(placements.get(&placement).copied());
                    let focal_distance = geometry_or_none!(named_parameter(
                        ctx, record, "PARABOLA", 2
                    )?
                    .and_then(Value::number));
                    ctx.insert_btree_set(&mut records, placement, "step_pcurve_source_records")?;
                    PcurveGeometry::Parabola(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::ParabolaPcurve::from_parts(
                            geometry_or_none!(cadmpeg_ir::units::FinitePoint2::new(vertex)),
                            cadmpeg_ir::units::FinitePoint2::from(x_axis),
                            cadmpeg_ir::units::FinitePoint2::from(y_axis),
                            geometry_or_none!(PositiveReal::new(focal_distance)),
                        )
                    ))
                }
                "HYPERBOLA" => {
                    let placement =
                        geometry_or_none!(named_parameter(ctx, record, "HYPERBOLA", 1)?
                            .and_then(Value::reference));
                    let (center, x_axis, y_axis) =
                        geometry_or_none!(placements.get(&placement).copied());
                    let major_radius =
                        geometry_or_none!(
                            named_parameter(ctx, record, "HYPERBOLA", 2)?.and_then(Value::number)
                        );
                    let minor_radius =
                        geometry_or_none!(
                            named_parameter(ctx, record, "HYPERBOLA", 3)?.and_then(Value::number)
                        );
                    ctx.insert_btree_set(&mut records, placement, "step_pcurve_source_records")?;
                    PcurveGeometry::Hyperbola(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::HyperbolaPcurve::from_parts(
                            geometry_or_none!(cadmpeg_ir::units::FinitePoint2::new(center)),
                            cadmpeg_ir::units::FinitePoint2::from(x_axis),
                            cadmpeg_ir::units::FinitePoint2::from(y_axis),
                            geometry_or_none!(PositiveReal::new(major_radius)),
                            geometry_or_none!(PositiveReal::new(minor_radius)),
                        )
                    ))
                }
                "POLYLINE" => geometry_or_none!(polyline_pcurve(id, record, points, losses, ctx)?),
                "CURVE_REPLICA" => {
                    let basis_id =
                        geometry_or_none!(named_parameter(ctx, record, "CURVE_REPLICA", 1)?
                            .and_then(Value::reference));
                    let operator_id =
                        geometry_or_none!(named_parameter(ctx, record, "CURVE_REPLICA", 2)?
                            .and_then(Value::reference));
                    let (basis, basis_records) = geometry_or_none!(decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        sources,
                        losses,
                        active,
                        depth + 1,
                        ctx,
                    )?);
                    let transform = geometry_or_none!(transformations.get(&operator_id).copied());
                    for record in basis_records {
                        ctx.insert_btree_set(&mut records, record, "step_pcurve_source_records")?;
                    }
                    ctx.insert_btree_set(&mut records, operator_id, "step_pcurve_source_records")?;
                    ctx.charge_collection_items(1, "step_pcurve_nested_geometry")?;
                    PcurveGeometry::Transformed(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
                            Box::new(basis),
                            transform
                        )
                        .ok()
                    ))
                }
                "TRIMMED_CURVE" => {
                    let basis_id =
                        geometry_or_none!(named_parameter(ctx, record, "TRIMMED_CURVE", 1)?
                            .and_then(Value::reference));
                    let sense =
                        geometry_or_none!(named_parameter(ctx, record, "TRIMMED_CURVE", 4)?
                            .and_then(Value::logical));
                    let (basis, basis_records) = geometry_or_none!(decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        sources,
                        losses,
                        active,
                        depth + 1,
                        ctx,
                    )?);
                    let scale = if matches!(
                        basis,
                        PcurveGeometry::Circle(_) | PcurveGeometry::Ellipse(_)
                    ) {
                        angle_scale
                    } else {
                        1.0
                    };
                    let start =
                        geometry_or_none!(named_parameter(ctx, record, "TRIMMED_CURVE", 2)?
                            .map(|value| pcurve_trim_parameter(ctx, value))
                            .transpose()?
                            .flatten())
                        .get()
                            * scale;
                    let end = geometry_or_none!(named_parameter(ctx, record, "TRIMMED_CURVE", 3)?
                        .map(|value| pcurve_trim_parameter(ctx, value))
                        .transpose()?
                        .flatten())
                    .get()
                        * scale;
                    for record in basis_records {
                        ctx.insert_btree_set(&mut records, record, "step_pcurve_source_records")?;
                    }
                    let (parameter_range, same_sense) =
                        trimmed_pcurve_parameterization(&basis, start, end, sense);
                    ctx.charge_collection_items(1, "step_pcurve_nested_geometry")?;
                    PcurveGeometry::Trimmed(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::TrimmedPcurve::try_new(
                            parameter_range,
                            same_sense,
                            Box::new(basis)
                        )
                        .ok()
                    ))
                }
                "OFFSET_CURVE_2D" => {
                    let basis_id =
                        geometry_or_none!(named_parameter(ctx, record, "OFFSET_CURVE_2D", 1)?
                            .and_then(Value::reference));
                    let distance =
                        geometry_or_none!(named_parameter(ctx, record, "OFFSET_CURVE_2D", 2)?
                            .and_then(Value::number)
                            .and_then(FiniteReal::new));
                    geometry_or_none!(named_parameter(ctx, record, "OFFSET_CURVE_2D", 3)?
                        .and_then(Value::logical));
                    let (basis, basis_records) = geometry_or_none!(decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        sources,
                        losses,
                        active,
                        depth + 1,
                        ctx,
                    )?);
                    for record in basis_records {
                        ctx.insert_btree_set(&mut records, record, "step_pcurve_source_records")?;
                    }
                    ctx.charge_collection_items(1, "step_pcurve_nested_geometry")?;
                    PcurveGeometry::Offset(geometry_or_none!(
                        cadmpeg_ir::geometry::pcurve::OffsetPcurve::from_finite_parts(
                            distance,
                            Box::new(basis)
                        )
                        .ok()
                    ))
                }
                _ => return Ok(None),
            }
        };
        Ok(Some((geometry, records)))
    })();
    active.remove(&id);
    result
}

fn pcurve_trim_parameter(
    ctx: &DecodeContext<'_>,
    value: &Value,
) -> Result<Option<FiniteReal>, CodecError> {
    fn bare_number(value: &Value) -> Option<f64> {
        match value {
            Value::Integer(value) => cadmpeg_core::convert::f64_from_i64(*value),
            Value::Real(value) => Some(value.get()),
            _ => None,
        }
    }
    let number = match value {
        Value::Integer(_) | Value::Real(_) => bare_number(value),
        Value::Typed(name, value) if name == "PARAMETER_VALUE" => bare_number(value),
        Value::List(values) => {
            let parameter = ctx.find_map(
                &values[..],
                |value| {
                    Ok(match value {
                        Value::Typed(name, value) if name == "PARAMETER_VALUE" => {
                            bare_number(value)
                        }
                        _ => None,
                    })
                },
                "STEP typed pcurve trim traversal",
            )?;
            if parameter.is_some() {
                parameter
            } else {
                ctx.find_map(
                    &values[..],
                    |value| {
                        Ok(match value {
                            Value::Integer(_) | Value::Real(_) => bare_number(value),
                            _ => None,
                        })
                    },
                    "STEP bare pcurve trim traversal",
                )?
            }
        }
        _ => None,
    };
    Ok(number.and_then(FiniteReal::new))
}

fn trimmed_pcurve_parameterization(
    geometry: &PcurveGeometry,
    start: f64,
    end: f64,
    sense: bool,
) -> ([f64; 2], bool) {
    let mut start = start;
    let mut end = end;
    // Closed STEP pcurves use cyclic parameter branches. Move the endpoint
    // that follows the declared traversal before projecting to an ordered
    // basis interval; non-closed malformed input still gets a valid interval.
    if let Some(domain) = pcurve_periodic_domain(geometry) {
        if sense && end < start {
            end = shift_periodic_parameter(end, domain);
        } else if !sense && start < end {
            start = shift_periodic_parameter(start, domain);
        }
    }
    let [from, to] = if sense { [start, end] } else { [end, start] };
    if from <= to {
        ([from, to], true)
    } else {
        ([to, from], false)
    }
}

fn pcurve_periodic_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Circle(_) | PcurveGeometry::Ellipse(_) | PcurveGeometry::Harmonic(_) => {
            Some([0.0, std::f64::consts::TAU])
        }
        PcurveGeometry::Nurbs { nurbs } if nurbs.periodic() => {
            pcurve_nurbs_parameter_domain(nurbs.degree(), nurbs.knots(), nurbs.pole_rows().count())
        }
        PcurveGeometry::PolarNurbs { nurbs } if nurbs.periodic() => {
            pcurve_nurbs_parameter_domain(nurbs.degree(), nurbs.knots(), nurbs.pole_rows().count())
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let basis = offset_pcurve.basis();
            pcurve_periodic_domain(basis)
        }
        PcurveGeometry::Transformed(placed) => pcurve_periodic_domain(placed.basis()),
        _ => None,
    }
}

fn pcurve_nurbs_parameter_domain(
    degree: u32,
    knots: &KnotVector,
    count: usize,
) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    let lower = *knots.get(degree)?;
    let upper = *knots.get(count)?;
    (upper > lower).then_some([lower, upper])
}

fn shift_periodic_parameter(value: f64, [lower, upper]: [f64; 2]) -> f64 {
    let period = upper - lower;
    if period.is_finite() {
        value + period
    } else {
        upper + (value - lower)
    }
}

fn procedural_surface_parameter_scales(
    ir: &CadIr,
    index: &SurfaceScaleIndex,
    surface_id: &SurfaceId,
    geometry: &SurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[f64; 2]>, CodecError> {
    let _depth = ctx.enter_nested("step_surface_parameter_scale_walk")?;
    let mut active_storage = ctx.reserve_scoped(0, "step surface scale traversal storage")?;
    let mut active = BTreeSet::new();
    let mut surface_id = surface_id;
    let mut geometry = geometry;
    loop {
        if ctx.contains_btree_set(&active, surface_id, "STEP active membership")? {
            return Ok(None);
        }
        let key = active_storage.with_storage(|| {
            ctx.format_retained(
                format_args!("{}", surface_id.as_str()),
                "step_surface_scale_active_id",
            )
        })?;
        let key = geometry_or_none!(SurfaceId::mint(key).ok());
        active_storage
            .with_storage(|| ctx.insert_btree_set(&mut active, key, "step_surface_scale_active"))?;
        let Some(solved) = geometry.solved() else {
            return Ok(None);
        };
        match surface_geometry_parameter_scales(
            ir,
            index,
            surface_id,
            solved,
            length_scale,
            angle_scale,
            source_curve_parameter_scales,
            ctx,
        )? {
            SurfaceScaleStep::Value(scales) => return Ok(scales),
            SurfaceScaleStep::Support(support) => {
                let Some(carrier) = index.surface(ir, ctx, support)? else {
                    return Ok(None);
                };
                surface_id = support;
                geometry = &carrier.geometry;
            }
        }
    }
}

/// Arena positions remain valid when the worklist appends model records.
struct SurfaceScaleIndex {
    surfaces: BTreeMap<String, usize>,
    owners: BTreeMap<String, Option<String>>,
    procedurals: BTreeMap<String, Vec<usize>>,
    owned_procedurals: BTreeMap<String, BTreeSet<usize>>,
}

impl SurfaceScaleIndex {
    fn build<'ctx>(
        ir: &CadIr,
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<(Self, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
        let mut workspace = ctx.reserve_scoped(0, "step surface scale index")?;
        let mut index = Self {
            surfaces: BTreeMap::new(),
            owners: BTreeMap::new(),
            procedurals: BTreeMap::new(),
            owned_procedurals: BTreeMap::new(),
        };
        workspace.with_storage(|| -> Result<(), CodecError> {
            for (position, surface) in ctx.admit_iter(
                &ir.model.surfaces[..], "step surface scale index",
            )?.enumerate() {
                index.add_surface(surface, position, ctx)?;
            }
            for (position, procedural) in ctx.admit_iter(
                &ir.model.procedural_surfaces[..], "step surface scale procedurals",
            )?.enumerate() {
                index.add_procedural(procedural, position, ctx)?;
            }
            Ok(())
        })?;
        Ok((index, workspace))
    }

    fn add_surface(
        &mut self,
        surface: &Surface,
        position: usize,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if !ctx.contains_key_btree_map(&self.surfaces, surface.id.as_str(), "step surface scale index")? {
            let key = ctx.format_retained(format_args!("{}", surface.id.as_str()), "step surface scale index")?;
            ctx.insert_btree_map(&mut self.surfaces, key, position, "step surface scale index")?;
        }
        let Some(construction) = surface.geometry.procedural_construction() else {
            return Ok(());
        };
        if let Some(owner) = ctx.get_mut_btree_map(
            &mut self.owners, construction.as_str(), "step surface scale owners",
        )? {
            if let Some(owner) = owner.take() {
                // A second owner invalidates every procedure for this construction.
                if let Some(procedurals) = ctx.get_btree_map(
                    &self.procedurals, construction.as_str(), "step surface scale procedurals",
                )? {
                    if let Some(owned) = ctx.get_mut_btree_map(
                        &mut self.owned_procedurals, owner.as_str(), "step surface scale owners",
                    )? {
                        for procedural in ctx.admit_iter(procedurals.as_slice(), "step surface scale owner invalidation")? {
                            ctx.remove_btree_set(owned, procedural, "step surface scale owner invalidation")?;
                        }
                    }
                }
            }
        } else {
            let key = ctx.format_retained(format_args!("{}", construction.as_str()), "step surface scale owners")?;
            let owner = ctx.format_retained(format_args!("{}", surface.id.as_str()), "step surface scale owners")?;
            ctx.insert_btree_map(&mut self.owners, key, Some(owner), "step surface scale owners")?;
            if let Some(procedurals) = ctx.get_btree_map(
                &self.procedurals, construction.as_str(), "step surface scale procedurals",
            )? {
                for &procedural in ctx.admit_iter(procedurals.as_slice(), "step surface scale owner association")? {
                    let owner = ctx.format_retained(format_args!("{}", surface.id.as_str()), "step surface scale owners")?;
                    ctx.insert_btree_group_set(&mut self.owned_procedurals, owner, procedural,
                        "step surface scale procedurals", "step surface scale procedure positions")?;
                }
            }
        }
        Ok(())
    }

    fn add_procedural(
        &mut self,
        procedural: &ProceduralSurface,
        position: usize,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        let key = ctx.format_retained(format_args!("{}", procedural.id.as_str()), "step surface scale procedurals")?;
        ctx.push_btree_group(&mut self.procedurals, key, position,
            "step surface scale procedurals", "step surface scale procedure positions")?;
        if let Some(Some(owner)) = ctx.get_btree_map(
            &self.owners, procedural.id.as_str(), "step surface scale owners",
        )? {
            let owner = ctx.format_retained(format_args!("{}", owner), "step surface scale procedurals")?;
            ctx.insert_btree_group_set(&mut self.owned_procedurals, owner, position,
                "step surface scale procedurals", "step surface scale procedure positions")?;
        }
        Ok(())
    }

    fn surface<'a>(
        &self,
        ir: &'a CadIr,
        ctx: &DecodeContext<'_>,
        id: &SurfaceId,
    ) -> Result<Option<&'a Surface>, CodecError> {
        Ok(ctx.get_btree_map(&self.surfaces, id.as_str(), "step surface scale lookup")?
            .and_then(|&position| ir.model.surfaces.get(position)))
    }

    fn owned_procedural<'a>(
        &self,
        ir: &'a CadIr,
        ctx: &DecodeContext<'_>,
        owner: &SurfaceId,
    ) -> Result<Option<&'a ProceduralSurface>, CodecError> {
        Ok(ctx.get_btree_map(&self.owned_procedurals, owner.as_str(), "step surface scale procedural lookup")?
            .filter(|positions| positions.len() == 1)
            .and_then(|positions| positions.first())
            .and_then(|&position| ir.model.procedural_surfaces.get(position)))
    }
}

enum SurfaceScaleStep<'a> {
    Value(Option<[f64; 2]>),
    Support(&'a SurfaceId),
}

fn surface_geometry_parameter_scales<'a>(
    ir: &'a CadIr,
    index: &SurfaceScaleIndex,
    surface_id: &SurfaceId,
    geometry: &SolvedSurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<SurfaceScaleStep<'a>, CodecError> {
    let _depth = ctx.enter_nested("step_surface_geometry_scale_walk")?;
    let mut geometry = geometry;
    while let SolvedSurfaceGeometry::Transformed(placed) = geometry {
        geometry = placed.basis();
    }
    let scales = match geometry {
        SolvedSurfaceGeometry::Plane(_) => Some([length_scale, length_scale]),
        SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Cone(_) => {
            Some([angle_scale, length_scale])
        }
        SolvedSurfaceGeometry::Sphere(_) | SolvedSurfaceGeometry::Torus(_) => {
            Some([angle_scale, angle_scale])
        }
        SolvedSurfaceGeometry::Nurbs(_) => Some([1.0, 1.0]),
        SolvedSurfaceGeometry::Transformed(_) => None,
        SolvedSurfaceGeometry::Unknown { .. } => {
            let Some(procedural) = index.owned_procedural(ir, ctx, surface_id)? else {
                return Ok(SurfaceScaleStep::Value(None));
            };
            if let Some(support) = procedural_surface_support(procedural.definition()) {
                return Ok(SurfaceScaleStep::Support(support));
            }
            return procedural_definition_parameter_scales(
                ir,
                procedural.definition(),
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                ctx,
            )
            .map(SurfaceScaleStep::Value);
        }
        SolvedSurfaceGeometry::Polygonal(_) => None,
    };
    Ok(SurfaceScaleStep::Value(scales))
}

fn procedural_surface_support(definition: &ProceduralSurfaceDefinition) -> Option<&SurfaceId> {
    match definition {
        ProceduralSurfaceDefinition::Offset(payload) => Some(payload.support()),
        ProceduralSurfaceDefinition::ParallelOffset(payload) => Some(payload.support()),
        ProceduralSurfaceDefinition::Subset(payload) => Some(payload.support()),
        ProceduralSurfaceDefinition::SubSurface(payload) => Some(payload.support()),
        ProceduralSurfaceDefinition::CurveBounded { support, .. } => Some(support),
        ProceduralSurfaceDefinition::Replica { source, .. } => Some(source),
        _ => None,
    }
}

fn procedural_definition_parameter_scales(
    ir: &CadIr,
    definition: &ProceduralSurfaceDefinition,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[f64; 2]>, CodecError> {
    match definition {
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
            let directrix = definition_payload.directrix();
            Ok(Some([
                geometry_or_none!(directrix_parameter_scale(
                    ir,
                    directrix,
                    length_scale,
                    angle_scale,
                    source_curve_parameter_scales,
                    ctx,
                )?),
                1.0,
            ]))
        }
        ProceduralSurfaceDefinition::LinearSweep(definition_payload) => Ok(Some([
            geometry_or_none!(directrix_parameter_scale(
                ir,
                definition_payload.directrix(),
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                ctx,
            )?),
            1.0,
        ])),
        ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => Ok(Some([
            angle_scale,
            geometry_or_none!(directrix_parameter_scale(
                ir,
                definition_payload.directrix(),
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                ctx,
            )?),
        ])),
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            let directrix = geometry_or_none!(directrix_parameter_scale(
                ir,
                definition_payload.directrix(),
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                ctx,
            )?);
            Ok(Some(if *definition_payload.transposed() {
                [angle_scale, directrix]
            } else {
                [directrix, angle_scale]
            }))
        }
        ProceduralSurfaceDefinition::DegenerateTorus { .. } => Ok(Some([angle_scale, angle_scale])),
        _ => Ok(None),
    }
}

fn directrix_parameter_scale(
    ir: &CadIr,
    curve_id: &CurveId,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, FiniteReal>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    if let Some(source_scale) = step_instance_id(ctx, curve_id.as_str())?
        .and_then(|id| source_curve_parameter_scales.get(&id))
    {
        return Ok(Some(source_scale.get()));
    }
    directrix_parameter_scale_inner(
        ir,
        curve_id,
        length_scale,
        angle_scale,
        &mut BTreeSet::new(),
        ctx,
    )
}

fn directrix_parameter_scale_inner(
    ir: &CadIr,
    curve_id: &CurveId,
    length_scale: f64,
    angle_scale: f64,
    active: &mut BTreeSet<CurveId>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    if ctx.contains_btree_set(active, curve_id, "STEP active membership")? {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("step_directrix_scale_walk")?;
    let key = ctx.format_retained(
        format_args!("{}", curve_id.as_str()),
        "step_directrix_scale_active_id",
    )?;
    let key = geometry_or_none!(CurveId::mint(key).ok());
    ctx.insert_btree_set(active, key, "step_directrix_scale_active")?;
    let scale = if let Some(curve) = ctx.find_map(
        &(ir.model.curves)[..],
        |curve| -> Result<Option<_>, CodecError> {
            Ok((ctx.equal(
                &curve.id,
                curve_id,
                "STEP directrix parameter scale inner equality",
            )?)
            .then_some(curve))
        },
        "STEP directrix parameter scale inner traversal",
    )? {
        if let Some(solved) = curve.geometry.solved() {
            directrix_geometry_parameter_scale(solved, length_scale, angle_scale, ctx)
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    };
    active.remove(curve_id);
    scale
}

fn directrix_geometry_parameter_scale(
    geometry: &SolvedCurveGeometry,
    length_scale: f64,
    angle_scale: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let _depth = ctx.enter_nested("step_directrix_geometry_scale_walk")?;
    match geometry {
        SolvedCurveGeometry::Line(_) => Ok(Some(length_scale)),
        SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_) => Ok(Some(angle_scale)),
        SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Nurbs(_)
        | SolvedCurveGeometry::Polyline(_) => Ok(Some(1.0)),
        SolvedCurveGeometry::Transformed(placed) => {
            directrix_geometry_parameter_scale(placed.basis(), length_scale, angle_scale, ctx)
        }
        SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => Ok(None),
    }
}

pub(super) fn surface_periodic_domains(geometry: &SolvedSurfaceGeometry) -> [Option<[f64; 2]>; 2] {
    match geometry {
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_) => [Some([0.0, std::f64::consts::TAU]), None],
        SolvedSurfaceGeometry::Torus(_) => [
            Some([0.0, std::f64::consts::TAU]),
            Some([0.0, std::f64::consts::TAU]),
        ],
        SolvedSurfaceGeometry::Nurbs(surface) => [
            surface
                .u_periodic()
                .then(|| {
                    nurbs_surface_parameter_domain(
                        surface.u_degree(),
                        surface.u_knots(),
                        surface.u_count(),
                    )
                })
                .flatten(),
            surface
                .v_periodic()
                .then(|| {
                    nurbs_surface_parameter_domain(
                        surface.v_degree(),
                        surface.v_knots(),
                        surface.v_count(),
                    )
                })
                .flatten(),
        ],
        SolvedSurfaceGeometry::Transformed(placed) => surface_periodic_domains(placed.basis()),
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => [None, None],
    }
}

fn nurbs_surface_parameter_domain(
    degree: u32,
    knots: &KnotVector,
    count: usize,
) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    let lower = *knots.get(degree)?;
    let upper = *knots.get(count)?;
    (upper > lower).then_some([lower, upper])
}

fn polyline_pcurve(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PcurveGeometry>, CodecError> {
    let values = geometry_or_none!(record.parameter(1).and_then(Value::list));
    let mut control_points = Vec::new();
    for value in ctx.admit_iter(&values[..], "STEP polyline pcurve value traversal")? {
        let point = geometry_or_none!(value.reference().and_then(|id| points.get(&id).copied()));
        ctx.push_vec(&mut control_points, point, "step_polyline_pcurve_points")?;
    }
    if control_points.len() < 2 {
        return Ok(None);
    }
    let last = geometry_or_none!(cadmpeg_core::convert::f64_from_index(
        control_points.len() - 1
    ));
    let mut knots = Vec::new();
    ctx.push_vec(&mut knots, 0.0, "step_polyline_pcurve_knots")?;
    for index in 0..control_points.len() {
        let knot = geometry_or_none!(cadmpeg_core::convert::f64_from_index(index));
        ctx.push_vec(&mut knots, knot, "step_polyline_pcurve_knots")?;
    }
    ctx.push_vec(&mut knots, last, "step_polyline_pcurve_knots")?;
    match PcurveNurbs::from_lanes(ctx, 1, knots, control_points, None, false)? {
        Ok(nurbs) => Ok(Some(PcurveGeometry::Nurbs { nurbs })),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("POLYLINE pcurve #{id} is not a pcurve carrier: {error}"),
                    "STEP polyline_pcurve text",
                )?),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

fn polyline(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, FinitePoint3>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<NurbsCurve>, CodecError> {
    let values = geometry_or_none!(record.parameter(1).and_then(Value::list));
    let mut control_points = Vec::new();
    for value in ctx.admit_iter(&values[..], "STEP polyline value traversal")? {
        let point = geometry_or_none!(value
            .reference()
            .and_then(|id| points.get(&id).copied().map(FinitePoint3::get)));
        ctx.push_vec(&mut control_points, point, "step_polyline_points")?;
    }
    if control_points.len() < 2 {
        return Ok(None);
    }
    let last = geometry_or_none!(cadmpeg_core::convert::f64_from_index(
        control_points.len() - 1
    ));
    let mut knots = Vec::new();
    ctx.push_vec(&mut knots, 0.0, "step_polyline_knots")?;
    for index in 0..control_points.len() {
        let knot = geometry_or_none!(cadmpeg_core::convert::f64_from_index(index));
        ctx.push_vec(&mut knots, knot, "step_polyline_knots")?;
    }
    ctx.push_vec(&mut knots, last, "step_polyline_knots")?;
    match cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        ctx,
        1,
        knots,
        control_points,
        None,
        false,
    )? {
        Ok(curve) => Ok(Some(curve)),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("POLYLINE #{id} is not a curve carrier: {error}"),
                    "STEP polyline text",
                )?),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

fn nurbs_surface(
    id: u64,
    record: &RawRecord,
    points: &BTreeMap<u64, FinitePoint3>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<NurbsSurface>, CodecError> {
    let (base, offset) = if record.partials.len() > 1 {
        (
            geometry_or_none!(record.partial(ctx, "B_SPLINE_SURFACE")?),
            0,
        )
    } else {
        let base = geometry_or_none!([
            "B_SPLINE_SURFACE_WITH_KNOTS",
            "UNIFORM_SURFACE",
            "QUASI_UNIFORM_SURFACE",
            "BEZIER_SURFACE",
        ]
        .into_iter()
        .map(|name| record.partial(ctx, name))
        .filter_map(Result::transpose)
        .next()
        .transpose()?);
        (base, 1)
    };
    let u_degree = geometry_or_none!(base
        .parameters
        .get(offset)
        .and_then(Value::integer)
        .and_then(|degree| u32::try_from(degree).ok()));
    let v_degree = geometry_or_none!(base
        .parameters
        .get(offset + 1)
        .and_then(Value::integer)
        .and_then(|degree| u32::try_from(degree).ok()));
    let rows = geometry_or_none!(base.parameters.get(offset + 2).and_then(Value::list));
    if !ctx.all_by(
        rows,
        |row| Ok(row.list().is_some()),
        "STEP nurbs surface traversal",
    )? {
        return Ok(None);
    }
    let u_count = geometry_or_none!(u32::try_from(rows.len()).ok());
    let v_count = geometry_or_none!(rows
        .first()
        .and_then(Value::list)
        .and_then(|row| u32::try_from(row.len()).ok()));
    if v_count == 0
        || u_degree >= u_count
        || v_degree >= v_count
        || ctx.any_by(
            rows,
            |row| {
                Ok(row
                    .list()
                    .is_none_or(|row| row.len() != cadmpeg_core::decode::index_from_u32(v_count)))
            },
            "STEP nurbs surface traversal",
        )?
    {
        return Ok(None);
    }
    let mut control_points = Vec::new();
    for row in ctx.admit_iter(rows, "STEP nurbs surface borrowed traversal")? {
        let mut decoded_row = Vec::new();
        for value in ctx.admit_iter(
            geometry_or_none!(row.list()),
            "STEP nurbs surface borrowed traversal",
        )? {
            let point = geometry_or_none!(value
                .reference()
                .and_then(|id| points.get(&id).copied().map(FinitePoint3::get)));
            ctx.push_vec(&mut decoded_row, point, "step_nurbs_surface_control_points")?;
        }
        ctx.push_vec(&mut control_points, decoded_row, "step_nurbs_surface_rows")?;
    }
    let surface_name = entity_type(
        ctx,
        record,
        &[
            "B_SPLINE_SURFACE_WITH_KNOTS",
            "UNIFORM_SURFACE",
            "QUASI_UNIFORM_SURFACE",
            "BEZIER_SURFACE",
        ],
    )?
    .unwrap_or("B_SPLINE_SURFACE");
    let (u_label, _u_label_storage) = ctx.format_scoped(
        format_args!("{surface_name} U direction"),
        "STEP surface periodicity label",
    )?;
    let u_periodic = geometry_or_none!(periodic_value(
        base.parameters.get(offset + 4),
        &u_label,
        id,
        losses,
        ctx,
    )?);
    let (v_label, _v_label_storage) = ctx.format_scoped(
        format_args!("{surface_name} V direction"),
        "STEP surface periodicity label",
    )?;
    let v_periodic = geometry_or_none!(periodic_value(
        base.parameters.get(offset + 5),
        &v_label,
        id,
        losses,
        ctx,
    )?);
    let expected_u = geometry_or_none!(usize::try_from(u_count)
        .ok()
        .and_then(|count| usize::try_from(u_degree)
            .ok()
            .and_then(|degree| count.checked_add(degree)))
        .and_then(|count| count.checked_add(1)));
    let expected_v = geometry_or_none!(usize::try_from(v_count)
        .ok()
        .and_then(|count| usize::try_from(v_degree)
            .ok()
            .and_then(|degree| count.checked_add(degree)))
        .and_then(|count| count.checked_add(1)));
    let (u_knots, v_knots) =
        if let Some(knot_leaf) = record.partial(ctx, "B_SPLINE_SURFACE_WITH_KNOTS")? {
            let tail = geometry_or_none!(knot_leaf.parameters.len().checked_sub(5));
            (
                geometry_or_none!(expand_knots(
                    geometry_or_none!(knot_leaf.parameters.get(tail)),
                    geometry_or_none!(knot_leaf.parameters.get(tail + 2)),
                    expected_u,
                    ctx,
                )?),
                geometry_or_none!(expand_knots(
                    geometry_or_none!(knot_leaf.parameters.get(tail + 1)),
                    geometry_or_none!(knot_leaf.parameters.get(tail + 3)),
                    expected_v,
                    ctx,
                )?),
            )
        } else {
            let kind = if record.partial(ctx, "UNIFORM_SURFACE")?.is_some() {
                DefaultNurbsKnotKind::Uniform
            } else if record.partial(ctx, "QUASI_UNIFORM_SURFACE")?.is_some() {
                DefaultNurbsKnotKind::QuasiUniform
            } else if record.partial(ctx, "BEZIER_SURFACE")?.is_some() {
                DefaultNurbsKnotKind::Bezier
            } else {
                return Ok(None);
            };
            (
                geometry_or_none!(default_nurbs_knots(
                    geometry_or_none!(usize::try_from(u_count).ok()),
                    u_degree,
                    kind,
                    ctx
                )?),
                geometry_or_none!(default_nurbs_knots(
                    geometry_or_none!(usize::try_from(v_count).ok()),
                    v_degree,
                    kind,
                    ctx
                )?),
            )
        };
    if u_knots.len() != expected_u || v_knots.len() != expected_v {
        return Ok(None);
    }
    let weights = if let Some(leaf) = record.partial(ctx, "RATIONAL_B_SPLINE_SURFACE")? {
        let rows = geometry_or_none!(leaf.parameters.first().and_then(Value::list));
        let mut values = Vec::new();
        for row in ctx.admit_iter(rows, "STEP nurbs surface borrowed traversal")? {
            let mut decoded_row = Vec::new();
            for value in ctx.admit_iter(
                geometry_or_none!(row.list()),
                "STEP nurbs surface borrowed traversal",
            )? {
                let number = geometry_or_none!(value.number());
                ctx.push_vec(&mut decoded_row, number, "step_nurbs_surface_weight_values")?;
            }
            ctx.push_vec(&mut values, decoded_row, "step_nurbs_surface_weight_rows")?;
        }
        Some(values)
    } else {
        None
    };
    let surface = NurbsSurface::from_lanes(
        ctx,
        NurbsSurfaceAxis::new(u_degree, u_knots, u_periodic),
        NurbsSurfaceAxis::new(v_degree, v_knots, v_periodic),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_points, weights),
        false,
    )?;
    match surface {
        Ok(surface) => Ok(Some(surface)),
        Err(error) => {
            ctx.push_vec(
                losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("B_SPLINE_SURFACE #{id} is not a surface carrier: {error}"),
                    "STEP nurbs_surface text",
                )?),
                "step_geometry_losses",
            )?;
            Ok(None)
        }
    }
}

fn expand_knots(
    multiplicities: &Value,
    distinct: &Value,
    expected: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<KnotVector>, CodecError> {
    let multiplicities = geometry_or_none!(multiplicities.list());
    let distinct = geometry_or_none!(distinct.list());
    if multiplicities.len() != distinct.len() {
        return Ok(None);
    }
    let mut knots = Vec::new();
    for (multiplicity, knot) in ctx
        .admit_iter(&multiplicities[..], "STEP expand knots traversal")?
        .zip(ctx.admit_iter(distinct, "STEP distinct knot traversal")?)
    {
        let count = geometry_or_none!(multiplicity
            .integer()
            .and_then(|count| usize::try_from(count).ok()));
        let knot = geometry_or_none!(knot.number().and_then(FiniteReal::new));
        if count == 0
            || knots
                .len()
                .checked_add(count)
                .is_none_or(|len| len > expected)
        {
            return Ok(None);
        }
        ctx.reserve_vec(&mut knots, count, "step_expanded_nurbs_knots")?;
        knots.extend(std::iter::repeat_with(|| knot).take(count));
    }
    Ok(KnotVector::from_finite_lanes(ctx, knots)?.ok())
}

fn references(value: &Value, ctx: &DecodeContext<'_>) -> Result<Option<Vec<u64>>, CodecError> {
    let values = geometry_or_none!(value.list());
    let mut references = Vec::new();
    for value in ctx.admit_iter(&values[..], "STEP references value traversal")? {
        let id = geometry_or_none!(value.reference());
        ctx.push_vec(&mut references, id, "step_nurbs_control_point_ids")?;
    }
    Ok(Some(references))
}

pub(super) fn curve_carrier_record(
    ctx: &DecodeContext<'_>,
    id: u64,
    exchange: &Exchange,
) -> Result<Option<u64>, CodecError> {
    let Some(record) = exchange.records().get(&id) else {
        return Ok(None);
    };
    if ctx.any_by(
        &record.partials[..],
        |partial| {
            Ok(matches!(
                partial.name.as_str(),
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
            ))
        },
        "STEP curve carrier partial traversal",
    )? {
        Ok(surface_curve_basis(ctx, record)?)
    } else {
        Ok(Some(id))
    }
}

fn numbers(value: &Value, ctx: &DecodeContext<'_>) -> Result<Option<Vec<f64>>, CodecError> {
    let values = geometry_or_none!(value.list());
    let mut numbers = Vec::new();
    for value in ctx.admit_iter(&values[..], "STEP numbers value traversal")? {
        let number = geometry_or_none!(value.number());
        ctx.push_vec(&mut numbers, number, "step_nurbs_weight_values")?;
    }
    Ok(Some(numbers))
}

pub(super) fn normalize(vector: Vector3) -> Option<UnitVector3> {
    if !vector.is_finite() {
        return None;
    }
    let scale = vector.x.abs().max(vector.y.abs()).max(vector.z.abs());
    if scale == 0.0 {
        return None;
    }
    let scaled = Vector3::new(vector.x / scale, vector.y / scale, vector.z / scale);
    UnitVector3::normalized_by_reciprocal(scaled)
}

fn project_axis(vector: UnitVector3, normal: UnitVector3) -> Option<UnitVector3> {
    let vector = *vector.recharted_by_reciprocal().as_raw();
    let normal = *normal.recharted_by_reciprocal().as_raw();
    normalize(vector - normal.scale(vector.dot(normal)))
}

fn second_project_axis(
    z_axis: UnitVector3,
    x_axis: UnitVector3,
    vector: UnitVector3,
) -> Option<UnitVector3> {
    let vector = *vector.recharted_by_reciprocal().as_raw();
    let z_axis = *z_axis.recharted_by_reciprocal().as_raw();
    let x_axis = *x_axis.recharted_by_reciprocal().as_raw();
    let projected = (vector - z_axis.scale(vector.dot(z_axis))) - x_axis.scale(vector.dot(x_axis));
    normalize(projected)
}

fn base_axis_3d(
    axis1: Option<UnitVector3>,
    axis2: Option<UnitVector3>,
    axis3: Option<UnitVector3>,
) -> Option<[UnitVector3; 3]> {
    let z_axis = axis3
        .unwrap_or(UnitVector3::Z_AXIS)
        .recharted_by_reciprocal();
    let default_x = default_reference_axis(z_axis);
    let x_axis = project_axis(axis1.unwrap_or(default_x), z_axis)?;
    let y_axis = second_project_axis(z_axis, x_axis, axis2.unwrap_or(UnitVector3::Y_AXIS))?;
    Some([x_axis, y_axis, z_axis])
}

const AXIS_PARALLEL_TOLERANCE: f64 = EPS_GEOMETRY_READ_EXACT_GEOMETRY;

fn default_reference_axis(axis: UnitVector3) -> UnitVector3 {
    if axis.as_raw().x.abs() >= 1.0 - AXIS_PARALLEL_TOLERANCE {
        UnitVector3::Y_AXIS
    } else {
        UnitVector3::X_AXIS
    }
}

fn transformation_direction<T: Copy>(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    name: &str,
    index: usize,
    directions: &BTreeMap<u64, T>,
) -> Result<Result<Option<T>, ()>, CodecError> {
    Ok(match transformation_parameter(ctx, record, name, index)? {
        Some(Value::Omitted | Value::Derived) => Ok(None),
        Some(Value::Reference(id)) => directions.get(id).copied().map(Some).ok_or(()),
        _ => Err(()),
    })
}

fn cartesian_transformation_operator(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    points: &BTreeMap<u64, FinitePoint3>,
    directions: &BTreeMap<u64, UnitVector3>,
) -> Result<Option<Transform>, CodecError> {
    let Ok(axis1) = transformation_direction(
        ctx,
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        0,
        directions,
    )?
    else {
        return Ok(None);
    };
    let Ok(axis2) = transformation_direction(
        ctx,
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        1,
        directions,
    )?
    else {
        return Ok(None);
    };
    let Some(origin) =
        transformation_parameter(ctx, record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 2)?
            .and_then(ValueExt::reference)
            .and_then(|id| points.get(&id).copied().map(FinitePoint3::get))
    else {
        return Ok(None);
    };
    let scale =
        match transformation_parameter(ctx, record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 3)? {
            Some(Value::Omitted | Value::Derived) | None => 1.0,
            Some(value) => {
                let Some(scale) = value.number() else {
                    return Ok(None);
                };
                scale
            }
        };
    let Some(scale) = PositiveReal::new(scale) else {
        return Ok(None);
    };
    let Ok(axis3) = transformation_direction(
        ctx,
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        4,
        directions,
    )?
    else {
        return Ok(None);
    };
    let Some([axis_x, axis_y, axis_z]) = base_axis_3d(axis1, axis2, axis3) else {
        return Ok(None);
    };
    let axis_x = axis_x.as_raw();
    let axis_y = axis_y.as_raw();
    let axis_z = axis_z.as_raw();
    Ok(Transform::affine([
        [
            axis_x.x * scale.get(),
            axis_y.x * scale.get(),
            axis_z.x * scale.get(),
            origin.x,
        ],
        [
            axis_x.y * scale.get(),
            axis_y.y * scale.get(),
            axis_z.y * scale.get(),
            origin.y,
        ],
        [
            axis_x.z * scale.get(),
            axis_y.z * scale.get(),
            axis_z.z * scale.get(),
            origin.z,
        ],
    ]))
}

fn cartesian_transformation_operator_2d(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    directions: &BTreeMap<u64, HypotDirection2>,
) -> Result<Option<Transform2>, CodecError> {
    let Ok(axis1) = transformation_direction(
        ctx,
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        0,
        directions,
    )?
    else {
        return Ok(None);
    };
    let Ok(axis2) = transformation_direction(
        ctx,
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        1,
        directions,
    )?
    else {
        return Ok(None);
    };
    let (axis1, axis2) = base_axis_2d(axis1, axis2);
    let Some(origin) =
        transformation_parameter(ctx, record, "CARTESIAN_TRANSFORMATION_OPERATOR_2D", 2)?
            .and_then(ValueExt::reference)
            .and_then(|id| points.get(&id).copied())
    else {
        return Ok(None);
    };
    let scale =
        match transformation_parameter(ctx, record, "CARTESIAN_TRANSFORMATION_OPERATOR_2D", 3)? {
            Some(Value::Omitted | Value::Derived) | None => 1.0,
            Some(value) => {
                let Some(scale) = value.number() else {
                    return Ok(None);
                };
                scale
            }
        };
    let Some(scale) = PositiveReal::new(scale) else {
        return Ok(None);
    };
    let [axis1_u, axis1_v] = axis1.get();
    let [axis2_u, axis2_v] = axis2.get();
    Ok(Transform2::affine([
        [axis1_u * scale.get(), axis2_u * scale.get(), origin.u],
        [axis1_v * scale.get(), axis2_v * scale.get(), origin.v],
    ]))
}

fn base_axis_2d(
    axis1: Option<HypotDirection2>,
    axis2: Option<HypotDirection2>,
) -> (HypotDirection2, HypotDirection2) {
    match (axis1, axis2) {
        (Some(axis1), axis2) => {
            let axis1 = axis1.recharted_by_hypot();
            let mut perpendicular = axis1.quarter_turn();
            if let Some(axis2) = axis2 {
                let [axis2_u, axis2_v] = axis2.recharted_by_hypot().get();
                let [perpendicular_u, perpendicular_v] = perpendicular.get();
                if axis2_u * perpendicular_u + axis2_v * perpendicular_v < 0.0 {
                    perpendicular = axis1.reverse_quarter_turn();
                }
            }
            (axis1, perpendicular)
        }
        (None, Some(axis2)) => {
            let axis2 = axis2.recharted_by_hypot();
            (axis2.reverse_quarter_turn(), axis2)
        }
        (None, None) => (HypotDirection2::X_AXIS, HypotDirection2::Y_AXIS),
    }
}

fn optional_direction(
    value: Option<&Value>,
    directions: &BTreeMap<u64, UnitVector3>,
) -> Option<UnitVector3> {
    match value? {
        Value::Omitted => None,
        Value::Reference(id) => directions.get(id).copied(),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests;
