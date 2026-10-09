// SPDX-License-Identifier: Apache-2.0
//! Offset curve entity projection.

use super::curve_conversion::angularly_equal;
use super::geometry::{
    declared_unit_vector, resolve_transform, source_object, WireProjectionOutcome,
};
use super::admit_with_scoped_loss_slots;
use crate::directory::{entry_by_sequence, DirectoryEntry};
use crate::global::ProjectedGlobal;
use crate::parameter::{record_by_sequence, ParameterRecord, TokenValue};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoles3},
    Curve, CurveGeometry, CurveOffsetDistanceLaw, CurveOffsetLawBasis, ProceduralCurve,
    ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{CurveId, PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use cadmpeg_ir::topology::{Edge, IncreasingParameterInterval, Point, Vertex};
use cadmpeg_ir::units::{FiniteVector, OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

const EPS_OFFSET_FRAME: f64 = 1.0e-10;

fn admit_offset_controls(
    ctx: &DecodeContext<'_>,
    _control_storage: ScopedReservation<'_>,
    controls: Vec<Point3>,
    operation: &'static str,
) -> Result<Option<Vec<FinitePoint3>>, CodecError> {
    let mut admitted = ctx.collection_vec(controls.len(), operation)?;
    let mut controls = controls.into_iter();
    while controls.len() != 0 || ctx.resource_refusal().is_some() {
        let Some(point) = ctx.next_charged(&mut controls, "iges offset control admission")? else { break; };
        let Some(point) = FinitePoint3::new(point) else {
            return Ok(None);
        };
        admitted.push(point);
    }
    Ok(Some(admitted))
}

fn transform_orientation(transform: cadmpeg_ir::transform::Transform) -> Option<f64> {
    let x = transform.apply_vector(Vector3::new(1.0, 0.0, 0.0))?.get();
    let y = transform.apply_vector(Vector3::new(0.0, 1.0, 0.0))?.get();
    let z = transform.apply_vector(Vector3::new(0.0, 0.0, 1.0))?.get();
    let determinant = x.cross(y).dot(z);
    (determinant.is_finite() && determinant != 0.0).then_some(determinant.signum())
}

fn placed_offset_normal(
    normal: UnitVector3,
    transform: cadmpeg_ir::transform::Transform,
) -> Option<UnitVector3> {
    let orientation = transform_orientation(transform)?;
    UnitVector3::normalized_by_reciprocal(
        transform.apply_vector(*normal.as_raw())?.scale(orientation),
    )
}

fn placed_offset_source(
    geometry: &SolvedCurveGeometry,
    transform: cadmpeg_ir::transform::Transform,
) -> Option<SolvedCurveGeometry> {
    let orientation = transform_orientation(transform)?;
    match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let direction = *line_curve.direction().as_raw();
            Some(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    transform.apply_point(line_curve.origin().get())?,
                    UnitVector3::normalized_by_reciprocal(
                        transform.apply_vector(direction)?.get(),
                    )?,
                ),
            ))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = transform.apply_point(circle_curve.center().get())?;
            let axis = UnitVector3::normalized_by_reciprocal(
                transform
                    .apply_vector(*circle_curve.frame().axis().as_raw())?
                    .get(),
            )?;
            let axis = if orientation < 0.0 {
                axis.reversed()
            } else {
                axis
            };
            let reference = UnitVector3::normalized_by_reciprocal(
                transform
                    .apply_vector(*circle_curve.frame().reference().as_raw())?
                    .get(),
            )?;
            let frame = OrthonormalFrame3::from_units(axis, reference)?;
            Some(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    center,
                    frame,
                    circle_curve.radius(),
                ),
            ))
        }
        _ => None,
    }
}

fn coordinate(point: Point3, index: u8) -> Option<f64> {
    match index {
        1 => Some(point.x),
        2 => Some(point.y),
        3 => Some(point.z),
        _ => None,
    }
}

