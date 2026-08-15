// SPDX-License-Identifier: Apache-2.0
//! Point and analytic curve entity projection.

use super::curve_conversion::angularly_equal;
use crate::directory::DirectoryEntry;
use crate::global::{Global, RealPrecision};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{knots_nondecreasing, Curve, CurveGeometry, NurbsCurve};
use cadmpeg_ir::ids::{BodyId, CurveId, EdgeId, PointId, RegionId, ShellId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Edge, Point, Region, Shell, Vertex};
use cadmpeg_ir::{CadIr, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

const MAX_TRANSFORM_DEPTH: usize = 64;
const COMPUTATION_TOLERANCE: f64 = 64.0 * f64::EPSILON;

#[derive(Clone, Copy)]
pub(crate) struct DeclaredInterval {
    lower: f64,
    upper: f64,
}

impl DeclaredInterval {
    fn outward(lower: f64, upper: f64) -> Self {
        Self {
            lower: lower.next_down(),
            upper: upper.next_up(),
        }
    }

    pub(crate) fn around(value: f64, uncertainty: f64) -> Self {
        if uncertainty == 0.0 {
            Self {
                lower: value,
                upper: value,
            }
        } else {
            Self::outward(value - uncertainty, value + uncertainty)
        }
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self::outward(self.lower + other.lower, self.upper + other.upper)
    }

    pub(crate) fn subtract(self, other: Self) -> Self {
        Self::outward(self.lower - other.upper, self.upper - other.lower)
    }

    pub(crate) fn multiply(self, other: Self) -> Self {
        let products = [
            self.lower * other.lower,
            self.lower * other.upper,
            self.upper * other.lower,
            self.upper * other.upper,
        ];
        Self::outward(
            products.into_iter().fold(f64::INFINITY, f64::min),
            products.into_iter().fold(f64::NEG_INFINITY, f64::max),
        )
    }

    pub(crate) fn scale(self, factor: f64) -> Self {
        self.multiply(Self::around(factor, 0.0))
    }

    pub(crate) fn contains(self, value: f64) -> bool {
        self.lower <= value && value <= self.upper
    }

    pub(crate) fn overlaps(self, other: Self) -> bool {
        self.lower <= other.upper && other.lower <= self.upper
    }
}

pub(crate) fn declared_unit_vector(
    record: &ParameterRecord,
    start: usize,
    vector: Vector3,
    precision: RealPrecision,
) -> bool {
    if !vector.norm().is_finite() {
        return false;
    }
    let values = [vector.x, vector.y, vector.z];
    let components = std::array::from_fn::<_, 3, _>(|offset| {
        DeclaredInterval::around(
            values[offset],
            record.number_uncertainty(start + offset, values[offset], precision),
        )
    });
    components
        .into_iter()
        .fold(DeclaredInterval::around(0.0, 0.0), |sum, component| {
            sum.add(component.multiply(component))
        })
        .contains(1.0)
}

pub(crate) fn declared_orthogonal_vectors(
    record: &ParameterRecord,
    left_start: usize,
    left: Vector3,
    right_start: usize,
    right: Vector3,
    precision: RealPrecision,
) -> bool {
    let left_values = [left.x, left.y, left.z];
    let right_values = [right.x, right.y, right.z];
    (0..3)
        .fold(DeclaredInterval::around(0.0, 0.0), |sum, offset| {
            let left = DeclaredInterval::around(
                left_values[offset],
                record.number_uncertainty(left_start + offset, left_values[offset], precision),
            );
            let right = DeclaredInterval::around(
                right_values[offset],
                record.number_uncertainty(right_start + offset, right_values[offset], precision),
            );
            sum.add(left.multiply(right))
        })
        .contains(0.0)
}

#[derive(Clone, Copy)]
pub(crate) struct Affine {
    pub(crate) rows: [[f64; 4]; 3],
}

impl Affine {
    pub(crate) const IDENTITY: Self = Self {
        rows: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    };

    pub(crate) fn compose(self, local: Self) -> Self {
        let mut rows = [[0.0; 4]; 3];
        for (row, values) in rows.iter_mut().enumerate() {
            for (column, value) in values.iter_mut().enumerate().take(3) {
                *value = (0..3)
                    .map(|index| self.rows[row][index] * local.rows[index][column])
                    .sum();
            }
            values[3] = self.rows[row][3]
                + (0..3)
                    .map(|index| self.rows[row][index] * local.rows[index][3])
                    .sum::<f64>();
        }
        Self { rows }
    }

    pub(super) fn point(self, point: Point3) -> Point3 {
        let values = [point.x, point.y, point.z];
        let coordinate = |row: usize| {
            self.rows[row][3]
                + values
                    .iter()
                    .enumerate()
                    .map(|(column, value)| self.rows[row][column] * value)
                    .sum::<f64>()
        };
        Point3::new(coordinate(0), coordinate(1), coordinate(2))
    }

    pub(super) fn vector(self, vector: Vector3) -> Vector3 {
        let values = [vector.x, vector.y, vector.z];
        let coordinate = |row: usize| {
            values
                .iter()
                .enumerate()
                .map(|(column, value)| self.rows[row][column] * value)
                .sum::<f64>()
        };
        Vector3::new(coordinate(0), coordinate(1), coordinate(2))
    }

    pub(super) fn body_transform(self) -> cadmpeg_ir::transform::Transform {
        cadmpeg_ir::transform::Transform {
            rows: [
                self.rows[0],
                self.rows[1],
                self.rows[2],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

pub(crate) fn resolve_transform(
    sequence: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: f64,
    precision: RealPrecision,
    path: &mut BTreeSet<u32>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Affine, String> {
    if sequence == 0 {
        return Ok(Affine::IDENTITY);
    }
    let sequence = u32::try_from(sequence)
        .map_err(|_| "transformation pointer is not a positive sequence".to_string())?;
    if sequence % 2 == 0 {
        return Err("transformation pointer names an even Directory sequence".into());
    }
    let _nested = ctx
        .map(|ctx| ctx.enter_nested("iges_transform_chain", None))
        .transpose()
        .map_err(|error| error.to_string())?;
    let depth_limit = ctx
        .and_then(|ctx| usize::try_from(ctx.policy().limits.max_recursion_depth).ok())
        .map_or(MAX_TRANSFORM_DEPTH, |policy| {
            policy.min(MAX_TRANSFORM_DEPTH)
        });
    if path.len() >= depth_limit {
        return Err(format!(
            "transformation chain exceeds {MAX_TRANSFORM_DEPTH} entities"
        ));
    }
    if !path.insert(sequence) {
        return Err("transformation chain is cyclic".into());
    }
    let result = (|| {
        let entry = entries
            .get(&sequence)
            .copied()
            .ok_or_else(|| format!("transformation D{sequence} is missing"))?;
        if entry.entity_type != 124 || !matches!(entry.form, 0 | 1) {
            return Err(format!(
                "transformation D{sequence} is type {} form {}, expected defining type 124 form 0 or 1",
                entry.entity_type, entry.form
            ));
        }
        let record = records
            .get(&sequence)
            .copied()
            .ok_or_else(|| format!("transformation D{sequence} parameters are missing"))?;
        let mut values = [0.0; 12];
        for (index, value) in values.iter_mut().enumerate() {
            *value = record.number(index + 1).ok_or_else(|| {
                format!(
                    "transformation D{sequence} coefficient {} is not numeric",
                    index + 1
                )
            })?;
            if !value.is_finite() {
                return Err(format!(
                    "transformation D{sequence} has a non-finite coefficient"
                ));
            }
        }
        for index in [3, 7, 11] {
            values[index] *= length_factor;
        }
        let coefficient_intervals = std::array::from_fn::<_, 9, _>(|offset| {
            let row = offset / 3;
            let column = offset % 3;
            let value_index = row * 4 + column;
            DeclaredInterval::around(
                values[value_index],
                record.number_uncertainty(value_index + 1, values[value_index], precision),
            )
        });
        let interval = |row: usize, column: usize| coefficient_intervals[row * 3 + column];
        let column_dot_interval = |left: usize, right: usize| {
            (0..3).fold(DeclaredInterval::around(0.0, 0.0), |sum, row| {
                sum.add(interval(row, left).multiply(interval(row, right)))
            })
        };
        if (0..3).any(|column| !column_dot_interval(column, column).contains(1.0))
            || [(0, 1), (0, 2), (1, 2)]
                .into_iter()
                .any(|(left, right)| !column_dot_interval(left, right).contains(0.0))
        {
            return Err(format!(
                "transformation D{sequence} linear part is not orthonormal within its declared numeric precision"
            ));
        }
        let determinant_interval = interval(0, 0)
            .multiply(
                interval(1, 1)
                    .multiply(interval(2, 2))
                    .subtract(interval(1, 2).multiply(interval(2, 1))),
            )
            .subtract(
                interval(0, 1).multiply(
                    interval(1, 0)
                        .multiply(interval(2, 2))
                        .subtract(interval(1, 2).multiply(interval(2, 0))),
                ),
            )
            .add(
                interval(0, 2).multiply(
                    interval(1, 0)
                        .multiply(interval(2, 1))
                        .subtract(interval(1, 1).multiply(interval(2, 0))),
                ),
            );
        let expected_determinant = if entry.form == 0 { 1.0 } else { -1.0 };
        if !determinant_interval.contains(expected_determinant) {
            return Err(format!(
                "transformation D{sequence} determinant disagrees with form {} within its declared numeric precision",
                entry.form
            ));
        }

        let raw_columns = [
            Vector3::new(values[0], values[4], values[8]),
            Vector3::new(values[1], values[5], values[9]),
            Vector3::new(values[2], values[6], values[10]),
        ];
        let first = {
            let v = raw_columns[0];
            let n = v.norm();
            (n.is_finite() && n > 0.0).then(|| v.scale(1.0 / n))
        }
        .ok_or_else(|| format!("transformation D{sequence} first axis cannot be normalized"))?;
        let second_projection = first.dot(raw_columns[1]);
        let second_residual = raw_columns[1] - first.scale(second_projection);
        let second = {
            let n = second_residual.norm();
            (n.is_finite() && n > 0.0).then(|| second_residual.scale(1.0 / n))
        }
        .ok_or_else(|| format!("transformation D{sequence} second axis cannot be normalized"))?;
        let perpendicular = first.cross(second);
        let third = perpendicular.scale(expected_determinant);
        let local = Affine {
            rows: [
                [first.x, second.x, third.x, values[3]],
                [first.y, second.y, third.y, values[7]],
                [first.z, second.z, third.z, values[11]],
            ],
        };
        let parent = resolve_transform(
            entry.transform,
            entries,
            records,
            length_factor,
            precision,
            path,
            ctx,
        )?;
        Ok(parent.compose(local))
    })();
    path.remove(&sequence);
    result
}

pub(crate) struct Projection {
    pub(crate) handled: BTreeSet<u32>,
    pub(crate) decoded: BTreeSet<u32>,
    pub(crate) losses: Vec<LossNote>,
}

fn admit_projected_entities(
    ctx: Option<&DecodeContext<'_>>,
    ir: &CadIr,
    admitted: &mut u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| {
        ctx.admit_entities(ir.model.entity_count() as u64, admitted, operation)
    })
}

pub(super) fn entity_loss(entry: &DirectoryEntry, message: impl Into<String>) -> LossNote {
    IgesLossCode::EntityNotProjected
        .note(format!(
            "IGES entity type {} form {} was not projected: {}",
            entry.entity_type,
            entry.form,
            message.into()
        ))
        .with_provenance(entry.loss_provenance())
}

pub(super) fn source_object(entry: &DirectoryEntry) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: "iges".into(),
        object_id: format!("D{}", entry.sequence),
        name: std::str::from_utf8(&entry.label)
            .ok()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        color: None,
        visible: Some(entry.status.blank == 0),
        layer: Some(entry.level.to_string()),
        instance_path: Vec::new(),
    }
}

pub(crate) fn project_geometry(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Projection, CodecError> {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    handled.extend(
        directory
            .iter()
            .filter(|entry| entry.entity_type == 124 && matches!(entry.form, 0 | 1 | 10 | 11 | 12))
            .map(|entry| entry.sequence),
    );
    let analytic_surface_locations = directory
        .iter()
        .filter(|entry| {
            matches!(entry.entity_type, 190 | 192 | 194 | 196 | 198) && matches!(entry.form, 0 | 1)
        })
        .filter_map(|entry| {
            records
                .get(&entry.sequence)
                .and_then(|record| record.integer(1))
                .and_then(|value| u32::try_from(value).ok())
        })
        .collect::<BTreeSet<_>>();
    let mut free_vertices = Vec::new();
    let mut wire_edges = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 123 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let components = [record.number(1), record.number(2), record.number(3)];
        let [Some(x), Some(y), Some(z)] = components else {
            losses.push(entity_loss(entry, "direction components are not numeric"));
            continue;
        };
        let direction = Vector3::new(x, y, z);
        if !direction.norm().is_finite() || direction.norm() <= 0.0 {
            losses.push(entity_loss(entry, "direction is zero or non-finite"));
            continue;
        }
        if !entry.status.is_physically_dependent() {
            losses.push(entity_loss(
                entry,
                "Direction Entity is not marked physically dependent",
            ));
            continue;
        }
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "Direction Entity references a prohibited transformation",
            ));
        }
    }
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 100 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let mut values = [0.0; 7];
        let mut malformed = None;
        for (index, value) in values.iter_mut().enumerate() {
            match record.number(index + 1) {
                Some(number) if number.is_finite() => *value = number * factor,
                _ => malformed = Some(index + 1),
            }
        }
        if let Some(index) = malformed {
            losses.push(entity_loss(
                entry,
                format!("arc parameter {index} is not a finite number"),
            ));
            continue;
        }
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
        let basis_x = transform.vector(Vector3::new(1.0, 0.0, 0.0));
        let basis_y = transform.vector(Vector3::new(0.0, 1.0, 0.0));
        let scale_x = basis_x.norm();
        let scale_y = basis_y.norm();
        let scale_tolerance = scale_x.max(scale_y).max(1.0) * COMPUTATION_TOLERANCE;
        if !scale_x.is_finite()
            || !scale_y.is_finite()
            || (scale_x - scale_y).abs() > scale_tolerance
            || basis_x.dot(basis_y).abs() > scale_x * scale_y * COMPUTATION_TOLERANCE
        {
            losses.push(entity_loss(
                entry,
                "affine placement does not preserve circular geometry",
            ));
            continue;
        }
        let center = transform.point(Point3::new(values[1], values[2], values[0]));
        let start = transform.point(Point3::new(values[3], values[4], values[0]));
        let end = transform.point(Point3::new(values[5], values[6], values[0]));
        let start_delta = start.vector_from(center);
        let end_delta = end.vector_from(center);
        let radius = start_delta.norm();
        let end_radius = end_delta.norm();
        let Some(ref_direction) = ({
            let n = start_delta.norm();
            (n.is_finite() && n > 0.0).then(|| start_delta.scale(1.0 / n))
        }) else {
            losses.push(entity_loss(entry, "arc start point equals its center"));
            continue;
        };
        let Some(axis) = ({
            let v = basis_x.cross(basis_y);
            let n = v.norm();
            (n.is_finite() && n > 0.0).then(|| v.scale(1.0 / n))
        }) else {
            losses.push(entity_loss(entry, "arc placement collapses its plane"));
            continue;
        };
        let radius_tolerance = global
            .minimum_resolution_mm()
            .max(radius.max(end_radius).max(1.0) * COMPUTATION_TOLERANCE);
        if !end_radius.is_finite() || (end_radius - radius).abs() > radius_tolerance {
            losses.push(entity_loss(
                entry,
                "arc start and terminate points have different radii",
            ));
            continue;
        }
        let Some(end_direction) = ({
            let n = end_delta.norm();
            (n.is_finite() && n > 0.0).then(|| end_delta.scale(1.0 / n))
        }) else {
            losses.push(entity_loss(entry, "arc terminate point equals its center"));
            continue;
        };
        let mut angle = axis
            .dot(ref_direction.cross(end_direction))
            .atan2(ref_direction.dot(end_direction))
            .rem_euclid(std::f64::consts::TAU);
        if angularly_equal(angle, 0.0) {
            angle = std::f64::consts::TAU;
        }
        let stem = format!("D{}", entry.sequence);
        let start_point = PointId(format!("iges:model:point#{stem}-start"));
        let end_point = PointId(format!("iges:model:point#{stem}-end"));
        let start_vertex = VertexId(format!("iges:model:vertex#{stem}-start"));
        let end_vertex = VertexId(format!("iges:model:vertex#{stem}-end"));
        let curve = CurveId(format!("iges:model:curve#{stem}"));
        let edge = EdgeId(format!("iges:model:edge#{stem}"));
        ir.model.points.extend([
            Point {
                source_object: None,
                id: start_point.clone(),
                position: start,
            },
            Point {
                source_object: None,
                id: end_point.clone(),
                position: end,
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
            id: curve.clone(),
            geometry: CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            },
            source_object: Some(source_object(entry)),
        });
        ir.model.edges.push(Edge {
            id: edge.clone(),
            curve: Some(curve),
            start: start_vertex,
            end: end_vertex,
            param_range: Some([0.0, angle]),
            tolerance: None,
        });
        wire_edges.push(edge);
        decoded.insert(entry.sequence);
    }
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 116 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let coordinates = [record.number(1), record.number(2), record.number(3)];
        let [Some(x), Some(y), Some(z)] = coordinates else {
            losses.push(entity_loss(entry, "X, Y, or Z is not numeric"));
            continue;
        };
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
        let position = transform.point(Point3::new(x * factor, y * factor, z * factor));
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            losses.push(entity_loss(entry, "scaled coordinates are not finite"));
            continue;
        }
        let point = PointId(format!("iges:model:point#D{}", entry.sequence));
        ir.model.points.push(Point {
            source_object: None,
            id: point.clone(),
            position,
        });
        if entry.status.subordinate == 0 || !analytic_surface_locations.contains(&entry.sequence) {
            let vertex = VertexId(format!("iges:model:vertex#D{}", entry.sequence));
            ir.model.vertices.push(Vertex {
                id: vertex.clone(),
                point,
                tolerance: None,
            });
            free_vertices.push(vertex);
        }
        decoded.insert(entry.sequence);
    }
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 110 && (0..=2).contains(&entry.form))
    {
        handled.insert(entry.sequence);
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let mut coordinates = [0.0; 6];
        let mut malformed = None;
        for (index, coordinate) in coordinates.iter_mut().enumerate() {
            match record.number(index + 1) {
                Some(value) if value.is_finite() => *coordinate = value * factor,
                _ => malformed = Some(index + 1),
            }
        }
        if let Some(index) = malformed {
            losses.push(entity_loss(
                entry,
                format!("endpoint coordinate {index} is not a finite number"),
            ));
            continue;
        }
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
        let start = transform.point(Point3::new(coordinates[0], coordinates[1], coordinates[2]));
        let end = transform.point(Point3::new(coordinates[3], coordinates[4], coordinates[5]));
        let delta = end.vector_from(start);
        let length = delta.norm();
        if !length.is_finite() || length <= 0.0 {
            losses.push(entity_loss(
                entry,
                "transformed endpoints are coincident or non-finite",
            ));
            continue;
        }
        let stem = format!("D{}", entry.sequence);
        let curve = CurveId(format!("iges:model:curve#{stem}"));
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Line {
                origin: start,
                direction: Vector3::new(delta.x / length, delta.y / length, delta.z / length),
            },
            source_object: Some(source_object(entry)),
        });
        if entry.form != 0 {
            decoded.insert(entry.sequence);
            continue;
        }
        let start_point = PointId(format!("iges:model:point#{stem}-start"));
        let end_point = PointId(format!("iges:model:point#{stem}-end"));
        let start_vertex = VertexId(format!("iges:model:vertex#{stem}-start"));
        let end_vertex = VertexId(format!("iges:model:vertex#{stem}-end"));
        let edge = EdgeId(format!("iges:model:edge#{stem}"));
        ir.model.points.extend([
            Point {
                source_object: None,
                id: start_point.clone(),
                position: start,
            },
            Point {
                source_object: None,
                id: end_point.clone(),
                position: end,
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
        ir.model.edges.push(Edge {
            id: edge.clone(),
            curve: Some(curve),
            start: start_vertex,
            end: end_vertex,
            param_range: Some([0.0, length]),
            tolerance: None,
        });
        wire_edges.push(edge);
        decoded.insert(entry.sequence);
    }
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 126 && (0..=5).contains(&entry.form))
    {
        handled.insert(entry.sequence);
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(k) = record.count(1) else {
            losses.push(entity_loss(entry, "upper control-point index K is invalid"));
            continue;
        };
        let Some(degree) = record
            .integer(2)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "basis degree M is invalid"));
            continue;
        };
        let degree_usize = usize::try_from(degree).unwrap_or(usize::MAX);
        if degree == 0 || k < degree_usize {
            losses.push(entity_loss(
                entry,
                "control-point count is smaller than degree plus one",
            ));
            continue;
        }
        let flags = [
            record.integer(3),
            record.integer(4),
            record.integer(5),
            record.integer(6),
        ];
        if flags.iter().any(|flag| !matches!(flag, Some(0 | 1))) {
            losses.push(entity_loss(
                entry,
                "one or more spline flags are not 0 or 1",
            ));
            continue;
        }
        let Some(control_count) = k.checked_add(1) else {
            losses.push(entity_loss(entry, "control-point count overflows"));
            continue;
        };
        let Some(knot_count) = control_count
            .checked_add(degree_usize)
            .and_then(|value| value.checked_add(1))
        else {
            losses.push(entity_loss(entry, "knot count overflows"));
            continue;
        };
        let knot_start = 7_usize;
        let Some(weight_start) = knot_start.checked_add(knot_count) else {
            losses.push(entity_loss(entry, "weight offset overflows"));
            continue;
        };
        let Some(pole_start) = weight_start.checked_add(control_count) else {
            losses.push(entity_loss(entry, "control-point offset overflows"));
            continue;
        };
        let Some(pole_value_count) = control_count.checked_mul(3) else {
            losses.push(entity_loss(entry, "control-point value count overflows"));
            continue;
        };
        let Some(range_start) = pole_start.checked_add(pole_value_count) else {
            losses.push(entity_loss(entry, "parameter-range offset overflows"));
            continue;
        };
        let collect_numbers = |start: usize, count: usize| -> Option<Vec<f64>> {
            (start..start.checked_add(count)?)
                .map(|index| record.number(index).filter(|value| value.is_finite()))
                .collect()
        };
        let Some(knots) = collect_numbers(knot_start, knot_count) else {
            losses.push(entity_loss(entry, "knot vector is truncated or non-finite"));
            continue;
        };
        if !knots_nondecreasing(&knots) {
            losses.push(entity_loss(entry, "knot vector is decreasing"));
            continue;
        }
        let Some(native_weights) = collect_numbers(weight_start, control_count) else {
            losses.push(entity_loss(
                entry,
                "weight vector is truncated or non-finite",
            ));
            continue;
        };
        if native_weights.iter().any(|weight| *weight <= 0.0) {
            losses.push(entity_loss(entry, "weights are not strictly positive"));
            continue;
        }
        let precision = global.real_precision();
        let uncertainty =
            |index: usize, value: f64| record.number_uncertainty(index, value, precision);
        let equal_within_significance =
            |left_index: usize, left: f64, right_index: usize, right: f64| {
                (left - right).abs()
                    <= uncertainty(left_index, left) + uncertainty(right_index, right)
            };
        let equal_weights = native_weights.first().is_some_and(|first| {
            native_weights.iter().enumerate().all(|(offset, weight)| {
                equal_within_significance(weight_start, *first, weight_start + offset, *weight)
            })
        });
        let polynomial = flags[2] == Some(1);
        if polynomial && !equal_weights {
            losses.push(entity_loss(entry, "polynomial spline has unequal weights"));
            continue;
        }
        let Some(native_poles) = collect_numbers(pole_start, pole_value_count) else {
            losses.push(entity_loss(
                entry,
                "control-point vector is truncated or non-finite",
            ));
            continue;
        };
        let Some(mut parameter_range) = collect_numbers(range_start, 2) else {
            losses.push(entity_loss(
                entry,
                "parameter range is missing or non-finite",
            ));
            continue;
        };
        let domain_start = knots[degree_usize];
        let domain_end = knots[control_count];
        if parameter_range[0] < domain_start
            && equal_within_significance(
                range_start,
                parameter_range[0],
                knot_start + degree_usize,
                domain_start,
            )
        {
            parameter_range[0] = domain_start;
        }
        if parameter_range[1] > domain_end
            && equal_within_significance(
                range_start + 1,
                parameter_range[1],
                knot_start + control_count,
                domain_end,
            )
        {
            parameter_range[1] = domain_end;
        }
        if parameter_range[0] >= parameter_range[1]
            || parameter_range[0] < domain_start
            || parameter_range[1] > domain_end
        {
            losses.push(entity_loss(
                entry,
                "parameter range lies outside the spline knot domain",
            ));
            continue;
        }
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
        let control_points = native_poles
            .chunks_exact(3)
            .map(|point| {
                transform.point(Point3::new(
                    point[0] * factor,
                    point[1] * factor,
                    point[2] * factor,
                ))
            })
            .collect::<Vec<_>>();
        let weights = (!polynomial).then_some(native_weights);
        let nurbs = NurbsCurve {
            degree,
            knots,
            control_points,
            weights,
            periodic: flags[3] == Some(1),
        };
        let Some(start) = cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            parameter_range[0],
        ) else {
            losses.push(entity_loss(entry, "spline start point cannot be evaluated"));
            continue;
        };
        let Some(end) = cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            parameter_range[1],
        ) else {
            losses.push(entity_loss(entry, "spline end point cannot be evaluated"));
            continue;
        };
        let stem = format!("D{}", entry.sequence);
        let start_point = PointId(format!("iges:model:point#{stem}-start"));
        let end_point = PointId(format!("iges:model:point#{stem}-end"));
        let start_vertex = VertexId(format!("iges:model:vertex#{stem}-start"));
        let end_vertex = VertexId(format!("iges:model:vertex#{stem}-end"));
        let curve = CurveId(format!("iges:model:curve#{stem}"));
        let edge = EdgeId(format!("iges:model:edge#{stem}"));
        ir.model.points.extend([
            Point {
                source_object: None,
                id: start_point.clone(),
                position: start,
            },
            Point {
                source_object: None,
                id: end_point.clone(),
                position: end,
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
            id: curve.clone(),
            geometry: CurveGeometry::Nurbs(nurbs),
            source_object: Some(source_object(entry)),
        });
        ir.model.edges.push(Edge {
            id: edge.clone(),
            curve: Some(curve),
            start: start_vertex,
            end: end_vertex,
            param_range: Some([parameter_range[0], parameter_range[1]]),
            tolerance: None,
        });
        wire_edges.push(edge);
        decoded.insert(entry.sequence);
    }
    let mut admitted_entities = 0;
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_primitives")?;
    let conics = super::conics::project(ir, directory, parameters, global, ctx);
    handled.extend(conics.handled);
    decoded.extend(conics.decoded);
    losses.extend(conics.losses);
    wire_edges.extend(conics.wire_edges);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_conics")?;
    let copious = super::copious::project(ir, directory, parameters, global, ctx)?;
    handled.extend(copious.handled);
    decoded.extend(copious.decoded);
    losses.extend(copious.losses);
    wire_edges.extend(copious.wire_edges);
    free_vertices.extend(copious.free_vertices);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_copious")?;
    let splines = super::splines::project(ir, directory, parameters, global, ctx)?;
    handled.extend(splines.handled);
    decoded.extend(splines.decoded);
    losses.extend(splines.losses);
    wire_edges.extend(splines.wire_edges);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_splines")?;
    let composites = super::composite::project(ir, directory, parameters, global, ctx)?;
    handled.extend(composites.handled);
    decoded.extend(composites.decoded);
    losses.extend(composites.losses);
    wire_edges.extend(composites.wire_edges);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_composites")?;
    let offsets = super::offsets::project(ir, directory, parameters, global, ctx);
    handled.extend(offsets.handled);
    decoded.extend(offsets.decoded);
    losses.extend(offsets.losses);
    wire_edges.extend(offsets.wire_edges);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_offsets")?;
    let analytic_surfaces =
        super::analytic_surfaces::project(ir, directory, parameters, global, ctx);
    handled.extend(analytic_surfaces.handled);
    decoded.extend(analytic_surfaces.decoded);
    losses.extend(analytic_surfaces.losses);
    admit_projected_entities(
        ctx,
        ir,
        &mut admitted_entities,
        "iges_geometry_analytic_surfaces",
    )?;
    let surfaces = super::surfaces::project(ir, directory, parameters, global, ctx)?;
    handled.extend(surfaces.handled);
    decoded.extend(surfaces.decoded);
    losses.extend(surfaces.losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_surfaces")?;
    if !wire_edges.is_empty() || !free_vertices.is_empty() {
        let body = BodyId("iges:model:body#free-geometry".into());
        let region = RegionId("iges:model:region#free-geometry".into());
        let shell = ShellId("iges:model:shell#free-geometry".into());
        ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Wire,
            regions: vec![region.clone()],
            transform: None,
            name: Some("IGES free geometry".into()),
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region.clone(),
            body,
            shells: vec![shell.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell,
            region,
            faces: Vec::new(),
            wire_edges,
            free_vertices,
        });
    }
    admit_projected_entities(
        ctx,
        ir,
        &mut admitted_entities,
        "iges_geometry_wire_topology",
    )?;
    let trimming = super::trimming::project(ir, directory, parameters, global, ctx);
    handled.extend(trimming.handled);
    decoded.extend(trimming.decoded);
    losses.extend(trimming.losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_trimming")?;
    let brep = super::brep::project(ir, directory, parameters, global, ctx);
    handled.extend(brep.handled);
    decoded.extend(brep.decoded);
    losses.extend(brep.losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_brep")?;
    let csg = super::csg::project(ir, directory, parameters, global, ctx);
    handled.extend(csg.handled);
    decoded.extend(csg.decoded);
    losses.extend(csg.losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_csg")?;
    let structure = super::structure::project(ir, directory, parameters, global, ctx);
    handled.extend(structure.handled);
    decoded.extend(structure.decoded);
    losses.extend(structure.losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_structure")?;
    let presentation = super::presentation::project(ir, directory, parameters, global, ctx);
    handled.extend(presentation.handled);
    decoded.extend(presentation.decoded);
    losses.extend(presentation.losses);
    admit_projected_entities(
        ctx,
        ir,
        &mut admitted_entities,
        "iges_geometry_presentation",
    )?;
    let drawing = super::drawing::project(ir, directory, parameters, global, ctx);
    handled.extend(drawing.handled);
    decoded.extend(drawing.decoded);
    losses.extend(drawing.losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_drawing")?;
    let annotation = super::annotation::project(ir, directory, parameters, global, ctx);
    handled.extend(annotation.handled);
    decoded.extend(annotation.decoded);
    losses.extend(annotation.losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_annotation")?;
    let analytic_surface_points = analytic_surface_locations
        .iter()
        .map(|sequence| PointId(format!("iges:model:point#D{sequence}")))
        .collect::<BTreeSet<_>>();
    let vertex_points = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<BTreeSet<_>>();
    ir.model.points.retain(|point| {
        !analytic_surface_points.contains(&point.id) || vertex_points.contains(&point.id)
    });
    Ok(Projection {
        handled,
        decoded,
        losses,
    })
}

#[cfg(test)]
mod tests;
