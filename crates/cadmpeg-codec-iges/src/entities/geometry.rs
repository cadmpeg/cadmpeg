// SPDX-License-Identifier: Apache-2.0
//! Point and analytic curve entity projection.

use super::curve_conversion::angularly_equal;
use crate::directory::DirectoryEntry;
use crate::global::{GlobalTable, ProjectedGlobal, RealPrecision};
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, TrailingPointerAnalysis};
use cadmpeg_core::decode::{refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{knots_nondecreasing, Curve, CurveGeometry, NurbsCurve};
use cadmpeg_ir::ids::{BodyId, CurveId, EdgeId, PointId, RegionId, ShellId, VertexId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Edge, Point, Region, Shell, Vertex};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{CadIr, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

const MAX_TRANSFORM_DEPTH: usize = 64;
const COMPUTATION_TOLERANCE: f64 = 64.0 * f64::EPSILON;
const CURVE_PLANE_NORMAL_EPSILON: f64 = 1.0e-10;

pub(super) fn planar_polyline_has_self_intersection(points: &[[f64; 2]]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let mut points = points.to_vec();
    let last = points.len() - 1;
    points[last] = points[0];
    for first_index in 0..last {
        for second_index in first_index + 1..last {
            let allowed_endpoint = if second_index == first_index + 1 {
                Some(points[second_index])
            } else if first_index == 0 && second_index + 1 == last {
                Some(points[0])
            } else {
                None
            };
            if planar_segments_intersect_beyond_endpoint(
                [points[first_index], points[first_index + 1]],
                [points[second_index], points[second_index + 1]],
                allowed_endpoint,
            ) {
                return true;
            }
        }
    }
    false
}

pub(super) fn planar_polylines_intersect(first: &[[f64; 2]], second: &[[f64; 2]]) -> bool {
    first
        .windows(2)
        .flat_map(|first_segment| {
            second
                .windows(2)
                .map(move |second_segment| [first_segment, second_segment])
        })
        .any(|segments| {
            planar_segments_intersect_beyond_endpoint(
                [segments[0][0], segments[0][1]],
                [segments[1][0], segments[1][1]],
                None,
            )
        })
}

pub(super) fn planar_segments_contain_point(point: [f64; 2], segment: [[f64; 2]; 2]) -> bool {
    planar_point_on_segment(point, segment[0], segment[1])
}

pub(super) fn plane_coordinates(
    points: &[Point3],
    plane: (Point3, Vector3),
) -> Option<Vec<[f64; 2]>> {
    let normal = plane.1.unit()?;
    let reference = if normal.x.abs() <= normal.y.abs() && normal.x.abs() <= normal.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if normal.y.abs() <= normal.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let u_axis = normal.cross(reference).unit()?;
    let v_axis = normal.cross(u_axis).unit()?;
    let coordinates = points
        .iter()
        .map(|point| {
            let displacement = point.vector_from(plane.0);
            [displacement.dot(u_axis), displacement.dot(v_axis)]
        })
        .collect::<Vec<_>>();
    coordinates
        .iter()
        .flatten()
        .all(|coordinate| coordinate.is_finite())
        .then_some(coordinates)
}

pub(super) fn linear_nurbs_parameters(
    degree: u32,
    knots: &[f64],
    control_count: usize,
    periodic: bool,
    range: [f64; 2],
) -> Option<Vec<f64>> {
    let degree = usize::try_from(degree).ok()?;
    let expected_knot_count = control_count.checked_add(degree)?.checked_add(1)?;
    if periodic
        || degree != 1
        || control_count < 2
        || knots.len() != expected_knot_count
        || !knots.windows(2).all(|pair| pair[0] <= pair[1])
        || knots.iter().any(|knot| !knot.is_finite())
        || !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
    {
        return None;
    }
    let domain = [knots[degree], knots[control_count]];
    if range[0] < domain[0] || range[1] > domain[1] {
        return None;
    }
    if knots
        .windows(2)
        .any(|pair| pair[0] == pair[1] && range[0] < pair[0] && pair[0] < range[1])
    {
        return None;
    }
    let mut parameters = vec![range[0]];
    for knot in knots.iter().copied() {
        if knot > range[0] && knot < range[1] && parameters.last().is_none_or(|last| *last != knot)
        {
            parameters.push(knot);
        }
    }
    parameters.push(range[1]);
    Some(parameters)
}

fn planar_cross(left: [f64; 2], right: [f64; 2], point: [f64; 2]) -> f64 {
    (right[0] - left[0]) * (point[1] - left[1]) - (right[1] - left[1]) * (point[0] - left[0])
}

fn planar_point_on_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> bool {
    planar_cross(start, end, point) == 0.0
        && point[0] >= start[0].min(end[0])
        && point[0] <= start[0].max(end[0])
        && point[1] >= start[1].min(end[1])
        && point[1] <= start[1].max(end[1])
}

fn planar_segments_intersect_beyond_endpoint(
    first: [[f64; 2]; 2],
    second: [[f64; 2]; 2],
    allowed_endpoint: Option<[f64; 2]>,
) -> bool {
    let [a, b] = first;
    let [c, d] = second;
    let orientations = [
        planar_cross(a, b, c),
        planar_cross(a, b, d),
        planar_cross(c, d, a),
        planar_cross(c, d, b),
    ];
    let opposite =
        |left: f64, right: f64| (left > 0.0 && right < 0.0) || (left < 0.0 && right > 0.0);
    if opposite(orientations[0], orientations[1]) && opposite(orientations[2], orientations[3]) {
        return true;
    }

    let mut contacts = [(c, orientations[0]), (d, orientations[1])]
        .into_iter()
        .filter_map(|(point, orientation)| {
            (orientation == 0.0 && planar_point_on_segment(point, a, b)).then_some(point)
        })
        .chain(
            [(a, orientations[2]), (b, orientations[3])]
                .into_iter()
                .filter_map(|(point, orientation)| {
                    (orientation == 0.0 && planar_point_on_segment(point, c, d)).then_some(point)
                }),
        );
    contacts.any(|point| Some(point) != allowed_endpoint)
}

pub(crate) fn point_display_symbol_type_allowed(
    entity_type: i64,
    global_table: GlobalTable,
) -> bool {
    match global_table {
        GlobalTable::Legacy => matches!(entity_type, 308 | 408),
        GlobalTable::V4_0 => entity_type == 408,
        GlobalTable::V5_0 | GlobalTable::V5_1 | GlobalTable::V5_2 | GlobalTable::V5_3 => {
            entity_type == 308
        }
    }
}

fn point_display_symbol_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    global_table: GlobalTable,
) -> bool {
    match record.value(4) {
        None | Some(crate::parameter::TokenValue::Omitted) => true,
        Some(crate::parameter::TokenValue::Integer(0)) => true,
        Some(crate::parameter::TokenValue::Integer(sequence)) => {
            u32::try_from(*sequence).ok().is_some_and(|sequence| {
                sequence % 2 == 1
                    && entries.get(&sequence).is_some_and(|target| {
                        target.form == 0
                            && point_display_symbol_type_allowed(target.entity_type, global_table)
                    })
            })
        }
        Some(crate::parameter::TokenValue::Real(_) | crate::parameter::TokenValue::String(_)) => {
            false
        }
    }
}

fn base_geometry_table_entry(entity_type: i64, form: i64) -> bool {
    match entity_type {
        106 => matches!(form, 1..=3 | 11..=13 | 63),
        100 | 104 | 108 | 110 | 112 | 114 | 116 | 118 | 120 | 122 | 126 | 128 | 130 | 140 | 142
        | 144 => true,
        _ => false,
    }
}

fn base_geometry_use_flag_valid(
    entity_type: i64,
    form: i64,
    use_flag: u8,
    global_table: GlobalTable,
) -> bool {
    !base_geometry_table_entry(entity_type, form)
        || !matches!(global_table, GlobalTable::V4_0)
        || matches!(use_flag, 0 | 1 | 2 | 5)
}

fn base_geometry_line_font_required(entity_type: i64, form: i64) -> bool {
    base_geometry_table_entry(entity_type, form)
        && !matches!(entity_type, 116)
        && !(entity_type == 106 && matches!(form, 1..=3))
}

fn base_geometry_line_font_valid(
    entity_type: i64,
    form: i64,
    line_font: i64,
    global_table: GlobalTable,
) -> bool {
    !matches!(global_table, GlobalTable::V4_0)
        || !base_geometry_line_font_required(entity_type, form)
        || line_font != 0
}

#[derive(Clone, Copy)]
enum ControlPointPlane {
    Unique,
    NonPlanar,
    NoUniquePlane,
}

fn classify_control_point_plane(points: &[Point3], tolerance: f64) -> ControlPointPlane {
    let Some(origin) = points.first().copied() else {
        return ControlPointPlane::NoUniquePlane;
    };
    let Some((first_direction, first_length)) = points.iter().skip(1).find_map(|point| {
        let direction = point.vector_from(origin);
        let length = direction.norm();
        (length.is_finite() && length > tolerance).then_some((direction, length))
    }) else {
        return ControlPointPlane::NoUniquePlane;
    };
    let Some(normal) = points.iter().skip(1).find_map(|point| {
        let candidate = first_direction.cross(point.vector_from(origin));
        let length = candidate.norm();
        (length.is_finite() && length > tolerance * first_length).then_some(candidate)
    }) else {
        return ControlPointPlane::NoUniquePlane;
    };
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= 0.0 {
        return ControlPointPlane::NonPlanar;
    }
    let normal = normal.scale(1.0 / normal_length);
    if points
        .iter()
        .skip(1)
        .any(|point| normal.dot(point.vector_from(origin)).abs() > tolerance)
    {
        ControlPointPlane::NonPlanar
    } else {
        ControlPointPlane::Unique
    }
}

fn control_points_fit_plane(points: &[Point3], normal: Vector3, tolerance: f64) -> bool {
    let Some(origin) = points.first().copied() else {
        return false;
    };
    points
        .iter()
        .skip(1)
        .all(|point| normal.dot(point.vector_from(origin)).abs() <= tolerance)
}

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

    pub(crate) fn reciprocal(self) -> Option<Self> {
        if self.contains(0.0) {
            return None;
        }
        let lower = 1.0 / self.lower;
        let upper = 1.0 / self.upper;
        Some(Self::outward(lower.min(upper), lower.max(upper)))
    }

    pub(crate) fn sqrt(self) -> Option<Self> {
        if self.upper < 0.0 {
            return None;
        }
        Some(Self::outward(self.lower.max(0.0).sqrt(), self.upper.sqrt()))
    }

    pub(crate) fn contains(self, value: f64) -> bool {
        self.lower <= value && value <= self.upper
    }

    pub(crate) fn is_finite(self) -> bool {
        self.lower.is_finite() && self.upper.is_finite()
    }

    pub(crate) fn is_strictly_positive(self) -> bool {
        self.lower > 0.0
    }

    pub(crate) fn lower_bound(self) -> f64 {
        self.lower
    }

    pub(crate) fn upper_bound(self) -> f64 {
        self.upper
    }

    pub(crate) fn overlaps(self, other: Self) -> bool {
        self.lower <= other.upper && other.lower <= self.upper
    }
}

