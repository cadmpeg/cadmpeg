// SPDX-License-Identifier: Apache-2.0
//! Offset curve entity projection.

use super::curve_conversion::angularly_equal;
use super::geometry::{
    declared_unit_vector, entity_loss, resolve_transform, source_object, WireProjectionOutcome,
};
use crate::directory::DirectoryEntry;
use crate::global::ProjectedGlobal;
use crate::parameter::{ParameterRecord, TokenValue};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, CurveOffsetDistanceLaw, CurveOffsetLawBasis, NurbsCurve, ProceduralCurve,
    ProceduralCurveDefinition,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const EPS_OFFSET_FRAME: f64 = 1.0e-10;

fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| vector.scale(1.0 / norm))
}

fn transform_orientation(transform: cadmpeg_ir::transform::Transform) -> Option<f64> {
    let x = transform.apply_vector(Vector3::new(1.0, 0.0, 0.0));
    let y = transform.apply_vector(Vector3::new(0.0, 1.0, 0.0));
    let z = transform.apply_vector(Vector3::new(0.0, 0.0, 1.0));
    let determinant = x.cross(y).dot(z);
    (determinant.is_finite() && determinant != 0.0).then_some(determinant.signum())
}

fn placed_offset_normal(
    normal: Vector3,
    transform: cadmpeg_ir::transform::Transform,
) -> Option<Vector3> {
    let orientation = transform_orientation(transform)?;
    unit_vector(transform.apply_vector(normal).scale(orientation))
}

fn placed_offset_source(
    geometry: &CurveGeometry,
    transform: cadmpeg_ir::transform::Transform,
) -> Option<CurveGeometry> {
    let orientation = transform_orientation(transform)?;
    match geometry {
        CurveGeometry::Line { origin, direction } => Some(CurveGeometry::Line {
            origin: transform.apply_point(*origin),
            direction: unit_vector(transform.apply_vector(*direction))?,
        }),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => Some(CurveGeometry::Circle {
            center: transform.apply_point(*center),
            axis: unit_vector(transform.apply_vector(*axis))?.scale(orientation),
            ref_direction: unit_vector(transform.apply_vector(*ref_direction))?,
            radius: *radius,
        }),
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

fn greville(knots: &[f64], degree: usize, control: usize) -> Option<f64> {
    let values = knots.get(control + 1..=control + degree)?;
    Some(values.iter().sum::<f64>() / degree as f64)
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
        Some(TokenValue::Omitted | TokenValue::Integer(0) | TokenValue::Real(0.0))
    )
}

#[derive(Clone, Copy)]
struct SourceParameterMap {
    native: [f64; 2],
    neutral: [f64; 2],
}

impl SourceParameterMap {
    fn new(native: [f64; 2], neutral: [f64; 2]) -> Option<Self> {
        (native
            .iter()
            .chain(neutral.iter())
            .all(|value| value.is_finite())
            && native[0] < native[1]
            && neutral[0] < neutral[1])
            .then_some(Self { native, neutral })
    }

    fn scale(self) -> f64 {
        (self.neutral[1] - self.neutral[0]) / (self.native[1] - self.native[0])
    }

    fn to_neutral(self, value: f64) -> f64 {
        self.neutral[0] + (value - self.native[0]) * self.scale()
    }
}

fn source_parameter_map(
    entry: &DirectoryEntry,
    record: &ParameterRecord,
    neutral: [f64; 2],
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
        (102 | 112, 0) | (106, 11..=13 | 63) | (126, 0..=5) => neutral,
        _ => return None,
    };
    SourceParameterMap::new(native, neutral)
}

fn source_parameter_range(
    ir: &CadIr,
    source_id: &CurveId,
    geometry: &CurveGeometry,
    tolerance: f64,
) -> Option<[f64; 2]> {
    let point_position = |vertex: &VertexId| {
        let point_id = ir
            .model
            .vertices
            .iter()
            .find(|item| item.id == *vertex)?
            .point
            .clone();
        ir.model
            .points
            .iter()
            .find(|item| item.id == point_id)
            .map(|point| point.position)
    };
    let candidates = ir
        .model
        .edges
        .iter()
        .filter(|edge| edge.curve.as_ref() == Some(source_id))
        .filter_map(|edge| {
            let range = edge.param_range?;
            let start = point_position(&edge.start)?;
            let end = point_position(&edge.end)?;
            let evaluated_start = cadmpeg_ir::eval::curve_point(geometry, range[0])?;
            let evaluated_end = cadmpeg_ir::eval::curve_point(geometry, range[1])?;
            (evaluated_start.distance(start) <= tolerance
                && evaluated_end.distance(end) <= tolerance)
                .then_some(range)
        })
        .collect::<Vec<_>>();
    let range = *candidates.first()?;
    candidates
        .iter()
        .all(|candidate| *candidate == range)
        .then_some(range)
}