fn greville(
    knots: &[f64],
    degree: usize,
    control: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<f64>, CodecError> {
    let Some(values) = knots.get(control + 1..=control + degree) else {
        return Ok(None);
    };
    let Some(divisor) = cadmpeg_core::convert::f64_from_index(degree) else {
        return Ok(None);
    };
    Ok(Some(
        ctx.fold(
            values,
            -0.0,
            |sum, value| Ok(sum + value),
            "iges offset Greville knots",
        )? / divisor,
    ))
}

fn omitted_or_integer_zero(record: &ParameterRecord, index: usize) -> bool {
    matches!(
        record.value(index),
        Some(TokenValue::Omitted | TokenValue::Integer(0))
    )
}

fn omitted_or_numeric_zero(record: &ParameterRecord, index: usize) -> bool {
    matches!(
        record.value(index),
        Some(TokenValue::Omitted | TokenValue::Integer(0))
    ) || matches!(record.value(index), Some(TokenValue::Real(value)) if value.get() == 0.0)
}

#[derive(Clone, Copy)]
struct SourceParameterMap {
    native: IncreasingParameterInterval,
    neutral: IncreasingParameterInterval,
    wide_coefficients: Option<(FiniteReal, FiniteReal)>,
}

impl SourceParameterMap {
    fn new(native: IncreasingParameterInterval, neutral: IncreasingParameterInterval) -> Self {
        let wide_coefficients = if (native.upper() - native.lower()).is_finite()
            && (neutral.upper() - neutral.lower()).is_finite()
        {
            None
        } else {
            native.affine_coefficients_to(neutral)
        };
        Self {
            native,
            neutral,
            wide_coefficients,
        }
    }

    fn scale(self) -> f64 {
        self.wide_coefficients.map_or_else(
            || {
                (self.neutral.upper() - self.neutral.lower())
                    / (self.native.upper() - self.native.lower())
            },
            |(scale, _)| scale.get(),
        )
    }

    fn to_neutral(self, value: f64) -> f64 {
        self.wide_coefficients.map_or_else(
            || self.neutral.lower() + (value - self.native.lower()) * self.scale(),
            |(scale, offset)| scale.get().mul_add(value, offset.get()),
        )
    }
}

fn source_parameter_map(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    neutral: FiniteVector<2>,
) -> Option<SourceParameterMap> {
    let native = match (entry.entity_type, entry.form) {
        (100, 0) => {
            let center = [record.number(2)?, record.number(3)?];
            let start = [record.number(4)?, record.number(5)?];
            let end = [record.number(6)?, record.number(7)?];
            let start_parameter = (start[1] - center[1])
                .atan2(start[0] - center[0])
                .rem_euclid(std::f64::consts::TAU);
            let end_parameter = (end[1] - center[1])
                .atan2(end[0] - center[0])
                .rem_euclid(std::f64::consts::TAU);
            let mut sweep = (end_parameter - start_parameter).rem_euclid(std::f64::consts::TAU);
            if angularly_equal(sweep, 0.0) {
                sweep = std::f64::consts::TAU;
            }
            [start_parameter, start_parameter + sweep]
        }
        (110, 0) => [0.0, 1.0],
        (130, 0) => [record.number(13)?, record.number(14)?],
        // These entities retain their IGES native parameter values in the
        // neutral edge range. Their domains are bounded by the entity data:
        // Type 102 starts at zero, Type 106 linear paths use one unit
        // interval per segment, and Types 112 and 126 carry their active
        // parameter bounds explicitly. Type 104 is not listed because the
        // neutral hyperbola carrier uses a different analytic parameter than
        // the IGES secant/tangent parameter and cannot use an affine map.
        (102 | 112, 0) | (106, 11..=13 | 63) | (126, 0..=5) => neutral.get(),
        _ => return None,
    };
    Some(SourceParameterMap::new(
        IncreasingParameterInterval::new(native)?,
        IncreasingParameterInterval::from_finite_endpoints(neutral)?,
    ))
}

#[derive(Default)]
struct OffsetSourceIndex {
    curves: BTreeMap<CurveId, usize>,
    points: BTreeMap<PointId, usize>,
    vertices: BTreeMap<VertexId, usize>,
    edges: BTreeMap<CurveId, Vec<usize>>,
    curve_count: usize,
    point_count: usize,
    vertex_count: usize,
    edge_count: usize,
}

impl OffsetSourceIndex {
    fn update(
        &mut self,
        ir: &CadIr,
        ctx: &DecodeContext<'_>,
        storage: &mut ScopedReservation<'_>,
    ) -> Result<(), CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(CodecError::from(refusal));
        }
        let mut source_index_entries = ir.model.curves[self.curve_count..].iter().enumerate();
        while source_index_entries.len() != 0 {
            let Some((offset, curve)) =
                ctx.next_charged(&mut source_index_entries, "iges offset source curve scan")?
            else {
                break;
            };
            if !ctx.contains_key_btree_map(
                &self.curves,
                &curve.id,
                "iges offset source curve query",
            )? {
                let key = storage.with_storage(|| {
                    curve
                        .id
                        .try_clone_for_decode(ctx, "iges offset source curve key")
                })?;
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut self.curves,
                        key,
                        self.curve_count + offset,
                        "iges offset source curve index",
                    )
                })?;
            }
        }
        self.curve_count = ir.model.curves.len();
        let mut source_index_entries = ir.model.points[self.point_count..].iter().enumerate();
        while source_index_entries.len() != 0 {
            let Some((offset, point)) =
                ctx.next_charged(&mut source_index_entries, "iges offset source point scan")?
            else {
                break;
            };
            if !ctx.contains_key_btree_map(
                &self.points,
                &point.id,
                "iges offset source point query",
            )? {
                let key = storage.with_storage(|| {
                    point
                        .id
                        .try_clone_for_decode(ctx, "iges offset source point key")
                })?;
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut self.points,
                        key,
                        self.point_count + offset,
                        "iges offset source point index",
                    )
                })?;
            }
        }
        self.point_count = ir.model.points.len();
        let mut source_index_entries = ir.model.vertices[self.vertex_count..].iter().enumerate();
        while source_index_entries.len() != 0 {
            let Some((offset, vertex)) =
                ctx.next_charged(&mut source_index_entries, "iges offset source vertex scan")?
            else {
                break;
            };
            if !ctx.contains_key_btree_map(
                &self.vertices,
                &vertex.id,
                "iges offset source vertex query",
            )? {
                let key = storage.with_storage(|| {
                    vertex
                        .id
                        .try_clone_for_decode(ctx, "iges offset source vertex key")
                })?;
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut self.vertices,
                        key,
                        self.vertex_count + offset,
                        "iges offset source vertex index",
                    )
                })?;
            }
        }
        self.vertex_count = ir.model.vertices.len();
        let mut source_index_entries = ir.model.edges[self.edge_count..].iter().enumerate();
        while source_index_entries.len() != 0 {
            let Some((offset, edge)) =
                ctx.next_charged(&mut source_index_entries, "iges offset source edge scan")?
            else {
                break;
            };
            let Some(curve) = edge.curve() else {
                continue;
            };
            let position = self.edge_count + offset;
            if let Some(group) =
                ctx.get_mut_btree_map(&mut self.edges, curve, "iges offset source edge query")?
            {
                storage.with_storage(|| {
                    ctx.push_vec(group, position, "iges offset source edge references")
                })?;
            } else {
                let key = storage.with_storage(|| {
                    curve.try_clone_for_decode(ctx, "iges offset source edge key")
                })?;
                let mut group = storage
                    .with_storage(|| ctx.collection_vec(1, "iges offset source edge references"))?;
                group.push(position);
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut self.edges,
                        key,
                        group,
                        "iges offset source edge index",
                    )
                })?;
            }
        }
        self.edge_count = ir.model.edges.len();
        Ok(())
    }
}