/// Return declared intervals for the Type 126 pole coordinates.
///
/// The projected NURBS stores coordinates after unit conversion and placement.
/// A later consumer that needs to distinguish source uncertainty from arithmetic
/// roundoff can use this source-space representation before composite-curve
/// degree elevation and concatenation alter the control polygon.
pub(super) fn type126_declared_control_points(
    record: &ParameterRecord,
    precision: RealPrecision,
) -> Option<Vec<[DeclaredInterval; 3]>> {
    let control_count = record.count(1)?.checked_add(1)?;
    let degree = usize::try_from(record.integer(2)?).ok()?;
    let knot_count = control_count.checked_add(degree)?.checked_add(1)?;
    let weight_start = 7usize.checked_add(knot_count)?;
    let pole_start = weight_start.checked_add(control_count)?;
    let pole_value_count = control_count.checked_mul(3)?;
    let range_start = pole_start.checked_add(pole_value_count)?;
    if record.parameter_end() < range_start.checked_add(2)? {
        return None;
    }
    (0..control_count)
        .map(|point| {
            (0..3)
                .map(|coordinate| {
                    let index = pole_start
                        .checked_add(point.checked_mul(3)?)?
                        .checked_add(coordinate)?;
                    let value = record.number(index).filter(|value| value.is_finite())?;
                    Some(DeclaredInterval::around(
                        value,
                        record.number_uncertainty(index, value, precision),
                    ))
                })
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()
        })
        .collect()
}