#[allow(clippy::many_single_char_names)]
pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> WireProjectionOutcome {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut wire_edges = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 130 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(source_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "offset source pointer is invalid"));
            continue;
        };
        let Some(flag) = record.integer(2).filter(|flag| matches!(flag, 1..=3)) else {
            losses.push(entity_loss(entry, "offset distance flag is not 1, 2, or 3"));
            continue;
        };
        let components = [record.number(10), record.number(11), record.number(12)];
        #[allow(clippy::many_single_char_names)]
        let [Some(x), Some(y), Some(z)] = components
        else {
            losses.push(entity_loss(entry, "offset plane normal is not numeric"));
            continue;
        };
        let Some(mut normal) = ({
            let v = Vector3::new(x, y, z);
            let n = v.norm();
            (n.is_finite() && n > 0.0).then(|| v.scale(1.0 / n))
        }) else {
            losses.push(entity_loss(
                entry,
                "offset plane normal is zero or non-finite",
            ));
            continue;
        };
        if !declared_unit_vector(record, 10, Vector3::new(x, y, z), global.real_precision()) {
            losses.push(entity_loss(
                entry,
                "offset plane normal is not a unit vector",
            ));
            continue;
        }
        let native_bounds = [record.number(13), record.number(14)];
        let [Some(native_start), Some(native_end)] = native_bounds else {
            losses.push(entity_loss(
                entry,
                "offset parameter interval is not numeric",
            ));
            continue;
        };
        if !native_start.is_finite() || !native_end.is_finite() || native_start >= native_end {
            losses.push(entity_loss(
                entry,
                "offset parameter interval is not increasing",
            ));
            continue;
        }
        let source_id = CurveId(format!("iges:model:curve#D{source_sequence}"));
        let Some(source_geometry) = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == source_id)
            .map(|curve| curve.geometry.clone())
        else {
            losses.push(entity_loss(entry, "offset source curve is missing"));
            continue;
        };
        let source_range = source_parameter_range(
            ir,
            &source_id,
            &source_geometry,
            global.minimum_resolution_mm(),
        );
        let Some(source_range) = source_range else {
            losses.push(entity_loss(
                entry,
                "offset source has no bounded neutral parameter domain",
            ));
            continue;
        };
        let Some(source_entry) = entries.get(&source_sequence).copied() else {
            losses.push(entity_loss(
                entry,
                "offset source Directory Entry is missing",
            ));
            continue;
        };
        let Some(source_record) = records.get(&source_sequence).copied() else {
            losses.push(entity_loss(
                entry,
                "offset source Parameter Data record is missing",
            ));
            continue;
        };
        let Some(parameter_map) = source_parameter_map(source_entry, source_record, source_range)
        else {
            losses.push(entity_loss(
                entry,
                "offset source has no supported native-to-neutral parameter mapping",
            ));
            continue;
        };
        if native_start < parameter_map.native[0] || native_end > parameter_map.native[1] {
            losses.push(entity_loss(
                entry,
                "offset parameter interval lies outside the source curve domain",
            ));
            continue;
        }
        let mut offset_source_id = source_id.clone();
        let mut offset_source_geometry = source_geometry.clone();
        if entry.transform != 0 {
            let transform = match resolve_transform(
                entry.transform,
                &entries,
                &records,
                factor,
                global.real_precision(),
                &mut BTreeSet::new(),
                ctx,
            ) {
                Ok(transform) => transform,
                Err(message) => {
                    losses.push(entity_loss(entry, message));
                    continue;
                }
            };
            let body_transform = transform.body_transform();
            let Some(placed_source_geometry) =
                placed_offset_source(&source_geometry, body_transform)
            else {
                losses.push(entity_loss(
                    entry,
                    "placed offset source has no exact line or circle carrier",
                ));
                continue;
            };
            let Some(placed_normal) = placed_offset_normal(normal, body_transform) else {
                losses.push(entity_loss(
                    entry,
                    "placed offset normal cannot be represented",
                ));
                continue;
            };
            normal = placed_normal;
            offset_source_id = CurveId(format!(
                "iges:model:curve#D{}-placed-source",
                entry.sequence
            ));
            offset_source_geometry = placed_source_geometry.clone();
        }
        let start = parameter_map.to_neutral(native_start);
        let end = parameter_map.to_neutral(native_end);
        let parameter_origin = parameter_map.to_neutral(0.0);
        let parameter_factor = parameter_map.scale();
        let (distance, distance_law, geometry) = match flag {
            1 => {
                if record.integer(3) != Some(0) {
                    losses.push(entity_loss(
                        entry,
                        "uniform offset DE2 is not explicit integer zero",
                    ));
                    continue;
                }
                if !omitted_or_integer_zero(record, 4)
                    || !omitted_or_integer_zero(record, 5)
                    || !(7..=9).all(|index| omitted_or_numeric_zero(record, index))
                {
                    losses.push(entity_loss(
                        entry,
                        "uniform offset has an unused scalar field that is neither zero nor omitted",
                    ));
                    continue;
                }
                let Some(distance) = record.number(6).filter(|value| value.is_finite()) else {
                    losses.push(entity_loss(entry, "uniform offset distance is not finite"));
                    continue;
                };
                let distance = distance * factor;
                let geometry = match &offset_source_geometry {
                    CurveGeometry::Line { origin, direction }
                        if normal.dot(*direction).abs() <= EPS_OFFSET_FRAME =>
                    {
                        CurveGeometry::Line {
                            origin: origin.translated(normal.cross(*direction), distance),
                            direction: *direction,
                        }
                    }
                    CurveGeometry::Circle {
                        center,
                        axis,
                        ref_direction,
                        radius,
                    } if normal.dot(*axis).abs() >= 1.0 - EPS_OFFSET_FRAME => {
                        let offset_radius = radius - distance * normal.dot(*axis).signum();
                        if offset_radius <= 0.0 {
                            losses.push(entity_loss(
                                entry,
                                "offset collapses or reverses the circle",
                            ));
                            continue;
                        }
                        CurveGeometry::Circle {
                            center: *center,
                            axis: *axis,
                            ref_direction: *ref_direction,
                            radius: offset_radius,
                        }
                    }
                    _ => {
                        losses.push(entity_loss(
                            entry,
                            "source curve has no exact uniform offset carrier",
                        ));
                        continue;
                    }
                };
                (distance, None, geometry)
            }
            2 => {
                if record.integer(3) != Some(0) {
                    losses.push(entity_loss(
                        entry,
                        "linear offset DE2 is not explicit integer zero",
                    ));
                    continue;
                }
                if !omitted_or_integer_zero(record, 4) {
                    losses.push(entity_loss(
                        entry,
                        "linear offset NDIM is neither zero nor omitted",
                    ));
                    continue;
                }
                let basis = match record.integer(5) {
                    Some(1) => CurveOffsetLawBasis::ArcLength,
                    Some(2) => CurveOffsetLawBasis::Parameter,
                    _ => {
                        losses.push(entity_loss(entry, "linear offset basis is not 1 or 2"));
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
                    losses.push(entity_loss(entry, "linear offset controls are not numeric"));
                    continue;
                };
                if [d1, td1, d2, td2].iter().any(|value| !value.is_finite()) || td1 >= td2 {
                    losses.push(entity_loss(
                        entry,
                        "linear offset control range is not increasing and finite",
                    ));
                    continue;
                }
                let distances = [d1 * factor, d2 * factor];
                let control_factor = match basis {
                    CurveOffsetLawBasis::ArcLength => factor,
                    CurveOffsetLawBasis::Parameter => parameter_factor,
                };
                let control_origin = match basis {
                    CurveOffsetLawBasis::ArcLength => 0.0,
                    CurveOffsetLawBasis::Parameter => parameter_origin,
                };
                let control_range = [
                    control_origin + td1 * control_factor,
                    control_origin + td2 * control_factor,
                ];
                let CurveGeometry::Line { direction, .. } = &offset_source_geometry else {
                    losses.push(entity_loss(
                        entry,
                        "linear offset source has no exact neutral carrier",
                    ));
                    continue;
                };
                if normal.dot(*direction).abs() > EPS_OFFSET_FRAME {
                    losses.push(entity_loss(
                        entry,
                        "offset normal is not perpendicular to the line",
                    ));
                    continue;
                }
                let law_parameter = |parameter: f64| match basis {
                    CurveOffsetLawBasis::Parameter => parameter,
                    CurveOffsetLawBasis::ArcLength => parameter - start,
                };
                let evaluate_distance = |parameter: f64| {
                    let alpha = (law_parameter(parameter) - control_range[0])
                        / (control_range[1] - control_range[0]);
                    distances[0] + alpha * (distances[1] - distances[0])
                };
                let offset_direction = normal.cross(*direction);
                let Some(source_start) =
                    cadmpeg_ir::eval::curve_point(&offset_source_geometry, start)
                else {
                    losses.push(entity_loss(
                        entry,
                        "linear offset source start cannot be evaluated",
                    ));
                    continue;
                };
                let Some(source_end) = cadmpeg_ir::eval::curve_point(&offset_source_geometry, end)
                else {
                    losses.push(entity_loss(
                        entry,
                        "linear offset source end cannot be evaluated",
                    ));
                    continue;
                };
                let controls = vec![
                    source_start.translated(offset_direction, evaluate_distance(start)),
                    source_end.translated(offset_direction, evaluate_distance(end)),
                ];
                let law = CurveOffsetDistanceLaw::Linear {
                    basis,
                    distances,
                    control_range,
                };
                (
                    distances[0],
                    Some(law),
                    CurveGeometry::Nurbs(NurbsCurve {
                        degree: 1,
                        knots: vec![start, start, end, end],
                        control_points: controls,
                        weights: None,
                        periodic: false,
                    }),
                )
            }
            3 => {
                let Some(function_sequence) = record
                    .integer(3)
                    .and_then(|value| u32::try_from(value).ok())
                else {
                    losses.push(entity_loss(entry, "offset function pointer is invalid"));
                    continue;
                };
                let Some(coordinate_index) = record
                    .integer(4)
                    .and_then(|value| u8::try_from(value).ok())
                    .filter(|value| matches!(value, 1..=3))
                else {
                    losses.push(entity_loss(
                        entry,
                        "offset function coordinate is not 1, 2, or 3",
                    ));
                    continue;
                };
                let basis = match record.integer(5) {
                    Some(1) => CurveOffsetLawBasis::ArcLength,
                    Some(2) => CurveOffsetLawBasis::Parameter,
                    _ => {
                        losses.push(entity_loss(entry, "function offset basis is not 1 or 2"));
                        continue;
                    }
                };
                if !(6..=9).all(|index| omitted_or_numeric_zero(record, index)) {
                    losses.push(entity_loss(
                        entry,
                        "function offset has an unused distance field that is neither zero nor omitted",
                    ));
                    continue;
                }
                let function_id = CurveId(format!("iges:model:curve#D{function_sequence}"));
                let Some(function) = ir.model.curves.iter().find(|curve| curve.id == function_id)
                else {
                    losses.push(entity_loss(entry, "offset function curve is missing"));
                    continue;
                };
                let CurveGeometry::Nurbs(function_nurbs) = &function.geometry else {
                    losses.push(entity_loss(
                        entry,
                        "offset function has no polynomial NURBS carrier",
                    ));
                    continue;
                };
                if function_nurbs.weights.is_some() || function_nurbs.degree == 0 {
                    losses.push(entity_loss(
                        entry,
                        "offset function is rational or degree zero",
                    ));
                    continue;
                }
                let CurveGeometry::Line { direction, .. } = &offset_source_geometry else {
                    losses.push(entity_loss(
                        entry,
                        "function offset source has no exact neutral carrier",
                    ));
                    continue;
                };
                if normal.dot(*direction).abs() > EPS_OFFSET_FRAME {
                    losses.push(entity_loss(
                        entry,
                        "offset normal is not perpendicular to the line",
                    ));
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
                let degree = function_nurbs.degree as usize;
                let Some(domain_start) = function_nurbs.knots.get(degree).copied() else {
                    losses.push(entity_loss(entry, "offset function knot domain is missing"));
                    continue;
                };
                let Some(domain_end) = function_nurbs
                    .knots
                    .get(function_nurbs.knots.len().saturating_sub(degree + 1))
                    .copied()
                else {
                    losses.push(entity_loss(entry, "offset function knot domain is missing"));
                    continue;
                };
                if function_range[0] < domain_start || function_range[1] > domain_end {
                    losses.push(entity_loss(
                        entry,
                        "offset function domain does not cover the source interval",
                    ));
                    continue;
                }
                let inverse_parameter =
                    |value: f64| (value - function_parameter_offset) / function_parameter_scale;
                let source_parameter = |independent: f64| match basis {
                    CurveOffsetLawBasis::ArcLength => start + independent,
                    CurveOffsetLawBasis::Parameter => independent,
                };
                let offset_direction = normal.cross(*direction);
                let mut controls = Vec::with_capacity(function_nurbs.control_points.len());
                for (index, function_control) in
                    function_nurbs.control_points.iter().copied().enumerate()
                {
                    let Some(function_parameter) = greville(&function_nurbs.knots, degree, index)
                    else {
                        losses.push(entity_loss(
                            entry,
                            "offset function Greville parameter is missing",
                        ));
                        controls.clear();
                        break;
                    };
                    let independent = inverse_parameter(function_parameter);
                    let Some(base) = cadmpeg_ir::eval::curve_point(
                        &offset_source_geometry,
                        source_parameter(independent),
                    ) else {
                        controls.clear();
                        break;
                    };
                    let Some(distance) = coordinate(function_control, coordinate_index) else {
                        controls.clear();
                        break;
                    };
                    controls.push(base.translated(offset_direction, distance));
                }
                if controls.len() != function_nurbs.control_points.len() {
                    losses.push(entity_loss(
                        entry,
                        "offset function controls cannot be composed",
                    ));
                    continue;
                }
                let knots = function_nurbs
                    .knots
                    .iter()
                    .map(|value| source_parameter(inverse_parameter(*value)))
                    .collect();
                let Some(function_start) =
                    cadmpeg_ir::eval::curve_point(&function.geometry, function_range[0])
                else {
                    losses.push(entity_loss(
                        entry,
                        "offset function start cannot be evaluated",
                    ));
                    continue;
                };
                let Some(distance) = coordinate(function_start, coordinate_index) else {
                    losses.push(entity_loss(entry, "offset function coordinate is invalid"));
                    continue;
                };
                let law = CurveOffsetDistanceLaw::Coordinate {
                    function: function_id,
                    coordinate: coordinate_index,
                    basis,
                    function_parameter_offset,
                    function_parameter_scale,
                };
                (
                    distance,
                    Some(law),
                    CurveGeometry::Nurbs(NurbsCurve {
                        degree: function_nurbs.degree,
                        knots,
                        control_points: controls,
                        weights: None,
                        periodic: false,
                    }),
                )
            }
            _ => {
                losses.push(entity_loss(entry, "offset curve form is unsupported"));
                continue;
            }
        };
        let Some(start_position) = cadmpeg_ir::eval::curve_point(&geometry, start) else {
            losses.push(entity_loss(
                entry,
                "offset start parameter cannot be evaluated",
            ));
            continue;
        };
        let Some(end_position) = cadmpeg_ir::eval::curve_point(&geometry, end) else {
            losses.push(entity_loss(
                entry,
                "offset end parameter cannot be evaluated",
            ));
            continue;
        };
        let curve_id = CurveId(format!("iges:model:curve#D{}", entry.sequence));
        let start_point = PointId(format!("iges:model:point#D{}:start", entry.sequence));
        let end_point = PointId(format!("iges:model:point#D{}:end", entry.sequence));
        let start_vertex = VertexId(format!("iges:model:vertex#D{}:start", entry.sequence));
        let end_vertex = VertexId(format!("iges:model:vertex#D{}:end", entry.sequence));
        let edge_id = EdgeId(format!("iges:model:edge#D{}", entry.sequence));
        if offset_source_id != source_id {
            ir.model.curves.push(Curve {
                id: offset_source_id.clone(),
                geometry: offset_source_geometry.clone(),
                source_object: Some(source_object(entry)),
            });
        }
        ir.model.points.extend([
            Point {
                source_object: None,
                id: start_point.clone(),
                position: start_position,
            },
            Point {
                source_object: None,
                id: end_point.clone(),
                position: end_position,
            },
        ]);
        ir.model.vertices.extend([
            Vertex {
                id: start_vertex.clone(),
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end_vertex.clone(),
                point: end_point,
                tolerance: None,
            },
        ]);
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry,
            source_object: Some(source_object(entry)),
        });
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            curve: Some(curve_id.clone()),
            start: start_vertex,
            end: end_vertex,
            param_range: Some([start, end]),
            tolerance: None,
        });
        let _attached = ir.model.add_procedural_curve(
            curve_id,
            ProceduralCurve::new(
                ProceduralCurveId(format!("iges:model:procedural-curve#D{}", entry.sequence)),
                ProceduralCurveDefinition::Offset {
                    source: offset_source_id,
                    distance,
                    support: None,
                    direction: None,
                    normal: Some(normal),
                    parameter_range: Some([start, end]),
                    distance_law,
                },
            ),
        );
        wire_edges.push(edge_id);
        decoded.insert(entry.sequence);
    }

    WireProjectionOutcome {
        decoded,
        losses,
        wire_edges,
    }
}

#[cfg(test)]
mod tests;