fn source_parameter_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    index: &OffsetSourceIndex,
    source_id: &CurveId,
    geometry: &SolvedCurveGeometry,
    tolerance: f64,
) -> Result<Option<FiniteVector<2>>, CodecError> {
    let point_position = |vertex: &VertexId| -> Result<Option<Point3>, CodecError> {
        let Some(position) =
            ctx.get_btree_map(&index.vertices, vertex, "iges offset source vertex query")?
        else {
            return Ok(None);
        };
        let point_id = &ir.model.vertices[*position].point;
        Ok(ctx
            .get_btree_map(&index.points, point_id, "iges offset source point query")?
            .and_then(|position| ir.model.points.get(*position))
            .map(|point| point.position().get()))
    };
    let mut chosen = None;
    let Some(edges) =
        ctx.get_btree_map(&index.edges, source_id, "iges offset source edge query")?
    else {
        return Ok(None);
    };
    let disagreement = ctx.any_by(
        edges,
        |position| {
            let edge = &ir.model.edges[*position];
            let (Some(range), Some(start), Some(end)) = (
                edge.param_range(),
                point_position(&edge.start)?,
                point_position(&edge.end)?,
            ) else {
                return Ok(false);
            };
            let Some(evaluated_start) =
                finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point_solved(ctx, geometry, range[0]),
                )?)?
            else {
                return Ok(false);
            };
            let Some(evaluated_end) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::curve_point_solved(ctx, geometry, range[1]),
            )?)?
            else {
                return Ok(false);
            };
            if evaluated_start.distance(start) > tolerance
                || evaluated_end.distance(end) > tolerance
            {
                return Ok(false);
            }
            match chosen {
                Some(previous) if previous != range => return Ok(true),
                None => chosen = Some(range),
                _ => {}
            }
            Ok(false)
        },
        "iges offset source range traversal",
    )?;
    if disagreement {
        return Ok(None);
    }
    Ok(chosen)
}