/// Return whether finite declared intervals prove one affine progression.
///
/// A sequence `x[i] = a + i*d` is affine exactly when one value of `d` makes
/// all intervals `[x[i] - i*d]` overlap. Pairwise bounds on `d` express that
/// condition without choosing a representative from any source interval. A
/// non-finite interval or bound is rejected instead of being treated as an
/// unconstrained value after arithmetic overflow.
pub(crate) fn declared_affine_progression(values: &[f64], uncertainties: &[f64]) -> bool {
    if values.len() < 2
        || values.len() != uncertainties.len()
        || values
            .iter()
            .zip(uncertainties)
            .any(|(value, uncertainty)| {
                !value.is_finite() || !uncertainty.is_finite() || *uncertainty < 0.0
            })
    {
        return false;
    }
    let intervals = values
        .iter()
        .zip(uncertainties)
        .map(|(value, uncertainty)| DeclaredInterval::around(*value, *uncertainty))
        .collect::<Vec<_>>();
    if intervals
        .iter()
        .any(|interval| !interval.lower.is_finite() || !interval.upper.is_finite())
    {
        return false;
    }
    let mut lower = f64::NEG_INFINITY;
    let mut upper = f64::INFINITY;
    for first in 0..intervals.len() {
        for second in first + 1..intervals.len() {
            let span = (second - first) as f64;
            let pair_lower = (intervals[second].lower - intervals[first].upper) / span;
            let pair_upper = (intervals[second].upper - intervals[first].lower) / span;
            if !pair_lower.is_finite() || !pair_upper.is_finite() {
                return false;
            }
            lower = lower.max(pair_lower);
            upper = upper.min(pair_upper);
        }
    }
    lower <= upper
}

fn interval_dot(left: [DeclaredInterval; 3], right: [DeclaredInterval; 3]) -> DeclaredInterval {
    (0..3).fold(DeclaredInterval::around(0.0, 0.0), |sum, index| {
        sum.add(left[index].multiply(right[index]))
    })
}

fn interval_squared_norm(components: [DeclaredInterval; 3]) -> DeclaredInterval {
    interval_dot(components, components)
}

fn is_finite_nonzero_vector(vector: Vector3) -> bool {
    vector.x.is_finite()
        && vector.y.is_finite()
        && vector.z.is_finite()
        && (vector.x != 0.0 || vector.y != 0.0 || vector.z != 0.0)
}

pub(crate) fn declared_unit_vector(
    record: &ParameterRecord,
    start: usize,
    vector: Vector3,
    precision: RealPrecision,
) -> bool {
    // CADIR admission for an IGES unit-vector field uses its declared-real
    // interval; IGES defines no separate receiver epsilon.
    if !is_finite_nonzero_vector(vector) {
        return false;
    }
    let values = [vector.x, vector.y, vector.z];
    let components = std::array::from_fn::<_, 3, _>(|offset| {
        DeclaredInterval::around(
            values[offset],
            record.number_uncertainty(start + offset, values[offset], precision),
        )
    });
    interval_squared_norm(components).contains(1.0)
}

pub(crate) fn declared_orthogonal_vectors(
    record: &ParameterRecord,
    left_start: usize,
    left: Vector3,
    right_start: usize,
    right: Vector3,
    precision: RealPrecision,
) -> bool {
    // The same CADIR policy admits an orthogonal pair when its dot-product
    // interval contains zero.
    let left_values = [left.x, left.y, left.z];
    let right_values = [right.x, right.y, right.z];
    let left = std::array::from_fn::<_, 3, _>(|offset| {
        DeclaredInterval::around(
            left_values[offset],
            record.number_uncertainty(left_start + offset, left_values[offset], precision),
        )
    });
    let right = std::array::from_fn::<_, 3, _>(|offset| {
        DeclaredInterval::around(
            right_values[offset],
            record.number_uncertainty(right_start + offset, right_values[offset], precision),
        )
    });
    interval_dot(left, right).contains(0.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclaredTransformFrameError {
    NotOrthonormal,
    WrongDeterminant,
}

fn validate_declared_transform_frame(
    coefficient_intervals: [DeclaredInterval; 9],
    expected_determinant: f64,
) -> Result<(), DeclaredTransformFrameError> {
    // CADIR admission for the IGES Type 124 invariants uses declared-real
    // intervals; IGES does not define a separate receiver epsilon.
    let columns = std::array::from_fn::<_, 3, _>(|column| {
        [
            coefficient_intervals[column],
            coefficient_intervals[3 + column],
            coefficient_intervals[6 + column],
        ]
    });
    if columns
        .into_iter()
        .any(|column| !interval_squared_norm(column).contains(1.0))
        || [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .any(|(left, right)| !interval_dot(columns[left], columns[right]).contains(0.0))
    {
        return Err(DeclaredTransformFrameError::NotOrthonormal);
    }

    let interval = |row: usize, column: usize| coefficient_intervals[row * 3 + column];
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
    determinant_interval
        .contains(expected_determinant)
        .then_some(())
        .ok_or(DeclaredTransformFrameError::WrongDeterminant)
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
        let expected_determinant = if entry.form == 0 { 1.0 } else { -1.0 };
        match validate_declared_transform_frame(coefficient_intervals, expected_determinant) {
            Ok(()) => {}
            Err(DeclaredTransformFrameError::NotOrthonormal) => {
                return Err(format!(
                    "transformation D{sequence} linear part is not orthonormal within its declared numeric precision"
                ));
            }
            Err(DeclaredTransformFrameError::WrongDeterminant) => {
                return Err(format!(
                    "transformation D{sequence} determinant disagrees with form {} within its declared numeric precision",
                    entry.form
                ));
            }
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

pub(crate) fn enforce_transform_depth(
    directory: &[DirectoryEntry],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(), CodecError> {
    let depth_limit = ctx
        .and_then(|ctx| usize::try_from(ctx.policy().limits.max_recursion_depth).ok())
        .map_or(MAX_TRANSFORM_DEPTH, |policy| {
            policy.min(MAX_TRANSFORM_DEPTH)
        });
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();

    for entry in directory {
        let Some(mut sequence) = u32::try_from(entry.transform)
            .ok()
            .filter(|sequence| sequence % 2 == 1)
        else {
            continue;
        };
        let mut path = BTreeSet::new();
        let mut depth = 0_usize;
        loop {
            if depth >= depth_limit {
                return Err(refuse_local_limit(
                    "iges_transform_depth",
                    depth_limit as u64,
                    depth.saturating_add(1) as u64,
                    None,
                ));
            }
            if !path.insert(sequence) {
                break;
            }
            depth += 1;
            let Some(transform) = entries.get(&sequence).copied() else {
                break;
            };
            if transform.entity_type != 124 || !matches!(transform.form, 0 | 1) {
                break;
            }
            let Some(next) = u32::try_from(transform.transform)
                .ok()
                .filter(|sequence| sequence % 2 == 1)
            else {
                break;
            };
            sequence = next;
        }
    }
    Ok(())
}

/// One sub-projector's result: the directory sequences whose records emitted
/// a neutral entity, and the losses the pass charged. The generic retention
/// pass spares a record only when it is decoded, consumed, or already
/// attributed, so membership in `decoded` is what marks projection success.
pub(super) struct ProjectionOutcome {
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
}

/// One source endpoint that participates in a face-local boundary vertex.
///
/// Boundary sewing is a topology decision. The endpoint coordinate remains
/// source evidence and must not be replaced by the neutral representative.
#[derive(Debug, Clone)]
pub(crate) struct BoundaryVertexSourceEndpoint {
    pub(crate) edge: String,
    pub(crate) endpoint: BoundaryEndpoint,
    pub(crate) position: Point3,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BoundaryEndpoint {
    Start,
    End,
}

/// The complete derivation of one neutral vertex created by boundary sewing.
#[derive(Debug, Clone)]
pub(crate) struct BoundaryVertexDerivation {
    pub(crate) source_entity: String,
    pub(crate) vertex: VertexId,
    pub(crate) representative: Point3,
    pub(crate) tolerance: f64,
    pub(crate) source_endpoints: Vec<BoundaryVertexSourceEndpoint>,
}

// The `merge_into` drains on the outcome types take `self` by value: every
// field a sub-projector returns has to be handed to an accumulator, so an
// outcome field cannot be dropped silently at the merge site. The accumulator
// element types are pairwise distinct, so arguments cannot be transposed.
impl ProjectionOutcome {
    fn merge_into(self, decoded: &mut BTreeSet<u32>, losses: &mut Vec<LossNote>) {
        decoded.extend(self.decoded);
        losses.extend(self.losses);
    }
}

/// A curve projector's result: the two-field outcome extended with the edges
/// the caller collects into the free-geometry wire shell.
pub(super) struct WireProjectionOutcome {
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
    pub(super) wire_edges: Vec<EdgeId>,
}

impl WireProjectionOutcome {
    fn merge_into(
        self,
        decoded: &mut BTreeSet<u32>,
        losses: &mut Vec<LossNote>,
        wire_edges: &mut Vec<EdgeId>,
    ) {
        decoded.extend(self.decoded);
        losses.extend(self.losses);
        wire_edges.extend(self.wire_edges);
    }
}

#[derive(Default)]
pub(crate) struct Projection {
    pub(crate) decoded: BTreeSet<u32>,
    /// Source records consumed as construction data without a standalone
    /// neutral entity. The generic retention pass suppresses its loss for
    /// these records; membership here is the only suppression channel.
    pub(crate) consumed: BTreeSet<u32>,
    pub(crate) losses: Vec<LossNote>,
    pub(crate) boundary_vertex_derivations: Vec<BoundaryVertexDerivation>,
}

fn positive_sequence(value: i64) -> Option<u32> {
    u32::try_from(value)
        .ok()
        .filter(|sequence| sequence % 2 == 1)
}

fn consumed_support_sequences(
    directory: &[DirectoryEntry],
    records: &BTreeMap<u32, &ParameterRecord>,
) -> BTreeSet<u32> {
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut transform_sequences = BTreeSet::new();
    for entry in directory {
        if let Some(sequence) = positive_sequence(entry.transform).filter(|sequence| {
            entries
                .get(sequence)
                .is_some_and(|target| target.entity_type == 124)
        }) {
            transform_sequences.insert(sequence);
        }
    }
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 184 && matches!(entry.form, 0 | 1))
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            continue;
        };
        let Some(count) = record.count(1) else {
            continue;
        };
        for index in 0..count.min(record.parameter_end()) {
            if let Some(sequence) = record
                .integer(2 + count + index)
                .and_then(positive_sequence)
                .filter(|sequence| {
                    entries
                        .get(sequence)
                        .is_some_and(|target| target.entity_type == 124)
                })
            {
                transform_sequences.insert(sequence);
            }
        }
    }
    let mut direction_sequences = BTreeSet::new();
    for entry in directory.iter().filter(|entry| {
        matches!(entry.entity_type, 190 | 192 | 194 | 196 | 198) && matches!(entry.form, 0 | 1)
    }) {
        let Some(record) = records.get(&entry.sequence).copied() else {
            continue;
        };
        let indices: &[usize] = match (entry.entity_type, entry.form) {
            (190 | 192 | 194 | 198, 0) => &[2],
            (190, 1) => &[2, 3],
            (192, 1) => &[2, 4],
            (194 | 198, 1) => &[2, 5],
            (196, 0) => &[],
            (196, 1) => &[3, 4],
            _ => &[],
        };
        for index in indices {
            if let Some(sequence) =
                record
                    .integer(*index)
                    .and_then(positive_sequence)
                    .filter(|sequence| {
                        entries
                            .get(sequence)
                            .is_some_and(|target| target.entity_type == 123 && target.form == 0)
                    })
            {
                direction_sequences.insert(sequence);
            }
        }
    }

    let mut consumed = direction_sequences;
    while let Some(sequence) = transform_sequences.pop_first() {
        if !consumed.insert(sequence) {
            continue;
        }
        let Some(entry) = entries.get(&sequence).copied() else {
            continue;
        };
        if let Some(parent) = positive_sequence(entry.transform).filter(|parent| {
            entries
                .get(parent)
                .is_some_and(|target| target.entity_type == 124)
        }) {
            transform_sequences.insert(parent);
        }
    }
    consumed
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

fn point_on_plane(point: Point3, plane: (Point3, Vector3), resolution: f64) -> bool {
    let distance = point.vector_from(plane.0).dot(plane.1).abs();
    distance.is_finite() && distance <= resolution
}

fn normal_matches_plane(normal: Vector3, plane_normal: Vector3) -> bool {
    let norm = normal.norm();
    norm.is_finite()
        && norm > 0.0
        && normal.scale(1.0 / norm).cross(plane_normal).norm() <= CURVE_PLANE_NORMAL_EPSILON
}

fn direction_in_plane(direction: Vector3, plane_normal: Vector3) -> bool {
    let direction_norm = direction.norm();
    let plane_norm = plane_normal.norm();
    direction_norm.is_finite()
        && direction_norm > 0.0
        && plane_norm.is_finite()
        && plane_norm > 0.0
        && direction
            .scale(1.0 / direction_norm)
            .dot(plane_normal.scale(1.0 / plane_norm))
            .abs()
            <= CURVE_PLANE_NORMAL_EPSILON
}

pub(super) fn curve_geometry_coplanar(
    geometry: &CurveGeometry,
    index: &ModelIndex<'_>,
    transform: Transform,
    plane: (Point3, Vector3),
    resolution: f64,
    active: &mut BTreeSet<CurveId>,
) -> bool {
    let point_valid =
        |point: Point3| point_on_plane(transform.apply_point(point), plane, resolution);
    let normal_valid = |normal: Vector3| {
        transform
            .apply_normal(normal)
            .is_some_and(|normal| normal_matches_plane(normal, plane.1))
    };
    let direction_valid =
        |direction: Vector3| direction_in_plane(transform.apply_vector(direction), plane.1);
    match geometry {
        CurveGeometry::Line { origin, direction } => {
            point_valid(*origin) && direction_valid(*direction)
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            ..
        } => point_valid(*center) && normal_valid(*axis) && direction_valid(*ref_direction),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            ..
        } => point_valid(*center) && normal_valid(*axis) && direction_valid(*major_direction),
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            ..
        } => point_valid(*vertex) && normal_valid(*axis) && direction_valid(*major_direction),
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            ..
        } => point_valid(*center) && normal_valid(*axis) && direction_valid(*major_direction),
        CurveGeometry::Degenerate { point } => point_valid(*point),
        CurveGeometry::Nurbs(curve) => curve.control_points.iter().copied().all(point_valid),
        CurveGeometry::Polyline { points, .. } => points.iter().copied().all(point_valid),
        CurveGeometry::Composite { segments, .. } => segments.iter().all(|segment| {
            let Some(curve) = index.curves(&segment.curve.0) else {
                return false;
            };
            if !active.insert(segment.curve.clone()) {
                return false;
            }
            let valid = curve_geometry_coplanar(
                &curve.geometry,
                index,
                transform,
                plane,
                resolution,
                active,
            );
            active.remove(&segment.curve);
            valid
        }),
        CurveGeometry::Transformed {
            basis,
            transform: map,
        } => curve_geometry_coplanar(
            basis,
            index,
            transform.compose(*map),
            plane,
            resolution,
            active,
        ),
        CurveGeometry::Procedural { .. } | CurveGeometry::Unknown { .. } => false,
    }
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
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Projection, CodecError> {
    let global_table = global.global_table();
    let admitted = |entry: &DirectoryEntry| {
        entry.status.is_use_flag_valid(global_table)
            && base_geometry_use_flag_valid(
                entry.entity_type,
                entry.form,
                entry.status.use_flag,
                global_table,
            )
            && base_geometry_line_font_valid(
                entry.entity_type,
                entry.form,
                entry.line_font,
                global_table,
            )
            && crate::profile::envelope_a_admits(entry.entity_type, entry.form, global_table)
    };
    let mut losses = Vec::new();
    for entry in directory {
        if !entry.status.is_use_flag_valid(global_table) {
            losses.push(entity_loss(
                entry,
                format!(
                    "Entity Use Flag {:02} is outside the declared dialect",
                    entry.status.use_flag
                ),
            ));
        } else if !base_geometry_use_flag_valid(
            entry.entity_type,
            entry.form,
            entry.status.use_flag,
            global_table,
        ) {
            losses.push(entity_loss(
                entry,
                format!(
                    "Entity Use Flag {:02} is outside the IGES 4.0 base geometry values 00, 01, 02, and 05",
                    entry.status.use_flag
                ),
            ));
        } else if !base_geometry_line_font_valid(
            entry.entity_type,
            entry.form,
            entry.line_font,
            global_table,
        ) {
            losses.push(entity_loss(
                entry,
                "Line Font must be nonzero for this IGES 4.0 geometry entity",
            ));
        }
    }
    let admitted_directory = directory.iter().any(|entry| !admitted(entry)).then(|| {
        directory
            .iter()
            .filter(|entry| admitted(entry))
            .cloned()
            .collect::<Vec<_>>()
    });
    let directory = admitted_directory.as_deref().unwrap_or(directory);
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut boundary_vertex_derivations = Vec::new();
    let consumed = consumed_support_sequences(directory, &records);
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
        if !is_finite_nonzero_vector(direction) {
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
        if !point_display_symbol_valid(record, &entries, global.global_table()) {
            losses.push(
                IgesLossCode::DisplayDataNotProjected
                    .note("Type 116 display symbol pointer is invalid for the declared dialect")
                    .with_provenance(entry.loss_provenance()),
            );
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
        .filter(|entry| entry.entity_type == 125 && (0..=4).contains(&entry.form))
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let coordinates = [record.number(1), record.number(2)];
        let [Some(x), Some(y)] = coordinates else {
            losses.push(entity_loss(
                entry,
                "X or Y reference coordinate is not numeric",
            ));
            continue;
        };
        if !x.is_finite() || !y.is_finite() {
            losses.push(entity_loss(
                entry,
                "X or Y reference coordinate is not finite",
            ));
            continue;
        }
        let required_real = |index| record.number(index).is_some_and(f64::is_finite);
        let optional_real = |index| record.number_or(index, 0.0).is_some_and(f64::is_finite);
        let shape_parameters_valid = match entry.form {
            0 => true,
            1 => required_real(3) && optional_real(4) && optional_real(5),
            2 => required_real(3) && required_real(4) && required_real(5),
            3 => required_real(3) && required_real(4) && optional_real(5),
            4 => required_real(3) && required_real(4) && required_real(5),
            _ => false,
        };
        if !shape_parameters_valid {
            losses.push(
                IgesLossCode::DisplayDataNotProjected
                    .note("Type 125 flash shape parameters are incomplete or non-finite")
                    .with_provenance(entry.loss_provenance()),
            );
        }
        if entry.form == 0 && record.integer_or(6, 0).is_none_or(|pointer| pointer == 0) {
            losses.push(
                IgesLossCode::DisplayDataNotProjected
                    .note("Type 125 Form 0 has no defining entity pointer")
                    .with_provenance(entry.loss_provenance()),
            );
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
        let position = transform.point(Point3::new(x * factor, y * factor, 0.0));
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            losses.push(entity_loss(entry, "scaled reference point is not finite"));
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
        if k < degree_usize {
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
        if !polynomial && equal_weights {
            losses.push(entity_loss(
                entry,
                "rational spline has equal weights but PROP3 declares rational",
            ));
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
        if control_points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
        {
            losses.push(entity_loss(
                entry,
                "transformed control-point vector is non-finite",
            ));
            continue;
        }
        let point_scale = control_points
            .iter()
            .skip(1)
            .map(|point| point.distance(control_points[0]))
            .filter(|distance| distance.is_finite())
            .fold(1.0, f64::max);
        let plane_tolerance = global
            .minimum_resolution_mm()
            .max(point_scale * COMPUTATION_TOLERANCE);
        let plane = classify_control_point_plane(&control_points, plane_tolerance);
        let planar = flags[0] == Some(1);
        if planar {
            let Some(normal_start) = range_start.checked_add(2) else {
                losses.push(entity_loss(entry, "plane-normal offset overflows"));
                continue;
            };
            let Some(normal_values) = collect_numbers(normal_start, 3) else {
                losses.push(entity_loss(
                    entry,
                    "plane-normal fields are missing or non-finite",
                ));
                continue;
            };
            let normal_definition =
                Vector3::new(normal_values[0], normal_values[1], normal_values[2]);
            if !declared_unit_vector(record, normal_start, normal_definition, precision) {
                losses.push(entity_loss(
                    entry,
                    "planar spline normal is not a declared unit vector",
                ));
                continue;
            }
            let normal = transform.vector(normal_definition);
            let normal_length = normal.norm();
            if !normal_length.is_finite()
                || normal_length <= 0.0
                || !control_points_fit_plane(
                    &control_points,
                    normal.scale(1.0 / normal_length),
                    plane_tolerance,
                )
                || matches!(plane, ControlPointPlane::NonPlanar)
            {
                losses.push(entity_loss(
                    entry,
                    "planar spline flag disagrees with the control-point geometry",
                ));
                continue;
            }
        } else if matches!(plane, ControlPointPlane::Unique) {
            losses.push(entity_loss(
                entry,
                "non-planar spline flag disagrees with a unique control-point plane",
            ));
            continue;
        }
        let weights = (!polynomial).then_some(native_weights);
        let nurbs = NurbsCurve {
            degree,
            knots,
            control_points,
            weights,
            // IGES PROP4 is informational; neutral evaluation uses the
            // serialized active carrier without periodic parameter wrapping.
            periodic: false,
        };
        let Some(start) = cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            parameter_range[0],
        )
        .filter(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) else {
            losses.push(entity_loss(entry, "spline start point cannot be evaluated"));
            continue;
        };
        let Some(end) = cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            parameter_range[1],
        )
        .filter(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) else {
            losses.push(entity_loss(entry, "spline end point cannot be evaluated"));
            continue;
        };
        let endpoint_distance = start.distance(end);
        let resolution = global.minimum_resolution_mm();
        let closed = endpoint_distance == 0.0 || endpoint_distance < resolution;
        if flags[1] != Some(i64::from(closed)) {
            losses.push(entity_loss(
                entry,
                "closed spline flag disagrees with evaluated endpoints",
            ));
            continue;
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
    // The stanza sequence below is order-sensitive three ways: `losses`,
    // `wire_edges`, and `free_vertices` are ordered vectors that reach the
    // decode report and the free-geometry shell; every `project` call appends
    // to `ir`; and every `admit_projected_entities` call can early-return on
    // the entity budget.
    super::conics::project(ir, directory, parameters, global, ctx).merge_into(
        &mut decoded,
        &mut losses,
        &mut wire_edges,
    );
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_conics")?;
    super::copious::project(ir, directory, parameters, global, ctx)?.merge_into(
        &mut decoded,
        &mut losses,
        &mut wire_edges,
        &mut free_vertices,
    );
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_copious")?;
    super::splines::project(ir, directory, parameters, global, ctx)?.merge_into(
        &mut decoded,
        &mut losses,
        &mut wire_edges,
    );
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_splines")?;
    super::composite::project(ir, directory, parameters, global, ctx)?.merge_into(
        &mut decoded,
        &mut losses,
        &mut wire_edges,
    );
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_composites")?;
    super::offsets::project(ir, directory, parameters, global, ctx).merge_into(
        &mut decoded,
        &mut losses,
        &mut wire_edges,
    );
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_offsets")?;
    // A valid V5 Type 130 constituent is deferred until its exact offset
    // carrier has been projected above. The second composite pass consumes
    // that carrier while retaining each entity's ordered child list.
    super::composite::project_type_130_children(ir, directory, parameters, global, ctx)?
        .merge_into(&mut decoded, &mut losses, &mut wire_edges);
    admit_projected_entities(
        ctx,
        ir,
        &mut admitted_entities,
        "iges_geometry_composites_offsets",
    )?;
    super::analytic_surfaces::project(ir, directory, parameters, global, ctx)
        .merge_into(&mut decoded, &mut losses);
    admit_projected_entities(
        ctx,
        ir,
        &mut admitted_entities,
        "iges_geometry_analytic_surfaces",
    )?;
    super::surfaces::project(ir, directory, parameters, global, ctx)?
        .merge_into(&mut decoded, &mut losses);
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
    let (trimming_projection, trimming_vertex_derivations) =
        super::trimming::project(ir, directory, parameters, global, ctx);
    boundary_vertex_derivations.extend(trimming_vertex_derivations);
    trimming_projection.merge_into(&mut decoded, &mut losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_trimming")?;
    super::brep::project(ir, directory, parameters, global, ctx)
        .merge_into(&mut decoded, &mut losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_brep")?;
    super::csg::project(ir, directory, parameters, global, ctx)
        .merge_into(&mut decoded, &mut losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_csg")?;
    super::structure::project(
        ir,
        directory,
        parameters,
        trailing_pointer_analysis,
        global,
        ctx,
    )
    .merge_into(&mut decoded, &mut losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_structure")?;
    super::presentation::project(ir, directory, parameters, global, ctx)
        .merge_into(&mut decoded, &mut losses);
    admit_projected_entities(
        ctx,
        ir,
        &mut admitted_entities,
        "iges_geometry_presentation",
    )?;
    super::drawing::project(
        ir,
        directory,
        parameters,
        trailing_pointer_analysis,
        global,
        ctx,
    )
    .merge_into(&mut decoded, &mut losses);
    admit_projected_entities(ctx, ir, &mut admitted_entities, "iges_geometry_drawing")?;
    super::annotation::project(ir, directory, parameters, global, ctx)
        .merge_into(&mut decoded, &mut losses);
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
        decoded,
        consumed,
        losses,
        boundary_vertex_derivations,
    })
}

#[cfg(test)]
mod tests;