pub(super) fn project<'ctx>(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences<'_>,
) -> Result<WireProjectionOutcome<'ctx>, CodecError> {
    let mut transform_storage = ctx.reserve_scoped(0, "iges offsets transform lookup")?;
    let mut transform_tables = None;
    let mut index_storage = ctx.reserve_scoped(0, "iges offsets source lookup")?;
    let mut index = OffsetSourceIndex::default();
    let mut decoded_storage = ctx.reserve_scoped(0, "iges offsets decoded sequences")?;
    let mut decoded = BTreeSet::new();
    let mut loss_slots_storage = ctx.reserve_scoped(0, "iges entity loss slots")?;
    let mut losses = Vec::new();
    let mut wire_slots_storage = ctx.reserve_scoped(0, "iges offsets wire slots")?;
    let mut wire_edges = Vec::new();

    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges offsets directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 130 && entry.form == 0) {
            continue;
        }
        let factor = global.length_factor_mm();
        let Some(record) = record_by_sequence(parameters, entry.sequence, ctx)? else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(source_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset source pointer is invalid"),
            )?;
            continue;
        };
        let Some(flag) = record.integer(2).filter(|flag| matches!(flag, 1..=3)) else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset distance flag is not 1, 2, or 3"),
            )?;
            continue;
        };
        let components = [record.number(10), record.number(11), record.number(12)];
        let [Some(x_component), Some(y_component), Some(z_component)] = components else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset plane normal is not numeric"),
            )?;
            continue;
        };
        let Some(mut normal) = UnitVector3::normalized_by_reciprocal(Vector3::new(
            x_component,
            y_component,
            z_component,
        )) else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset plane normal is zero or non-finite"),
            )?;
            continue;
        };
        if declared_unit_vector(
            record,
            10,
            Vector3::new(x_component, y_component, z_component),
            global.real_precision(),
        )
        .is_none()
        {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset plane normal is not a unit vector"),
            )?;
            continue;
        }
        let native_bounds = [record.number(13), record.number(14)];
        let [Some(native_start), Some(native_end)] = native_bounds else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset parameter interval is not numeric"),
            )?;
            continue;
        };
        let Some(native_interval) = IncreasingParameterInterval::new([native_start, native_end])
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset parameter interval is not increasing"),
            )?;
            continue;
        };
        let mut identity_storage = ctx.reserve_scoped(0, "iges offset temporary identities")?;
        let source_id = identity_storage.with_storage(|| {
            crate::ids::curve_admitted(&crate::ids::Stem::directory(source_sequence), ctx)
        })?;
        index.update(ir, ctx, &mut index_storage)?;
        let Some(source_geometry) = ctx
            .get_btree_map(&index.curves, &source_id, "iges offset source curve query")?
            .and_then(|position| ir.model.curves.get(*position))
            .and_then(|curve| curve.geometry.solved())
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset source curve is missing"),
            )?;
            continue;
        };
        let source_range = source_parameter_range(
            ctx,
            ir,
            &index,
            &source_id,
            source_geometry,
            global.minimum_resolution_mm(),
        )?;
        let Some(source_range) = source_range else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "offset source has no bounded neutral parameter domain"
                ),
            )?;
            continue;
        };
        let Some(source_entry) = entry_by_sequence(directory, source_sequence, ctx)? else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset source Directory Entry is missing"),
            )?;
            continue;
        };
        let Some(source_record) = record_by_sequence(parameters, source_sequence, ctx)? else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset source Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(parameter_map) = source_parameter_map(source_entry, source_record, source_range)
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "offset source has no supported native-to-neutral parameter mapping"
                ),
            )?;
            continue;
        };
        if native_interval.lower() < parameter_map.native.lower()
            || native_interval.upper() > parameter_map.native.upper()
        {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "offset parameter interval lies outside the source curve domain"
                ),
            )?;
            continue;
        }
        let mut offset_source_id = Cow::Borrowed(&source_id);
        let mut offset_source_geometry = Cow::Borrowed(source_geometry);
        if entry.transform != 0 {
            if transform_tables.is_none() {
                let mut entries = BTreeMap::new();
                let mut directory_entries = directory.iter();
                while !directory_entries.as_slice().is_empty() {
                    let Some(entry) =
                        ctx.next_charged(&mut directory_entries, "iges offsets transform directory traversal")?
                    else {
                        break;
                    };
                    transform_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut entries,
                            entry.sequence,
                            entry,
                            "iges offsets transform directory index",
                        )
                    })?;
                }
                let mut records = BTreeMap::new();
                let mut source_index_entries = parameters.iter();
                while source_index_entries.len() != 0 {
                    let Some(record) =
                        ctx.next_charged(&mut source_index_entries, "iges offsets transform parameter traversal")?
                    else {
                        break;
                    };
                    transform_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut records,
                            record.directory_sequence,
                            record,
                            "iges offsets transform parameter index",
                        )
                    })?;
                }
                transform_tables = Some((entries, records));
            }
            let (entries, records) = transform_tables
                .as_ref()
                .ok_or_else(|| CodecError::malformed("offset transform lookup is absent"))?;
            let mut path_storage = ctx.reserve_scoped(0, "iges offsets transform path")?;
            let transform = match path_storage.with_storage(|| {
                resolve_transform(
                    entry.transform,
                    entries,
                    records,
                    factor,
                    global.real_precision(),
                    &mut BTreeSet::new(),
                    ctx,
                )
            }) {
                Ok(transform) => transform,
                Err(error) => {
                    let message = error.non_resource()?;
                    super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{message}"))?;
                    continue;
                }
            };
            let Some(placed_source_geometry) = placed_offset_source(source_geometry, transform)
            else {
                super::push_entity_loss_with_scoped_slots(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    format_args!(
                        "{}",
                        "placed offset source has no exact line or circle carrier"
                    ),
                )?;
                continue;
            };
            let Some(placed_normal) = placed_offset_normal(normal, transform) else {
                super::push_entity_loss_with_scoped_slots(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    format_args!("{}", "placed offset normal cannot be represented"),
                )?;
                continue;
            };
            normal = placed_normal;
            offset_source_id = Cow::Owned(identity_storage.with_storage(|| {
                crate::ids::curve_admitted(
                    &crate::ids::Stem::directory(entry.sequence)
                        .tail(crate::ids::Word::PlacedSource),
                    ctx,
                )
            })?);
            offset_source_geometry = Cow::Owned(placed_source_geometry);
        }
        let normal_direction = *normal.as_raw();
        let start = parameter_map.to_neutral(native_interval.lower());
        let end = parameter_map.to_neutral(native_interval.upper());
        let parameter_origin = parameter_map.to_neutral(0.0);
        let parameter_factor = parameter_map.scale();
        let (distance, distance_law, geometry) = match flag {
            1 => {
                if record.integer(3) != Some(0) {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "uniform offset DE2 is not explicit integer zero"),
                    )?;
                    continue;
                }
                if !omitted_or_integer_zero(record, 4)
                    || !omitted_or_integer_zero(record, 5)
                    || !(7..=9).all(|index| omitted_or_numeric_zero(record, index))
                {
                    super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "uniform offset has an unused scalar field that is neither zero nor omitted"))?;
                    continue;
                }
                let Some(distance) = record.number(6).and_then(FiniteReal::new) else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "uniform offset distance is not finite"),
                    )?;
                    continue;
                };
                let distance = distance.get() * factor;
                let geometry = match offset_source_geometry.as_ref() {
                    SolvedCurveGeometry::Line(line_curve)
                        if {
                            let direction = *line_curve.direction().as_raw();
                            normal_direction.dot(direction).abs() <= EPS_OFFSET_FRAME
                        } =>
                    {
                        let origin = line_curve.origin().get();
                        let direction = *line_curve.direction().as_raw();
                        let Some(payload) = admit_with_scoped_loss_slots(
                            FinitePoint3::new(
                                origin.translated(normal_direction.cross(direction), distance),
                            )
                            .ok_or("LineCurve.origin must be finite")
                            .map(|origin| {
                                cadmpeg_ir::geometry::analytic::LineCurve::new(
                                    origin,
                                    line_curve.direction(),
                                )
                            }),
                            entry,
                            &mut loss_slots_storage,
                            &mut losses,
                            ctx,
                        )?
                        else {
                            continue;
                        };
                        CurveGeometry::Solved(SolvedCurveGeometry::Line(payload))
                    }
                    SolvedCurveGeometry::Circle(circle_curve)
                        if {
                            let axis = circle_curve.frame().axis().as_raw();
                            normal_direction.dot(*axis).abs() >= 1.0 - EPS_OFFSET_FRAME
                        } =>
                    {
                        let axis = circle_curve.frame().axis().as_raw();
                        let radius = circle_curve.radius().get();
                        let offset_radius =
                            radius - distance * normal_direction.dot(*axis).signum();
                        if offset_radius <= 0.0 {
                            super::push_entity_loss_with_scoped_slots(
                                ctx,
                                &mut loss_slots_storage,
                                &mut losses,
                                entry,
                                format_args!("{}", "offset collapses or reverses the circle"),
                            )?;
                            continue;
                        }
                        let Some(payload) = admit_with_scoped_loss_slots(
                            PositiveLength::new(offset_radius)
                                .ok_or("CircleCurve.radius must be positive and finite")
                                .map(|radius| {
                                    cadmpeg_ir::geometry::analytic::CircleCurve::new(
                                        circle_curve.center(),
                                        *circle_curve.frame(),
                                        radius,
                                    )
                                }),
                            entry,
                            &mut loss_slots_storage,
                            &mut losses,
                            ctx,
                        )?
                        else {
                            continue;
                        };
                        CurveGeometry::Solved(SolvedCurveGeometry::Circle(payload))
                    }
                    _ => {
                        super::push_entity_loss_with_scoped_slots(
                            ctx,
                            &mut loss_slots_storage,
                            &mut losses,
                            entry,
                            format_args!("{}", "source curve has no exact uniform offset carrier"),
                        )?;
                        continue;
                    }
                };
                (distance, None, geometry)
            }
            2 => {
                if record.integer(3) != Some(0) {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "linear offset DE2 is not explicit integer zero"),
                    )?;
                    continue;
                }
                if !omitted_or_integer_zero(record, 4) {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "linear offset NDIM is neither zero nor omitted"),
                    )?;
                    continue;
                }
                let basis = match record.integer(5) {
                    Some(1) => CurveOffsetLawBasis::ArcLength,
                    Some(2) => CurveOffsetLawBasis::Parameter,
                    _ => {
                        super::push_entity_loss_with_scoped_slots(
                            ctx,
                            &mut loss_slots_storage,
                            &mut losses,
                            entry,
                            format_args!("{}", "linear offset basis is not 1 or 2"),
                        )?;
                        continue;
                    }
                };
                let values = [
                    record.number(6),
                    record.number(7),
                    record.number(8),
                    record.number(9),
                ];
                let [Some(d1), Some(td1), Some(d2), Some(td2)] = values else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "linear offset controls are not numeric"),
                    )?;
                    continue;
                };
                let [Some(d1), Some(td1), Some(d2), Some(td2)] =
                    [d1, td1, d2, td2].map(FiniteReal::new)
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!(
                            "{}",
                            "linear offset control range is not increasing and finite"
                        ),
                    )?;
                    continue;
                };
                let Some(native_control_range) = IncreasingParameterInterval::between(td1, td2)
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!(
                            "{}",
                            "linear offset control range is not increasing and finite"
                        ),
                    )?;
                    continue;
                };
                let distances = [d1.get() * factor, d2.get() * factor];
                let control_factor = match basis {
                    CurveOffsetLawBasis::ArcLength => factor,
                    CurveOffsetLawBasis::Parameter => parameter_factor,
                };
                let control_origin = match basis {
                    CurveOffsetLawBasis::ArcLength => 0.0,
                    CurveOffsetLawBasis::Parameter => parameter_origin,
                };
                let control_range = [
                    control_origin + native_control_range.lower() * control_factor,
                    control_origin + native_control_range.upper() * control_factor,
                ];
                let SolvedCurveGeometry::Line(line_curve) = offset_source_geometry.as_ref() else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "linear offset source has no exact neutral carrier"),
                    )?;
                    continue;
                };
                let direction = *line_curve.direction().as_raw();
                if normal_direction.dot(direction).abs() > EPS_OFFSET_FRAME {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset normal is not perpendicular to the line"),
                    )?;
                    continue;
                }
                let law_parameter = |parameter: f64| match basis {
                    CurveOffsetLawBasis::Parameter => parameter,
                    CurveOffsetLawBasis::ArcLength => parameter - start,
                };
                let evaluate_distance = |parameter: f64| {
                    let independent = law_parameter(parameter);
                    let span = control_range[1] - control_range[0];
                    let alpha = if span.is_finite() {
                        (independent - control_range[0]) / span
                    } else {
                        cadmpeg_ir::math::parameter_fraction(
                            independent,
                            control_range[0],
                            control_range[1],
                        )
                        .map_or(
                            (independent - control_range[0]) / span,
                            cadmpeg_ir::scalar::FiniteReal::get,
                        )
                    };
                    let ordinary = distances[0] + alpha * (distances[1] - distances[0]);
                    if ordinary.is_finite() {
                        ordinary
                    } else {
                        cadmpeg_ir::math::interpolate(distances[0], distances[1], alpha)
                            .map_or(ordinary, cadmpeg_ir::scalar::FiniteReal::get)
                    }
                };
                let offset_direction = normal_direction.cross(direction);
                let Some(source_start) =
                    finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::curve_point_solved(
                            ctx,
                            &offset_source_geometry,
                            start,
                        ),
                    )?)?
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "linear offset source start cannot be evaluated"),
                    )?;
                    continue;
                };
                let Some(source_end) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point_solved(ctx, &offset_source_geometry, end),
                )?)?
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "linear offset source end cannot be evaluated"),
                    )?;
                    continue;
                };
                let control_storage;
                let (mut controls, result_control_storage) =
                    ctx.temporary_vec(2, "iges linear-offset controls")?;
                control_storage = result_control_storage;
                controls.extend([
                    source_start.translated(offset_direction, evaluate_distance(start)),
                    source_end.translated(offset_direction, evaluate_distance(end)),
                ]);
                let mut knots = ctx.collection_vec(4, "iges linear-offset knots")?;
                knots.extend([start, start, end, end]);
                let law = CurveOffsetDistanceLaw::linear(basis, distances, control_range);
                let Some(controls) =
                    admit_offset_controls(ctx, control_storage, controls, "iges linear-offset admitted controls")?
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!(
                            "linear offset carrier is inconsistent: control_points contains a non-finite point"
                        ),
                    )?;
                    continue;
                };
                let offset_nurbs = match NurbsCurve::new(
                    ctx,
                    1,
                    knots,
                    NurbsPoles3::Polynomial { points: controls },
                    false,
                )? {
                    Ok(nurbs) => nurbs,
                    Err(error) => {
                        super::push_entity_loss_with_scoped_slots(
                            ctx,
                            &mut loss_slots_storage,
                            &mut losses,
                            entry,
                            format_args!("linear offset carrier is inconsistent: {error}"),
                        )?;
                        continue;
                    }
                };
                (
                    distances[0],
                    Some(law),
                    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(offset_nurbs)),
                )
            }
            3 => {
                let Some(function_sequence) = record
                    .integer(3)
                    .and_then(|value| u32::try_from(value).ok())
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function pointer is invalid"),
                    )?;
                    continue;
                };
                let Some(coordinate_index) = record
                    .integer(4)
                    .and_then(|value| u8::try_from(value).ok())
                    .and_then(|value| {
                        cadmpeg_ir::geometry::CurveOffsetCoordinate::try_new(value).ok()
                    })
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function coordinate is not 1, 2, or 3"),
                    )?;
                    continue;
                };
                let basis = match record.integer(5) {
                    Some(1) => CurveOffsetLawBasis::ArcLength,
                    Some(2) => CurveOffsetLawBasis::Parameter,
                    _ => {
                        super::push_entity_loss_with_scoped_slots(
                            ctx,
                            &mut loss_slots_storage,
                            &mut losses,
                            entry,
                            format_args!("{}", "function offset basis is not 1 or 2"),
                        )?;
                        continue;
                    }
                };
                if !(6..=9).all(|index| omitted_or_numeric_zero(record, index)) {
                    super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{}", "function offset has an unused distance field that is neither zero nor omitted"))?;
                    continue;
                }
                let function_id = crate::ids::curve_admitted(
                    &crate::ids::Stem::directory(function_sequence),
                    ctx,
                )?;
                let Some(function) = ctx
                    .get_btree_map(
                        &index.curves,
                        &function_id,
                        "iges offset function curve query",
                    )?
                    .and_then(|position| ir.model.curves.get(*position))
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function curve is missing"),
                    )?;
                    continue;
                };
                let Some(SolvedCurveGeometry::Nurbs(function_nurbs)) = function.geometry.solved()
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function has no polynomial NURBS carrier"),
                    )?;
                    continue;
                };
                if matches!(
                    function_nurbs.pole_rows(),
                    cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. }
                ) || function_nurbs.degree() == 0
                {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function is rational or degree zero"),
                    )?;
                    continue;
                }
                let SolvedCurveGeometry::Line(line_curve) = offset_source_geometry.as_ref() else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "function offset source has no exact neutral carrier"),
                    )?;
                    continue;
                };
                let direction = *line_curve.direction().as_raw();
                if normal_direction.dot(direction).abs() > EPS_OFFSET_FRAME {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset normal is not perpendicular to the line"),
                    )?;
                    continue;
                }
                let (function_parameter_offset, function_parameter_scale) = match basis {
                    CurveOffsetLawBasis::ArcLength => (0.0, 1.0 / factor),
                    CurveOffsetLawBasis::Parameter => {
                        (-parameter_origin / parameter_factor, 1.0 / parameter_factor)
                    }
                };
                let independent_range = match basis {
                    CurveOffsetLawBasis::ArcLength => [0.0, end - start],
                    CurveOffsetLawBasis::Parameter => [start, end],
                };
                let function_range = independent_range
                    .map(|value| function_parameter_offset + function_parameter_scale * value);
                let degree = cadmpeg_core::decode::index_from_u32(function_nurbs.degree());
                let Some(domain_start) = function_nurbs.knots().get(degree).copied() else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function knot domain is missing"),
                    )?;
                    continue;
                };
                let Some(domain_end) = degree
                    .checked_add(1)
                    .and_then(|count| function_nurbs.knots().len().checked_sub(count))
                    .and_then(|index| function_nurbs.knots().get(index))
                    .copied()
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function knot domain is missing"),
                    )?;
                    continue;
                };
                if function_range[0] < domain_start || function_range[1] > domain_end {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!(
                            "{}",
                            "offset function domain does not cover the source interval"
                        ),
                    )?;
                    continue;
                }
                let inverse_parameter =
                    |value: f64| (value - function_parameter_offset) / function_parameter_scale;
                let source_parameter = |independent: f64| match basis {
                    CurveOffsetLawBasis::ArcLength => start + independent,
                    CurveOffsetLawBasis::Parameter => independent,
                };
                let offset_direction = normal_direction.cross(direction);
                let control_storage;
                let (mut controls, result_control_storage) = ctx
                    .temporary_vec(function_nurbs.pole_count(), "iges function-offset controls")?;
                control_storage = result_control_storage;
                let mut indices = 0..function_nurbs.pole_count();
                while !indices.is_empty() || ctx.resource_refusal().is_some() {
                    let Some(index) = ctx.next_charged(&mut indices, "iges function-offset control traversal")? else { break; };
                    let Some(function_control) = function_nurbs.pole_rows().point_at(index) else {
                        ctx.clear_vec(&mut controls, "iges offset rejected controls")?;
                        break;
                    };
                    let Some(function_parameter) =
                        greville(function_nurbs.knots(), degree, index, ctx)?
                    else {
                        super::push_entity_loss_with_scoped_slots(
                            ctx,
                            &mut loss_slots_storage,
                            &mut losses,
                            entry,
                            format_args!("{}", "offset function Greville parameter is missing"),
                        )?;
                        ctx.clear_vec(&mut controls, "iges offset rejected controls")?;
                        break;
                    };
                    let independent = inverse_parameter(function_parameter);
                    let Some(base) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::curve_point_solved(
                            ctx,
                            &offset_source_geometry,
                            source_parameter(independent),
                        ),
                    )?)?
                    else {
                        ctx.clear_vec(&mut controls, "iges offset rejected controls")?;
                        break;
                    };
                    let Some(distance) = coordinate(function_control.get(), coordinate_index.get())
                    else {
                        ctx.clear_vec(&mut controls, "iges offset rejected controls")?;
                        break;
                    };
                    controls.push(base.translated(offset_direction, distance));
                }
                if controls.len() != function_nurbs.pole_count() {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function controls cannot be composed"),
                    )?;
                    continue;
                }
                let mut knots =
                    ctx.collection_vec(function_nurbs.knots().len(), "iges function-offset knots")?;
                knots.extend(
                    ctx.admit_iter(
                        &function_nurbs.knots()[..],
                        "iges function-offset knot mapping",
                    )?
                    .map(|value| source_parameter(inverse_parameter(*value))),
                );
                let Some(function_start) =
                    finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::curve_point(
                            ctx,
                            &function.geometry,
                            function_range[0],
                        ),
                    )?)?
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function start cannot be evaluated"),
                    )?;
                    continue;
                };
                let Some(distance) = coordinate(function_start.get(), coordinate_index.get())
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", "offset function coordinate is invalid"),
                    )?;
                    continue;
                };
                let law = CurveOffsetDistanceLaw::Coordinate {
                    function: function_id,
                    coordinate: coordinate_index,
                    basis,
                    function_parameter_offset,
                    function_parameter_scale,
                };
                let Some(controls) =
                    admit_offset_controls(ctx, control_storage, controls, "iges function-offset admitted controls")?
                else {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!(
                            "offset-function carrier is inconsistent: control_points contains a non-finite point"
                        ),
                    )?;
                    continue;
                };
                let offset_nurbs = match NurbsCurve::new(
                    ctx,
                    function_nurbs.degree(),
                    knots,
                    NurbsPoles3::Polynomial { points: controls },
                    false,
                )? {
                    Ok(nurbs) => nurbs,
                    Err(error) => {
                        super::push_entity_loss_with_scoped_slots(
                            ctx,
                            &mut loss_slots_storage,
                            &mut losses,
                            entry,
                            format_args!("offset-function carrier is inconsistent: {error}"),
                        )?;
                        continue;
                    }
                };
                (
                    distance,
                    Some(Ok(law)),
                    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(offset_nurbs)),
                )
            }
            _ => {
                super::push_entity_loss_with_scoped_slots(
                    ctx,
                    &mut loss_slots_storage,
                    &mut losses,
                    entry,
                    format_args!("{}", "offset curve form is unsupported"),
                )?;
                continue;
            }
        };
        let Some(start_position) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, &geometry, start),
        )?)?
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset start parameter cannot be evaluated"),
            )?;
            continue;
        };
        let Some(end_position) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, &geometry, end),
        )?)?
        else {
            super::push_entity_loss_with_scoped_slots(
                ctx,
                &mut loss_slots_storage,
                &mut losses,
                entry,
                format_args!("{}", "offset end parameter cannot be evaluated"),
            )?;
            continue;
        };
        let curve_id = identity_storage.with_storage(|| {
            crate::ids::curve_admitted(&crate::ids::Stem::directory(entry.sequence), ctx)
        })?;
        let start_point = crate::ids::point_admitted(
            &crate::ids::Stem::directory(entry.sequence).part(crate::ids::Word::Start),
            ctx,
        )?;
        sequences.record_point(
            &start_point,
            &crate::ids::Stem::directory(entry.sequence),
            ctx,
        )?;
        let end_point = crate::ids::point_admitted(
            &crate::ids::Stem::directory(entry.sequence).part(crate::ids::Word::End),
            ctx,
        )?;
        sequences.record_point(
            &end_point,
            &crate::ids::Stem::directory(entry.sequence),
            ctx,
        )?;
        let start_vertex = crate::ids::vertex_admitted(
            &crate::ids::Stem::directory(entry.sequence).part(crate::ids::Word::Start),
            ctx,
        )?;
        let end_vertex = crate::ids::vertex_admitted(
            &crate::ids::Stem::directory(entry.sequence).part(crate::ids::Word::End),
            ctx,
        )?;
        let edge_id = crate::ids::edge_admitted(&crate::ids::Stem::directory(entry.sequence), ctx)?;
        let range = distance_law
            .transpose()
            .and_then(|distance_law| match distance_law {
                Some(distance_law) => {
                    cadmpeg_ir::geometry::CurveOffsetRange::variable([start, end], distance_law)
                }
                None => cadmpeg_ir::geometry::CurveOffsetRange::uniform([start, end]),
            });
        let payload = match range {
            Ok(range) => {
                let source_id = offset_source_id
                    .try_clone_for_decode(ctx, "iges offset procedural source identity")?;
                cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::with_unit_plane_normal(
                    source_id, distance, normal, Some(range),
                )
            }
            Err(error) => Err(error),
        };
        let admitted_payload = match payload {
            Ok(admitted_payload) => admitted_payload,
            Err(error) => {
                super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{error}"))?;
                continue;
            }
        };
        let procedural = ProceduralCurve::new(
            crate::ids::procedural_curve_admitted(
                &crate::ids::Stem::directory(entry.sequence),
                ctx,
            )?,
            ProceduralCurveDefinition::Offset(admitted_payload),
        );
        if !ctx.equal_bytes(
            offset_source_id.as_str().as_bytes(),
            source_id.as_str().as_bytes(),
            "iges offset placed identity comparison",
        )? {
            let placed_geometry = match offset_source_geometry {
                Cow::Owned(geometry) => geometry,
                Cow::Borrowed(_) => {
                    return Err(CodecError::Malformed(
                        "placed offset source is absent".into(),
                    ));
                }
            };
            sequences.record_curve(&offset_source_id, entry.sequence, ctx)?;
            ctx.reserve_vec(&mut ir.model.curves, 1, "iges offset source curve slots")?;
            ctx.charge_entities(1, "iges_geometry_offsets")?;
            ir.model.curves.push(Curve {
                id: offset_source_id
                    .try_clone_for_decode(ctx, "iges offset placed source identity")?,
                geometry: CurveGeometry::Solved(placed_geometry),
                source_object: Some(match source_object(entry, ctx) {
                    Ok(source) => source,
                    Err(error) => {
                        super::push_entity_loss_with_scoped_slots(
                            ctx,
                            &mut loss_slots_storage,
                            &mut losses,
                            entry,
                            format_args!("{}", super::non_resource_error(error, ctx)?),
                        )?;
                        continue;
                    }
                }),
            });
        }
        ctx.reserve_vec(&mut ir.model.points, 2, "iges offset neutral point slots")?;
        ctx.charge_entities(2, "iges_geometry_offsets")?;
        ir.model.points.extend([
            Point::new(
                start_point.try_clone_for_decode(ctx, "iges offset start point identity")?,
                start_position,
                None,
            ),
            Point::new(
                end_point.try_clone_for_decode(ctx, "iges offset end point identity")?,
                end_position,
                None,
            ),
        ]);
        ctx.reserve_vec(
            &mut ir.model.vertices,
            2,
            "iges offset neutral vertex slots",
        )?;
        ctx.charge_entities(2, "iges_geometry_offsets")?;
        ir.model.vertices.extend([
            Vertex {
                id: start_vertex.try_clone_for_decode(ctx, "iges offset start vertex identity")?,
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end_vertex.try_clone_for_decode(ctx, "iges offset end vertex identity")?,
                point: end_point,
                tolerance: None,
            },
        ]);
        sequences.record_curve(&curve_id, entry.sequence, ctx)?;
        ctx.reserve_vec(&mut ir.model.curves, 1, "iges offset neutral curve slots")?;
        ctx.charge_entities(1, "iges_geometry_offsets")?;
        ir.model.curves.push(Curve {
            id: curve_id.try_clone_for_decode(ctx, "iges offset curve identity")?,
            geometry,
            source_object: Some(match source_object(entry, ctx) {
                Ok(source) => source,
                Err(error) => {
                    super::push_entity_loss_with_scoped_slots(
                        ctx,
                        &mut loss_slots_storage,
                        &mut losses,
                        entry,
                        format_args!("{}", super::non_resource_error(error, ctx)?),
                    )?;
                    continue;
                }
            }),
        });
        let carrier = match cadmpeg_ir::topology::EdgeCarrier::new(
            Some(curve_id.try_clone_for_decode(ctx, "iges offset edge carrier identity")?),
            Some([start, end]),
        ) {
            Ok(carrier) => carrier,
            Err(error) => {
                super::push_entity_loss_with_scoped_slots(ctx, &mut loss_slots_storage, &mut losses, entry, format_args!("{error}"))?;
                continue;
            }
        };
        ctx.reserve_vec(&mut ir.model.edges, 1, "iges offset neutral edge slots")?;
        ctx.charge_entities(1, "iges_geometry_offsets")?;
        ir.model.edges.push(Edge {
            id: edge_id.try_clone_for_decode(ctx, "iges offset edge identity")?,
            carrier,
            start: start_vertex,
            end: end_vertex,
            tolerance: None,
        });

        ctx.charge_entities(1, "iges_geometry_offsets")?;
        let _attached = ir.model.add_procedural_curve(ctx, &curve_id, procedural)?;
        ctx.reserve_scoped_vec(&mut wire_slots_storage, &mut wire_edges, 1, "iges offset wire edge slots")?;
        wire_edges.push(edge_id);
        ctx.insert_scoped_btree_set(&mut decoded_storage, &mut decoded, entry.sequence, "iges offsets decoded sequences", "iges offsets decoded sequences")?;
    }

    Ok(WireProjectionOutcome {
        decoded,
        decoded_storage,
        losses,
        loss_slots_storage,
        wire_edges,
        wire_slots_storage,
    })
}

#[cfg(test)]
mod tests;
