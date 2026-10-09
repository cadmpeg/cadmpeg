// SPDX-License-Identifier: Apache-2.0
//! Point and analytic curve entity projection.

use super::curve_conversion::angularly_equal;
use crate::directory::{DirectoryEntry, Subordinate, UseFlag};
use crate::global::{GlobalTable, ProjectedGlobal, RealPrecision};
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, TrailingPointerAnalysis};
use cadmpeg_core::decode::{index_from_u32, u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{KnotVector, NurbsCurve},
    Curve, CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{BodyId, CurveId, EdgeId, FaceId, PointId, SurfaceId, VertexId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal, PositiveLength, PositiveReal};
use cadmpeg_ir::topology::{
    Body, BodyKind, Edge, IncreasingParameterInterval, Point, Region, Shell, Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::{CadIr, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_TRANSFORM_DEPTH: usize = 64;
const COMPUTATION_TOLERANCE: f64 = 64.0 * f64::EPSILON;
const CURVE_PLANE_NORMAL_EPSILON: f64 = 1.0e-10;

pub(super) type ScopedValues<'ctx, T> = (Vec<T>, cadmpeg_core::decode::ScopedReservation<'ctx>);

pub(super) fn planar_polyline_has_self_intersection(
    points: &[[f64; 2]],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if points.len() < 3 {
        return Ok(false);
    }
    let last = points.len() - 1;
    let point_at = |index: usize| {
        if index == last {
            points[0]
        } else {
            points[index]
        }
    };
    ctx.any_by(
        0..last,
        |first_index| {
            ctx.any_by(
                first_index + 1..last,
                |second_index| {
                    let allowed_endpoint = if second_index == first_index + 1 {
                        Some(point_at(second_index))
                    } else if first_index == 0 && second_index + 1 == last {
                        Some(points[0])
                    } else {
                        None
                    };
                    Ok(planar_segments_intersect_beyond_endpoint(
                        [point_at(first_index), point_at(first_index + 1)],
                        [point_at(second_index), point_at(second_index + 1)],
                        allowed_endpoint,
                    ))
                },
                "iges planar self-intersection comparisons",
            )
        },
        "iges planar self-intersection segments",
    )
}

pub(super) fn planar_polylines_intersect(
    first: &[[f64; 2]],
    second: &[[f64; 2]],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    ctx.any_by(
        first.windows(2),
        |first_segment| {
            ctx.any_by(
                second.windows(2),
                |second_segment| {
                    Ok(planar_segments_intersect_beyond_endpoint(
                        [first_segment[0], first_segment[1]],
                        [second_segment[0], second_segment[1]],
                        None,
                    ))
                },
                "iges planar ring intersection comparisons",
            )
        },
        "iges planar ring intersection segments",
    )
}

pub(super) fn closed_polyline_has_duplicate<T>(
    points: &[T],
    coincident: impl Fn(&T, &T) -> bool,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    ctx.any_by(
        points.iter().enumerate(),
        |(first, left)| {
            ctx.any_by(
                points[first + 1..].iter().enumerate(),
                |(offset, right)| {
                    let second = first + 1 + offset;
                    Ok(!(first == 0 && second + 1 == points.len()) && coincident(left, right))
                },
                "iges closed polyline duplicate comparisons",
            )
        },
        "iges closed polyline duplicate points",
    )
}

pub(super) fn planar_segments_contain_point(point: [f64; 2], segment: [[f64; 2]; 2]) -> bool {
    planar_point_on_segment(point, segment[0], segment[1])
}

pub(super) fn plane_coordinates(
    points: &[Point3],
    plane: (Point3, Vector3),
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<[f64; 2]>>, CodecError> {
    let Some(normal) = plane.1.unit() else {
        return Ok(None);
    };
    let reference = if normal.x.abs() <= normal.y.abs() && normal.x.abs() <= normal.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if normal.y.abs() <= normal.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let Some(u_axis) = normal.cross(reference).unit() else {
        return Ok(None);
    };
    let Some(v_axis) = normal.cross(u_axis).unit() else {
        return Ok(None);
    };
    ctx.collect_options(
        points.iter().map(|point| {
            let displacement = point.vector_from(plane.0);
            let coordinates = [displacement.dot(u_axis), displacement.dot(v_axis)];
            coordinates
                .into_iter()
                .all(f64::is_finite)
                .then_some(coordinates)
        }),
        "iges plane coordinates",
    )
}

pub(super) fn linear_nurbs_parameters<'ctx>(
    degree: u32,
    knots: &[f64],
    control_count: usize,
    periodic: bool,
    range: [f64; 2],
    ctx: &'ctx DecodeContext<'_>,
) -> Result<Option<ScopedValues<'ctx, f64>>, CodecError> {
    let Some((degree, expected_knot_count)) = usize::try_from(degree).ok().and_then(|degree| {
        control_count
            .checked_add(degree)?
            .checked_add(1)
            .map(|count| (degree, count))
    }) else {
        return Ok(None);
    };
    if periodic
        || degree != 1
        || control_count < 2
        || knots.len() != expected_knot_count
        || !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
    {
        return Ok(None);
    }
    let domain = [knots[degree], knots[control_count]];
    if range[0] < domain[0] || range[1] > domain[1] {
        return Ok(None);
    }
    if !ctx.all_by(
        knots.iter().enumerate(),
        |(index, knot)| {
            Ok(knot.is_finite()
                && (index == 0
                    || (knots[index - 1] <= *knot
                        && !(knots[index - 1] == *knot && range[0] < *knot && *knot < range[1]))))
        },
        "iges linear NURBS knot validation",
    )? {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "iges linear NURBS parameter storage")?;
    let mut parameters = Vec::new();
    ctx.push_scoped_vec(
        &mut storage,
        &mut parameters,
        range[0],
        "iges linear NURBS parameters",
    )?;
    let mut previous = range[0];
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut source_values = IntoIterator::into_iter(knots);
    while source_values.len() != 0 {
        let Some(&knot) = ctx.next_charged(&mut source_values, "iges linear NURBS parameter knots")? else {
            break;
        };
        if knot > range[0] && knot < range[1] && previous != knot {
            ctx.push_scoped_vec(
                &mut storage,
                &mut parameters,
                knot,
                "iges linear NURBS parameters",
            )?;
            previous = knot;
        }
    }
    ctx.push_scoped_vec(
        &mut storage,
        &mut parameters,
        range[1],
        "iges linear NURBS parameters",
    )?;
    Ok(Some((parameters, storage)))
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

fn point_display_symbol_type_allowed(entity_type: i64, global_table: GlobalTable) -> bool {
    match global_table {
        GlobalTable::Legacy => matches!(entity_type, 308 | 408),
        GlobalTable::V4_0 => entity_type == 408,
        GlobalTable::V5_0 | GlobalTable::V5Later => entity_type == 308,
    }
}

fn point_display_symbol_valid(
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    Ok(match record.value(4) {
        None | Some(crate::parameter::TokenValue::Omitted) => true,
        Some(crate::parameter::TokenValue::Integer(0)) => true,
        Some(crate::parameter::TokenValue::Integer(sequence)) => {
            match u32::try_from(*sequence)
                .ok()
                .filter(|sequence| sequence % 2 == 1)
            {
                Some(sequence) => ctx
                    .get_btree_map(entries, &sequence, "iges point symbol target lookup")?
                    .is_some_and(|target| {
                        target.form == 0
                            && point_display_symbol_type_allowed(target.entity_type, global_table)
                    }),
                None => false,
            }
        }
        Some(crate::parameter::TokenValue::Real(_) | crate::parameter::TokenValue::String(_)) => {
            false
        }
    })
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
    use_flag: UseFlag,
    global_table: GlobalTable,
) -> bool {
    !base_geometry_table_entry(entity_type, form)
        || !matches!(global_table, GlobalTable::V4_0)
        || matches!(
            use_flag,
            UseFlag::Geometry | UseFlag::Annotation | UseFlag::Definition | UseFlag::Parametric
        )
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

fn classify_control_point_plane(
    points: &[FinitePoint3],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<ControlPointPlane, CodecError> {
    let Some(origin) = points.first().map(|point| point.get()) else {
        return Ok(ControlPointPlane::NoUniquePlane);
    };
    let Some((first_direction, first_length)) = ctx.find_map(
        &points[1..],
        |point| {
            let direction = point.get().vector_from(origin);
            let length = direction.norm();
            Ok((length.is_finite() && length > tolerance).then_some((direction, length)))
        },
        "iges NURBS plane direction search",
    )?
    else {
        return Ok(ControlPointPlane::NoUniquePlane);
    };
    let Some(normal) = ctx.find_map(
        &points[1..],
        |point| {
            let candidate = first_direction.cross(point.get().vector_from(origin));
            let length = candidate.norm();
            Ok((length.is_finite() && length > tolerance * first_length).then_some(candidate))
        },
        "iges NURBS plane normal search",
    )?
    else {
        return Ok(ControlPointPlane::NoUniquePlane);
    };
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= 0.0 {
        return Ok(ControlPointPlane::NonPlanar);
    }
    let normal = normal.scale(1.0 / normal_length);
    Ok(
        if ctx.any_by(
            &points[1..],
            |point| Ok(normal.dot(point.get().vector_from(origin)).abs() > tolerance),
            "iges NURBS planarity check",
        )? {
            ControlPointPlane::NonPlanar
        } else {
            ControlPointPlane::Unique
        },
    )
}

fn control_points_fit_plane(
    points: &[FinitePoint3],
    normal: Vector3,
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(origin) = points.first().map(|point| point.get()) else {
        return Ok(false);
    };
    ctx.all_by(
        &points[1..],
        |point| Ok(normal.dot(point.get().vector_from(origin)).abs() <= tolerance),
        "iges NURBS declared plane check",
    )
}

#[derive(Clone, Copy)]
pub(super) struct DeclaredInterval {
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

    pub(super) fn around(value: f64, uncertainty: f64) -> Self {
        if uncertainty == 0.0 {
            Self {
                lower: value,
                upper: value,
            }
        } else {
            Self::outward(value - uncertainty, value + uncertainty)
        }
    }

    pub(super) fn add(self, other: Self) -> Self {
        Self::outward(self.lower + other.lower, self.upper + other.upper)
    }

    pub(super) fn subtract(self, other: Self) -> Self {
        Self::outward(self.lower - other.upper, self.upper - other.lower)
    }

    pub(super) fn multiply(self, other: Self) -> Self {
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

    pub(super) fn scale(self, factor: f64) -> Self {
        self.multiply(Self::around(factor, 0.0))
    }

    pub(super) fn reciprocal(self) -> Option<Self> {
        if self.contains(0.0) {
            return None;
        }
        let lower = 1.0 / self.lower;
        let upper = 1.0 / self.upper;
        Some(Self::outward(lower.min(upper), lower.max(upper)))
    }

    pub(super) fn sqrt(self) -> Option<Self> {
        if self.upper < 0.0 {
            return None;
        }
        Some(Self::outward(self.lower.max(0.0).sqrt(), self.upper.sqrt()))
    }

    pub(super) fn contains(self, value: f64) -> bool {
        self.lower <= value && value <= self.upper
    }

    pub(super) fn is_finite(self) -> bool {
        self.lower.is_finite() && self.upper.is_finite()
    }

    pub(super) fn is_strictly_positive(self) -> bool {
        self.lower > 0.0
    }

    pub(super) fn lower_bound(self) -> f64 {
        self.lower
    }

    pub(super) fn upper_bound(self) -> f64 {
        self.upper
    }

    pub(super) fn overlaps(self, other: Self) -> bool {
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<[DeclaredInterval; 3]>>, CodecError> {
    let Some((control_count, pole_start)) = (|| {
        let control_count = record.count(1)?.checked_add(1)?;
        let degree = usize::try_from(record.integer(2)?).ok()?;
        let knot_count = control_count.checked_add(degree)?.checked_add(1)?;
        let weight_start = 7usize.checked_add(knot_count)?;
        let pole_start = weight_start.checked_add(control_count)?;
        let pole_value_count = control_count.checked_mul(3)?;
        let range_start = pole_start.checked_add(pole_value_count)?;
        (record.parameter_end() >= range_start.checked_add(2)?)
            .then_some((control_count, pole_start))
    })() else {
        return Ok(None);
    };
    let mut controls =
        ctx.collection_vec(control_count, "iges Type126 declared control intervals")?;
    let mut point_iter = 0..control_count;
    while let Some(point) =
        ctx.next_charged(&mut point_iter, "iges Type126 declared control traversal")?
    {
        let mut coordinates = [DeclaredInterval::around(0.0, 0.0); 3];
        for (coordinate, value) in coordinates.iter_mut().enumerate() {
            let Some(index) = point
                .checked_mul(3)
                .and_then(|offset| pole_start.checked_add(offset))
                .and_then(|offset| offset.checked_add(coordinate))
            else {
                return Ok(None);
            };
            let Some(number) = record.number(index) else {
                return Ok(None);
            };
            *value = DeclaredInterval::around(
                number,
                record.number_uncertainty(index, number, precision),
            );
        }
        controls.push(coordinates);
    }
    Ok(Some(controls))
}

/// Return whether finite declared intervals prove one affine progression.
///
/// A sequence `x[i] = a + i*d` is affine exactly when one value of `d` makes
/// all intervals `[x[i] - i*d]` overlap. Pairwise bounds on `d` express that
/// condition without choosing a representative from any source interval. A
/// non-finite interval or bound is rejected instead of being treated as an
/// unconstrained value after arithmetic overflow.
pub(super) fn declared_affine_progression(
    values: &[f64],
    uncertainties: &[f64],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if values.len() < 2
        || values.len() != uncertainties.len()
        || ctx.any_by(
            values.iter().zip(uncertainties),
            |(value, uncertainty)| {
                if !value.is_finite() || !uncertainty.is_finite() || *uncertainty < 0.0 {
                    return Ok(true);
                }
                let interval = DeclaredInterval::around(*value, *uncertainty);
                Ok(!interval.lower.is_finite() || !interval.upper.is_finite())
            },
            "iges affine progression finite values",
        )?
    {
        return Ok(false);
    }
    let mut lower = f64::NEG_INFINITY;
    let mut upper = f64::INFINITY;
    let mut first_iter = 0..values.len();
    while let Some(first) = ctx.next_charged(&mut first_iter, "iges affine progression origins")? {
        let first_interval = DeclaredInterval::around(values[first], uncertainties[first]);
        let mut second_iter = first + 1..values.len();
        while let Some(second) =
            ctx.next_charged(&mut second_iter, "iges affine progression pairs")?
        {
            let second_interval = DeclaredInterval::around(values[second], uncertainties[second]);
            let Some(span) = cadmpeg_core::convert::f64_from_index(second - first) else {
                return Ok(false);
            };
            let pair_lower = (second_interval.lower - first_interval.upper) / span;
            let pair_upper = (second_interval.upper - first_interval.lower) / span;
            if !pair_lower.is_finite() || !pair_upper.is_finite() {
                return Ok(false);
            }
            lower = lower.max(pair_lower);
            upper = upper.min(pair_upper);
            if lower > upper {
                return Ok(false);
            }
        }
    }
    Ok(lower <= upper)
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
    vector.is_finite() && (vector.x != 0.0 || vector.y != 0.0 || vector.z != 0.0)
}

/// The normalized finite vector, when its length is nonzero and finite.
pub(super) fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| vector.scale(1.0 / norm))
}

pub(super) fn declared_unit_vector(
    record: &ParameterRecord,
    start: usize,
    vector: Vector3,
    precision: RealPrecision,
) -> Option<Vector3> {
    // CADIR admission for an IGES unit-vector field uses its declared-real
    // interval; IGES defines no separate receiver epsilon.
    let normalized = unit_vector(vector)?;
    let values = [vector.x, vector.y, vector.z];
    let components = std::array::from_fn::<_, 3, _>(|offset| {
        DeclaredInterval::around(
            values[offset],
            record.number_uncertainty(start + offset, values[offset], precision),
        )
    });
    interval_squared_norm(components)
        .contains(1.0)
        .then_some(normalized)
}

pub(super) fn declared_orthogonal_vectors(
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

#[derive(Debug)]
pub(crate) enum TransformResolutionError {
    Invalid(TransformFailure),
    Resource(CodecError),
}

#[derive(Debug)]
pub(crate) enum TransformFailure {
    Literal(&'static str),
    Depth,
    MissingEntry(u32),
    WrongTypeForm {
        sequence: u32,
        entity_type: i64,
        form: i64,
    },
    MissingParameters(u32),
    NonNumericCoefficient {
        sequence: u32,
        index: usize,
    },
    NonFiniteCoefficient(u32),
    NotOrthonormal(u32),
    WrongDeterminant {
        sequence: u32,
        form: i64,
    },
    FirstAxis(u32),
    SecondAxis(u32),
    NonFiniteScaled(u32),
    NonFiniteComposed(u32),
}

impl fmt::Display for TransformFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(message) => formatter.write_str(message),
            Self::Depth => write!(formatter, "transformation chain exceeds {MAX_TRANSFORM_DEPTH} entities"),
            Self::MissingEntry(sequence) => write!(formatter, "transformation D{sequence} is missing"),
            Self::WrongTypeForm { sequence, entity_type, form } => write!(formatter, "transformation D{sequence} is type {entity_type} form {form}, expected defining type 124 form 0 or 1"),
            Self::MissingParameters(sequence) => write!(formatter, "transformation D{sequence} parameters are missing"),
            Self::NonNumericCoefficient { sequence, index } => write!(formatter, "transformation D{sequence} coefficient {index} is not numeric"),
            Self::NonFiniteCoefficient(sequence) => write!(formatter, "transformation D{sequence} has a non-finite coefficient"),
            Self::NotOrthonormal(sequence) => write!(formatter, "transformation D{sequence} linear part is not orthonormal within its declared numeric precision"),
            Self::WrongDeterminant { sequence, form } => write!(formatter, "transformation D{sequence} determinant disagrees with form {form} within its declared numeric precision"),
            Self::FirstAxis(sequence) => write!(formatter, "transformation D{sequence} first axis cannot be normalized"),
            Self::SecondAxis(sequence) => write!(formatter, "transformation D{sequence} second axis cannot be normalized"),
            Self::NonFiniteScaled(sequence) => write!(formatter, "transformation D{sequence} has non-finite coefficients after length scaling"),
            Self::NonFiniteComposed(sequence) => write!(formatter, "transformation D{sequence} has non-finite coefficients after composition"),
        }
    }
}

impl From<TransformFailure> for TransformResolutionError {
    fn from(reason: TransformFailure) -> Self {
        Self::Invalid(reason)
    }
}

impl From<&'static str> for TransformResolutionError {
    fn from(message: &'static str) -> Self {
        Self::Invalid(TransformFailure::Literal(message))
    }
}

impl From<CodecError> for TransformResolutionError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl TransformResolutionError {
    pub(crate) fn non_resource(self) -> Result<TransformFailure, CodecError> {
        match self {
            Self::Invalid(message) => Ok(message),
            Self::Resource(error) => Err(error),
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
    ctx: &DecodeContext<'_>,
) -> Result<Transform, TransformResolutionError> {
    if sequence == 0 {
        return Ok(Transform::identity());
    }
    let sequence = u32::try_from(sequence).map_err(|_| {
        TransformFailure::Literal("transformation pointer is not a positive sequence")
    })?;
    if sequence % 2 == 0 {
        return Err("transformation pointer names an even Directory sequence".into());
    }
    let _nested = ctx.enter_nested("iges_transform_chain")?;
    let depth_limit = usize::try_from(ctx.policy().limits.max_recursion_depth)
        .ok()
        .map_or(MAX_TRANSFORM_DEPTH, |policy| {
            policy.min(MAX_TRANSFORM_DEPTH)
        });
    if path.len() >= depth_limit {
        return Err(TransformFailure::Depth.into());
    }
    if ctx.contains_btree_set(path, &sequence, "iges transform chain lookup")? {
        return Err("transformation chain is cyclic".into());
    }
    let mut path_storage = ctx.reserve_scoped(0, "iges transform path storage")?;
    path_storage
        .with_storage(|| ctx.insert_btree_set(path, sequence, "iges transform chain path"))?;
    let result: Result<Transform, TransformResolutionError> = (|| {
        let entry = ctx
            .get_btree_map(entries, &sequence, "iges transform entry lookup")?
            .copied()
            .ok_or(TransformFailure::MissingEntry(sequence))?;
        if entry.entity_type != 124 || !matches!(entry.form, 0 | 1) {
            return Err(TransformFailure::WrongTypeForm {
                sequence,
                entity_type: entry.entity_type,
                form: entry.form,
            }
            .into());
        }
        let record = ctx
            .get_btree_map(records, &sequence, "iges transform parameter lookup")?
            .copied()
            .ok_or(TransformFailure::MissingParameters(sequence))?;
        let mut values = [FiniteReal::ZERO; 12];
        for (index, value) in values.iter_mut().enumerate() {
            let number =
                record
                    .number(index + 1)
                    .ok_or(TransformFailure::NonNumericCoefficient {
                        sequence,
                        index: index + 1,
                    })?;
            *value =
                FiniteReal::new(number).ok_or(TransformFailure::NonFiniteCoefficient(sequence))?;
        }
        let mut values = values.map(FiniteReal::get);
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
                return Err(TransformFailure::NotOrthonormal(sequence).into());
            }
            Err(DeclaredTransformFrameError::WrongDeterminant) => {
                return Err(TransformFailure::WrongDeterminant {
                    sequence,
                    form: entry.form,
                }
                .into());
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
        .ok_or(TransformFailure::FirstAxis(sequence))?;
        let second_projection = first.dot(raw_columns[1]);
        let second_residual = raw_columns[1] - first.scale(second_projection);
        let second = {
            let n = second_residual.norm();
            (n.is_finite() && n > 0.0).then(|| second_residual.scale(1.0 / n))
        }
        .ok_or(TransformFailure::SecondAxis(sequence))?;
        let perpendicular = first.cross(second);
        let third = perpendicular.scale(expected_determinant);
        let local = Transform::affine([
            [first.x, second.x, third.x, values[3]],
            [first.y, second.y, third.y, values[7]],
            [first.z, second.z, third.z, values[11]],
        ])
        .ok_or(TransformFailure::NonFiniteScaled(sequence))?;
        let parent = resolve_transform(
            entry.transform,
            entries,
            records,
            length_factor,
            precision,
            path,
            ctx,
        )?;
        parent
            .compose(local)
            .map_err(|_| TransformFailure::NonFiniteComposed(sequence))
            .map_err(TransformResolutionError::from)
    })();
    if let Err(error) = ctx.remove_btree_set(path, &sequence, "iges transform chain removal") {
        // Refused cleanup must destroy the path before its frame storage expires.
        drop(std::mem::take(path));
        return Err(error.into());
    }
    result
}

pub(crate) fn enforce_transform_depth(
    directory: &[DirectoryEntry],
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let depth_limit = usize::try_from(ctx.policy().limits.max_recursion_depth)
        .ok()
        .map_or(MAX_TRANSFORM_DEPTH, |policy| {
            policy.min(MAX_TRANSFORM_DEPTH)
        });
    let mut entry_iter = directory.iter();
    while let Some(entry) = ctx.next_charged(
        &mut entry_iter,
        "iges transform preflight directory traversal",
    )? {
        let Some(mut sequence) = u32::try_from(entry.transform)
            .ok()
            .filter(|sequence| sequence % 2 == 1)
        else {
            continue;
        };
        let mut path_storage = ctx.reserve_scoped(0, "iges transform preflight path storage")?;
        let mut path = BTreeSet::new();
        let mut depth = 0_usize;
        let mut steps = std::iter::repeat(());
        while ctx
            .next_charged(&mut steps, "iges transform preflight walk")?
            .is_some()
        {
            if depth >= depth_limit {
                let requested = depth.checked_add(1).map(u64_from_index).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "iges_transform_depth",
                        cadmpeg_core::decode::u64_from_index(depth_limit),
                        u64::MAX,
                    )
                })?;
                return Err(ctx.refuse_codec_limit(
                    "iges_transform_depth",
                    cadmpeg_core::decode::u64_from_index(depth_limit),
                    requested,
                ));
            }
            if !path_storage.with_storage(|| {
                ctx.insert_btree_set(&mut path, sequence, "iges transform preflight path")
            })? {
                break;
            }
            depth += 1;
            let Some(transform) = crate::directory::entry_by_sequence(directory, sequence, ctx)?
            else {
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
pub(super) struct ProjectionOutcome<'ctx> {
    pub(super) decoded: BTreeSet<u32>,
    pub(super) decoded_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    pub(super) losses: Vec<LossNote>,
    pub(super) loss_slots_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

/// One source endpoint that participates in a face-local boundary vertex.
///
/// Boundary sewing is a topology decision. The endpoint coordinate remains
/// source evidence and must not be replaced by the neutral representative.
#[derive(Debug, Clone)]
pub(crate) struct BoundaryVertexSourceEndpoint {
    pub(crate) edge: String,
    pub(crate) endpoint: BoundaryEndpoint,
    pub(crate) position: FinitePoint3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum BoundaryEndpoint {
    Start,
    End,
}

/// The complete derivation of one neutral vertex created by boundary sewing.
#[derive(Debug, Clone)]
pub(crate) struct BoundaryVertexDerivation {
    pub(crate) source_entity: String,
    pub(crate) vertex: VertexId,
    pub(crate) representative: FinitePoint3,
    pub(crate) tolerance: f64,
    pub(crate) source_endpoints: Vec<BoundaryVertexSourceEndpoint>,
}

impl BoundaryVertexDerivation {
    pub(super) fn for_decode(
        source: (&str, &VertexId),
        representative: FinitePoint3,
        tolerance: f64,
        endpoints: &[BoundaryVertexSourceEndpoint],
        members: &[usize],
        ctx: &DecodeContext<'_>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<Self, CodecError> {
        storage.with_storage(|| {
            let mut source_endpoints =
                ctx.collection_vec(members.len(), "iges boundary derivation endpoints")?;
            if let Some(refusal) = ctx.resource_refusal() {
                return Err(refusal.into());
            }
            let mut source_values = IntoIterator::into_iter(members);
            while source_values.len() != 0 {
                let Some(member) = ctx.next_charged(&mut source_values, "iges boundary derivation member traversal")? else {
                    break;
                };
                let endpoint = &endpoints[*member];
                source_endpoints.push(BoundaryVertexSourceEndpoint {
                    edge: ctx
                        .copy_retained_text(&endpoint.edge, "iges boundary derivation edge text")?,
                    endpoint: endpoint.endpoint,
                    position: endpoint.position,
                });
            }
            Ok(Self {
                source_entity: ctx
                    .copy_retained_text(source.0, "iges boundary derivation source text")?,
                vertex: source
                    .1
                    .try_clone_for_decode(ctx, "iges trimming identity copy")?,
                representative,
                tolerance,
                source_endpoints,
            })
        })
    }
}

// The `merge_into` drains on the outcome types take `self` by value: every
// field a sub-projector returns has to be handed to an accumulator, so an
// outcome field cannot be dropped silently at the merge site. The accumulator
// element types are pairwise distinct, so arguments cannot be transposed.
impl ProjectionOutcome<'_> {
    fn merge_into(
        self,
        decoded: &mut BTreeSet<u32>,
        decoded_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        losses: &mut Vec<LossNote>,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        let source_decoded_storage;
        let loss_slots_storage;
        let Self {
            decoded: source_decoded,
            decoded_storage: result_decoded_storage,
            losses: source_losses,
            loss_slots_storage: result_loss_slots_storage,
        } = self;
        source_decoded_storage = result_decoded_storage;
        loss_slots_storage = result_loss_slots_storage;
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(source_decoded);
        while source_values.len() != 0 {
            let Some(sequence) = ctx.next_charged(&mut source_values, "iges merged decoded traversal")? else {
                break;
            };
            decoded_storage.with_storage(|| {
                ctx.insert_btree_set(decoded, sequence, "iges merged decoded sequences")
            })?;
        }
        drop(source_values);
        drop(source_decoded_storage);
        ctx.extend_vec(losses, source_losses, "iges merged loss slots")?;
        drop(loss_slots_storage);
        Ok(())
    }
}

/// A curve projector's result: the two-field outcome extended with the edges
/// the caller collects into the free-geometry wire shell.
pub(super) type ScopedWireProjection<'ctx> = (
    WireProjectionOutcome,
    cadmpeg_core::decode::ScopedReservation<'ctx>,
);

pub(super) struct WireProjectionOutcome {
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
    pub(super) wire_edges: Vec<EdgeId>,
}

impl WireProjectionOutcome {
    fn merge_into(
        self,
        decoded: &mut BTreeSet<u32>,
        decoded_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        losses: &mut Vec<LossNote>,
        wire_edges: &mut Vec<EdgeId>,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(self.decoded);
        while source_values.len() != 0 {
            let Some(sequence) = ctx.next_charged(&mut source_values, "iges merged decoded traversal")? else {
                break;
            };
            decoded_storage.with_storage(|| {
                ctx.insert_btree_set(decoded, sequence, "iges merged decoded sequences")
            })?;
        }
        ctx.extend_vec(losses, self.losses, "iges merged loss slots")?;
        ctx.extend_vec(wire_edges, self.wire_edges, "iges merged wire edge slots")?;
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Projection<'ctx> {
    pub(crate) placement_rejections: BTreeMap<u32, super::structure::PlacementRejection>,
    pub(crate) decoded: BTreeSet<u32>,
    /// Source records consumed as construction data without a standalone
    /// neutral entity. The generic retention pass suppresses its loss for
    /// these records; membership here is the only suppression channel.
    pub(crate) consumed: BTreeSet<u32>,
    pub(crate) losses: Vec<LossNote>,
    pub(crate) boundary_vertex_derivations: Vec<BoundaryVertexDerivation>,
    /// The Directory sequence each projected record was decoded from.
    pub(crate) sequences: SourceSequences<'ctx>,
    _decoded_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    _consumed_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    _boundary_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

fn positive_sequence(value: i64) -> Option<u32> {
    u32::try_from(value)
        .ok()
        .filter(|sequence| sequence % 2 == 1)
}

fn consumed_support_sequences<'ctx>(
    directory: &[DirectoryEntry],
    records: &BTreeMap<u32, &ParameterRecord>,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<(BTreeSet<u32>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    let mut support_storage = ctx.reserve_scoped(0, "iges consumed-support pending transforms")?;
    let mut transform_sequences = BTreeSet::new();
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges consumed-support directory traversal")?
        else {
            break;
        };
        if let Some(sequence) = positive_sequence(entry.transform) {
            if crate::directory::entry_by_sequence(directory, sequence, ctx)?
                .is_none_or(|target| target.entity_type != 124)
            {
                continue;
            }
            support_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut transform_sequences,
                    sequence,
                    "iges consumed-support transforms",
                )
            })?;
        }
    }
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges consumed-support directory traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 184 && matches!(entry.form, 0 | 1)) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(records, &entry.sequence, "iges geometry parameter lookup")?
            .copied()
        else {
            continue;
        };
        let Some(count) = record.count(1) else {
            continue;
        };
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut source_values = IntoIterator::into_iter(0..count.min(record.parameter_end()));
        while source_values.len() != 0 {
            let Some(index) = ctx.next_charged(&mut source_values, "iges consumed-support assembly members")? else {
                break;
            };
            if let Some(sequence) = record
                .integer(2 + count + index)
                .and_then(positive_sequence)
            {
                if crate::directory::entry_by_sequence(directory, sequence, ctx)?
                    .is_none_or(|target| target.entity_type != 124)
                {
                    continue;
                }
                support_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut transform_sequences,
                        sequence,
                        "iges consumed-support transforms",
                    )
                })?;
            }
        }
    }
    let mut consumed_storage = ctx.reserve_scoped(0, "iges consumed membership storage")?;
    let mut direction_sequences = BTreeSet::new();
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges consumed-support directory traversal")?
        else {
            break;
        };
        if !(matches!(entry.entity_type, 190 | 192 | 194 | 196 | 198) && matches!(entry.form, 0 | 1)) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(records, &entry.sequence, "iges geometry parameter lookup")?
            .copied()
        else {
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
            if let Some(sequence) = record.integer(*index).and_then(positive_sequence) {
                if !crate::directory::entry_by_sequence(directory, sequence, ctx)?
                    .is_some_and(|target| target.entity_type == 123 && target.form == 0)
                {
                    continue;
                }
                consumed_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut direction_sequences,
                        sequence,
                        "iges consumed-support directions",
                    )
                })?;
            }
        }
    }

    let mut consumed = direction_sequences;
    while let Some(sequence) = ctx
        .next_charged(
            &mut transform_sequences.iter(),
            "iges consumed-support closure traversal",
        )?
        .copied()
    {
        ctx.remove_btree_set(
            &mut transform_sequences,
            &sequence,
            "iges consumed-support pending removal",
        )?;
        if !consumed_storage.with_storage(|| {
            ctx.insert_btree_set(&mut consumed, sequence, "iges consumed-support closure")
        })? {
            continue;
        }
        let Some(entry) = crate::directory::entry_by_sequence(directory, sequence, ctx)? else {
            continue;
        };
        if let Some(parent) = positive_sequence(entry.transform) {
            if crate::directory::entry_by_sequence(directory, parent, ctx)?
                .is_none_or(|target| target.entity_type != 124)
            {
                continue;
            }
            support_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut transform_sequences,
                    parent,
                    "iges consumed-support transforms",
                )
            })?;
        }
    }
    Ok((consumed, consumed_storage))
}

fn point_on_plane(point: Point3, plane: (Point3, Vector3), resolution: f64) -> bool {
    let distance = point.vector_from(plane.0).dot(plane.1).abs();
    distance.is_finite() && distance <= resolution
}

fn normal_matches_plane(normal: Vector3, plane_normal: Vector3) -> bool {
    let unit = |vector| {
        cadmpeg_ir::features::FiniteVector3::new(vector)
            .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
    };
    let (Some(normal), Some(plane_normal)) = (unit(normal), unit(plane_normal)) else {
        return false;
    };
    normal.cross(plane_normal).norm() <= CURVE_PLANE_NORMAL_EPSILON
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
    geometry: &SolvedCurveGeometry,
    index: &ModelIndex<'_>,
    transform: Transform,
    plane: (Point3, Vector3),
    resolution: f64,
    active: &mut BTreeSet<CurveId>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let _depth = ctx.enter_nested("iges coplanar curve recursion")?;
    let point_valid = |point: Point3| {
        transform
            .apply_point(point)
            .is_some_and(|point| point_on_plane(point.get(), plane, resolution))
    };
    let normal_valid = |normal: Vector3| {
        transform
            .apply_normal(normal)
            .is_some_and(|normal| normal_matches_plane(*normal.as_raw(), plane.1))
    };
    let direction_valid = |direction: Vector3| {
        transform
            .apply_vector(direction)
            .is_some_and(|direction| direction_in_plane(direction.get(), plane.1))
    };
    let valid = match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            point_valid(origin) && direction_valid(direction)
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            point_valid(center) && normal_valid(*axis) && direction_valid(*ref_direction)
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            point_valid(center) && normal_valid(*axis) && direction_valid(*major_direction)
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex().get();
            let axis = parabola_curve.frame().axis().as_raw();
            let major_direction = parabola_curve.frame().reference().as_raw();
            point_valid(vertex) && normal_valid(*axis) && direction_valid(*major_direction)
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let center = hyperbola_curve.center().get();
            let axis = hyperbola_curve.frame().axis().as_raw();
            let major_direction = hyperbola_curve.frame().reference().as_raw();
            point_valid(center) && normal_valid(*axis) && direction_valid(*major_direction)
        }
        SolvedCurveGeometry::Degenerate(degenerate_curve) => {
            let point = degenerate_curve.point().get();
            point_valid(point)
        }
        SolvedCurveGeometry::Nurbs(curve) => ctx.all_by(
            0..curve.pole_count(),
            |index| {
                Ok(curve
                    .pole_rows()
                    .point_at(index)
                    .is_some_and(|point| point_valid(point.get())))
            },
            "iges coplanar NURBS controls",
        )?,
        SolvedCurveGeometry::Polyline(polyline) => ctx.all_by(
            polyline.points(),
            |point| Ok(point_valid(point.get())),
            "iges coplanar polyline points",
        )?,
        SolvedCurveGeometry::Composite { segments, .. } => {
            let mut valid = true;
            let mut segment_iter = segments.iter();
            while let Some(segment) =
                ctx.next_charged(&mut segment_iter, "iges coplanar composite segments")?
            {
                let Some(curve) = index.curves(segment.curve.as_str(), ctx)? else {
                    valid = false;
                    break;
                };
                let Some(geometry) = curve.geometry.solved() else {
                    valid = false;
                    break;
                };
                if ctx.contains_btree_set(active, &segment.curve, "iges coplanar active lookup")? {
                    valid = false;
                    break;
                }
                let mut active_storage = ctx.reserve_scoped(0, "iges coplanar active storage")?;
                active_storage.with_storage(|| {
                    let active_id = segment
                        .curve
                        .try_clone_for_decode(ctx, "iges coplanar active curve id")?;
                    ctx.insert_btree_set(active, active_id, "iges coplanar active curves")
                })?;
                let result = (|| {
                    let valid = curve_geometry_coplanar(
                        geometry, index, transform, plane, resolution, active, ctx,
                    )?;
                    ctx.remove_btree_set(active, &segment.curve, "iges coplanar active removal")?;
                    Ok(valid)
                })();
                let segment_valid = match result {
                    Ok(valid) => valid,
                    Err(error) => {
                        // Destroy the path before this frame's storage expires.
                        drop(std::mem::take(active));
                        return Err(error);
                    }
                };
                if !segment_valid {
                    valid = false;
                    break;
                }
            }
            valid
        }
        SolvedCurveGeometry::Transformed(placed) => match transform.compose(*placed.transform()) {
            Ok(transform) => curve_geometry_coplanar(
                placed.basis(),
                index,
                transform,
                plane,
                resolution,
                active,
                ctx,
            )?,
            Err(_) => false,
        },
        SolvedCurveGeometry::Unknown { .. } => false,
    };
    Ok(valid)
}

/// Records an entity loss when geometry admission fails.
pub(super) fn admit<T>(
    result: Result<T, &str>,
    entry: &DirectoryEntry,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(message) => {
            super::push_entity_loss(ctx, losses, entry, format_args!("{message}"))?;
            Ok(None)
        }
    }
}

/// The Directory sequence a source-object association names.
///
/// The sole producer of the `D{sequence}` object-id spelling. Nothing reads the
/// sequence back out of that text: every consumer holds the sequence itself,
/// through [`SourceSequences`] or through the entry it is projecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceObjectId(u32);

impl SourceObjectId {
    /// Names the Directory entry a projected record was decoded from.
    pub(crate) const fn new(sequence: u32) -> Self {
        Self(sequence)
    }

    /// The object-id text every source association carries.
    pub(crate) fn text(self) -> String {
        format!("D{}", self.0)
    }
}

/// The Directory sequence each projected body and face was decoded from.
///
/// Recorded where the id is minted, so appearance binding reads the sequence
/// the decoder held rather than parsing it back out of the identity.
#[derive(Debug, Default)]
pub(crate) struct SourceSequences<'ctx> {
    bodies: BTreeMap<BodyId, u32>,
    faces: BTreeMap<FaceId, u32>,
    curves: BTreeMap<CurveId, u32>,
    surfaces: BTreeMap<SurfaceId, u32>,
    points: BTreeMap<PointId, u32>,
    body_neutral_forms: BTreeMap<BodyId, u32>,
    storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

impl<'ctx> SourceSequences<'ctx> {
    pub(crate) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            storage: Some(ctx.reserve_scoped(0, "iges source sequence storage")?),
            ..Self::default()
        })
    }
    fn insert<K: Ord + cadmpeg_core::decode::cost::DecodeCost>(
        storage: &mut Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
        values: &mut BTreeMap<K, u32>,
        id: &K,
        sequence: u32,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
        copy: impl FnOnce(&K, &DecodeContext<'_>) -> Result<K, CodecError>,
    ) -> Result<(), CodecError> {
        if let Some(existing) = ctx.get_mut_btree_map(values, id, "iges source sequence lookup")? {
            *existing = sequence;
            return Ok(());
        }
        let insert = || {
            let key = copy(id, ctx)?;
            ctx.insert_btree_map(values, key, sequence, operation)?;
            Ok(())
        };
        match storage {
            Some(storage) => storage.with_storage(insert),
            None => insert(),
        }
    }

    /// Records the entry a body was decoded from, and -- when the body's key is
    /// rooted at that entry -- that the body is its neutral form.
    pub(super) fn record_body(
        &mut self,
        id: &BodyId,
        sequence: u32,
        stem: &crate::ids::Stem,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        Self::insert(
            &mut self.storage,
            &mut self.bodies,
            id,
            sequence,
            ctx,
            "iges source body sequences",
            |id, ctx| id.try_clone_for_decode(ctx, "iges source sequence key"),
        )?;
        if let Some(origin) = stem.origin() {
            Self::insert(
                &mut self.storage,
                &mut self.body_neutral_forms,
                id,
                origin,
                ctx,
                "iges source neutral body forms",
                |id, ctx| id.try_clone_for_decode(ctx, "iges source sequence key"),
            )?;
        }
        Ok(())
    }

    pub(super) fn record_face(
        &mut self,
        id: &FaceId,
        sequence: u32,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        Self::insert(
            &mut self.storage,
            &mut self.faces,
            id,
            sequence,
            ctx,
            "iges source face sequences",
            |id, ctx| id.try_clone_for_decode(ctx, "iges source sequence key"),
        )
    }

    pub(super) fn record_curve(
        &mut self,
        id: &CurveId,
        sequence: u32,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        Self::insert(
            &mut self.storage,
            &mut self.curves,
            id,
            sequence,
            ctx,
            "iges source curve sequences",
            |id, ctx| id.try_clone_for_decode(ctx, "iges source sequence key"),
        )
    }

    pub(super) fn record_surface(
        &mut self,
        id: &SurfaceId,
        sequence: u32,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        Self::insert(
            &mut self.storage,
            &mut self.surfaces,
            id,
            sequence,
            ctx,
            "iges source surface sequences",
            |id, ctx| id.try_clone_for_decode(ctx, "iges source sequence key"),
        )
    }

    /// Records the entry a point is the neutral form of, when its key is rooted
    /// at one.
    pub(super) fn record_point(
        &mut self,
        id: &PointId,
        stem: &crate::ids::Stem,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if let Some(sequence) = stem.origin() {
            Self::insert(
                &mut self.storage,
                &mut self.points,
                id,
                sequence,
                ctx,
                "iges source point sequences",
                |id, ctx| id.try_clone_for_decode(ctx, "iges source sequence key"),
            )?;
        }
        Ok(())
    }

    pub(super) fn body(
        &self,
        id: &BodyId,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<u32>, CodecError> {
        Ok(ctx
            .get_btree_map(&self.bodies, id, "iges source sequence lookup")?
            .copied())
    }

    pub(super) fn face(
        &self,
        id: &FaceId,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<u32>, CodecError> {
        Ok(ctx
            .get_btree_map(&self.faces, id, "iges source sequence lookup")?
            .copied())
    }

    /// The Directory sequence this curve was decoded from.
    pub(crate) fn curve(
        &self,
        id: &CurveId,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<u32>, CodecError> {
        Ok(ctx
            .get_btree_map(&self.curves, id, "iges source sequence lookup")?
            .copied())
    }

    /// The Directory sequence this surface was decoded from.
    pub(crate) fn surface(
        &self,
        id: &SurfaceId,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<u32>, CodecError> {
        Ok(ctx
            .get_btree_map(&self.surfaces, id, "iges source sequence lookup")?
            .copied())
    }

    /// The Directory sequence this point is the neutral form of.
    pub(crate) fn point(
        &self,
        id: &PointId,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<u32>, CodecError> {
        Ok(ctx
            .get_btree_map(&self.points, id, "iges source sequence lookup")?
            .copied())
    }

    /// The Directory sequence this body is the neutral form of.
    pub(crate) fn body_neutral_form(
        &self,
        id: &BodyId,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<u32>, CodecError> {
        Ok(ctx
            .get_btree_map(&self.body_neutral_forms, id, "iges source sequence lookup")?
            .copied())
    }
}

pub(super) fn source_object(
    entry: &DirectoryEntry,
    ctx: &DecodeContext<'_>,
) -> Result<SourceObjectAssociation, cadmpeg_core::CodecError> {
    let render = |args: std::fmt::Arguments<'_>, operation: &'static str| {
        ctx.format_retained(args, operation)
    };
    let object_id = render(format_args!("D{}", entry.sequence), "iges source object ID")?;
    let name = std::str::from_utf8(&entry.label)
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| render(format_args!("{value}"), "iges source object name"))
        .transpose()?;
    let layer = render(format_args!("{}", entry.level), "iges source object layer")?;
    Ok(SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Iges,
        object_id: cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            object_id,
            "validate nonblank text",
        )?
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("source object_id must not be empty"))?,
        name,
        color: None,
        visible: Some(entry.status.is_visible()),
        layer: Some(layer),
        instance_path: Vec::new(),
    })
}

pub(crate) fn project_geometry<'ctx>(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<Projection<'ctx>, CodecError> {
    let mut sequences = SourceSequences::new(ctx)?;
    let global_table = global.global_table();
    let admitted = |entry: &DirectoryEntry| {
        entry.status.use_flag(global_table).is_some_and(|use_flag| {
            base_geometry_use_flag_valid(entry.entity_type, entry.form, use_flag, global_table)
        }) && base_geometry_line_font_valid(
            entry.entity_type,
            entry.form,
            entry.line_font,
            global_table,
        ) && crate::profile::envelope_a_admits(entry.entity_type, entry.form, global_table)
    };
    let mut losses = Vec::new();
    let mut needs_admitted_copy = false;
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges geometry directory admission")?
        else {
            break;
        };
        needs_admitted_copy |= !admitted(entry);
        let Some(use_flag) = entry.status.use_flag(global_table) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "Entity Use Flag {:02} is outside the effective specification family",
                    entry.status.use_flag_code()
                ),
            )?;
            continue;
        };
        if !base_geometry_use_flag_valid(entry.entity_type, entry.form, use_flag, global_table) {
            super::push_entity_loss(ctx, &mut losses, entry, format_args!(
                    "Entity Use Flag {:02} is outside the IGES 4.0 base geometry values 00, 01, 02, and 05",
                    entry.status.use_flag_code()
                ))?;
        } else if !base_geometry_line_font_valid(
            entry.entity_type,
            entry.form,
            entry.line_font,
            global_table,
        ) {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "Line Font must be nonzero for this IGES 4.0 geometry entity"
                ),
            )?;
        }
    }
    let mut admitted_storage = ctx.reserve_scoped(0, "iges admitted directory storage")?;
    let admitted_directory = if needs_admitted_copy {
        let mut admitted_entries = Vec::new();
        let mut directory_entries = directory.iter();
        while !directory_entries.as_slice().is_empty() {
            let Some(entry) =
                ctx.next_charged(&mut directory_entries, "iges admitted directory traversal")?
            else {
                break;
            };
            if !(admitted(entry)) {
                continue;
            }
            ctx.push_scoped_vec(
                &mut admitted_storage,
                &mut admitted_entries,
                *entry,
                "iges admitted geometry directory",
            )?;
        }
        Some(admitted_entries)
    } else {
        None
    };
    let directory = admitted_directory.as_deref().unwrap_or(directory);
    let mut lookup_storage = ctx.reserve_scoped(0, "iges geometry source lookup storage")?;
    let mut records = BTreeMap::new();
    let mut source_index_entries = parameters.iter();
    while source_index_entries.len() != 0 {
        let Some(record) =
            ctx.next_charged(&mut source_index_entries, "iges geometry parameter traversal")?
        else {
            break;
        };
        lookup_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut records,
                record.directory_sequence,
                record,
                "iges geometry parameter index",
            )
        })?;
    }
    let mut entries = BTreeMap::new();
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges geometry directory index traversal")?
        else {
            break;
        };
        lookup_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut entries,
                entry.sequence,
                entry,
                "iges geometry directory index",
            )
        })?;
    }
    let mut decoded_storage = ctx.reserve_scoped(0, "iges decoded membership storage")?;
    let mut decoded = BTreeSet::new();
    let mut boundary_storage = ctx.reserve_scoped(0, "iges boundary derivation storage")?;
    let mut boundary_vertex_derivations = Vec::new();
    let consumed_storage;
    let (consumed, result_consumed_storage) = consumed_support_sequences(directory, &records, ctx)?;
    consumed_storage = result_consumed_storage;
    let mut location_storage = ctx.reserve_scoped(0, "iges analytic location storage")?;
    let mut analytic_surface_locations = BTreeSet::new();
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges geometry family traversal")?
        else {
            break;
        };
        if !(matches!(entry.entity_type, 190 | 192 | 194 | 196 | 198) && matches!(entry.form, 0 | 1)) {
            continue;
        }
        if let Some(sequence) = ctx
            .get_btree_map(&records, &entry.sequence, "iges geometry parameter lookup")?
            .and_then(|record| record.integer(1))
            .and_then(|value| u32::try_from(value).ok())
        {
            location_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut analytic_surface_locations,
                    sequence,
                    "iges analytic-surface locations",
                )
            })?;
        }
    }
    let mut free_vertices = Vec::new();
    let mut wire_edges = Vec::new();
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges geometry family traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 123 && entry.form == 0) {
            continue;
        }
        let Some(record) = ctx
            .get_btree_map(&records, &entry.sequence, "iges geometry parameter lookup")?
            .copied()
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let components = [record.number(1), record.number(2), record.number(3)];
        let [Some(x), Some(y), Some(z)] = components else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "direction components are not numeric"),
            )?;
            continue;
        };
        let direction = Vector3::new(x, y, z);
        if !is_finite_nonzero_vector(direction) {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "direction is zero or non-finite"),
            )?;
            continue;
        }
        if !entry.status.is_physically_dependent() {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Direction Entity is not marked physically dependent"),
            )?;
            continue;
        }
        if entry.transform != 0 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "Direction Entity references a prohibited transformation"
                ),
            )?;
        }
    }
    let mut primitive_entries = directory.iter();
    while let Some(entry) =
        ctx.next_charged(&mut primitive_entries, "iges geometry family traversal")?
    {
        if !(entry.entity_type == 100 && entry.form == 0) {
            continue;
        }
        let factor = global.length_factor_mm();
        let Some(record) = ctx
            .get_btree_map(&records, &entry.sequence, "iges geometry parameter lookup")?
            .copied()
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let mut values = [FiniteReal::ZERO; 7];
        let mut malformed = None;
        for (index, value) in values.iter_mut().enumerate() {
            match record.number(index + 1).and_then(FiniteReal::new) {
                Some(number) => *value = number,
                None => malformed = Some(index + 1),
            }
        }
        if let Some(index) = malformed {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("arc parameter {index} is not a finite number"),
            )?;
            continue;
        }
        let values = values.map(|value| value.get() * factor);
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
            Err(error) => {
                let message = error.non_resource()?;
                super::push_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                continue;
            }
        };
        let Some(basis_x) = transform
            .apply_vector(Vector3::new(1.0, 0.0, 0.0))
            .map(cadmpeg_ir::features::FiniteVector3::get)
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite vector"),
            )?;
            continue;
        };
        let Some(basis_y) = transform
            .apply_vector(Vector3::new(0.0, 1.0, 0.0))
            .map(cadmpeg_ir::features::FiniteVector3::get)
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite vector"),
            )?;
            continue;
        };
        let scale_x = basis_x.norm();
        let scale_y = basis_y.norm();
        let scale_tolerance = scale_x.max(scale_y).max(1.0) * COMPUTATION_TOLERANCE;
        if !scale_x.is_finite()
            || !scale_y.is_finite()
            || (scale_x - scale_y).abs() > scale_tolerance
            || basis_x.dot(basis_y).abs() > scale_x * scale_y * COMPUTATION_TOLERANCE
        {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "affine placement does not preserve circular geometry"),
            )?;
            continue;
        }
        let Some(center) = transform.apply_point(Point3::new(values[1], values[2], values[0]))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite point"),
            )?;
            continue;
        };
        let Some(start) = transform.apply_point(Point3::new(values[3], values[4], values[0]))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite point"),
            )?;
            continue;
        };
        let Some(end) = transform.apply_point(Point3::new(values[5], values[6], values[0])) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite point"),
            )?;
            continue;
        };
        let start_delta = start.vector_from(center.get());
        let end_delta = end.vector_from(center.get());
        let radius = start_delta.norm();
        let end_radius = end_delta.norm();
        let Some(ref_raw) = ({
            let n = start_delta.norm();
            (n.is_finite() && n > 0.0).then(|| start_delta.scale(1.0 / n))
        }) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "arc start point equals its center"),
            )?;
            continue;
        };
        let ref_direction = UnitVector3::normalized_by_reciprocal(start_delta);
        let ref_raw = ref_direction.map_or(ref_raw, |direction| *direction.as_raw());
        let Some(axis_raw) = ({
            let v = basis_x.cross(basis_y);
            let n = v.norm();
            (n.is_finite() && n > 0.0).then(|| v.scale(1.0 / n))
        }) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "arc placement collapses its plane"),
            )?;
            continue;
        };
        let axis = UnitVector3::normalized_by_reciprocal(basis_x.cross(basis_y));
        let axis_raw = axis.map_or(axis_raw, |direction| *direction.as_raw());
        let radius_tolerance = global
            .minimum_resolution_mm()
            .max(radius.max(end_radius).max(1.0) * COMPUTATION_TOLERANCE);
        if !end_radius.is_finite() || (end_radius - radius).abs() > radius_tolerance {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "arc start and terminate points have different radii"),
            )?;
            continue;
        }
        let Some(end_raw) = ({
            let n = end_delta.norm();
            (n.is_finite() && n > 0.0).then(|| end_delta.scale(1.0 / n))
        }) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "arc terminate point equals its center"),
            )?;
            continue;
        };
        let end_direction = UnitVector3::normalized_by_reciprocal(end_delta);
        let end_raw = end_direction.map_or(end_raw, |direction| *direction.as_raw());
        let mut angle = axis_raw
            .dot(ref_raw.cross(end_raw))
            .atan2(ref_raw.dot(end_raw))
            .rem_euclid(std::f64::consts::TAU);
        if angularly_equal(angle, 0.0) {
            angle = std::f64::consts::TAU;
        }
        let stem = crate::ids::Stem::directory(entry.sequence);
        let start_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        sequences.record_point(&start_point, &stem, ctx)?;
        let end_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
        sequences.record_point(&end_point, &stem, ctx)?;
        let start_vertex = crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        let end_vertex = crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
        let curve = crate::ids::curve_admitted(&stem, ctx)?;
        let edge = crate::ids::edge_admitted(&stem, ctx)?;
        ctx.reserve_vec(&mut ir.model.points, 2, "iges circle neutral point slots")?;
        ctx.charge_entities(2, "iges_geometry_primitives")?;
        ir.model.points.extend([
            Point::new(
                start_point.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                start,
                None,
            ),
            Point::new(
                end_point.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                end,
                None,
            ),
        ]);
        ctx.reserve_vec(
            &mut ir.model.vertices,
            2,
            "iges circle neutral vertex slots",
        )?;
        ctx.charge_entities(2, "iges_geometry_primitives")?;
        ir.model.vertices.extend([
            Vertex {
                id: start_vertex
                    .try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end_vertex.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                point: end_point,
                tolerance: None,
            },
        ]);
        sequences.record_curve(&curve, entry.sequence, ctx)?;
        ctx.reserve_vec(&mut ir.model.curves, 1, "iges circle neutral curve slots")?;
        ctx.charge_entities(1, "iges_geometry_primitives")?;
        ir.model.curves.push(Curve {
            id: curve.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                    center,
                    axis.and_then(|axis| {
                        ref_direction
                            .and_then(|reference| OrthonormalFrame3::from_units(axis, reference))
                    })
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "CircleCurve.axis/ref_direction must form an orthonormal frame",
                        )
                    })?,
                    PositiveLength::new(radius).ok_or_else(|| {
                        CodecError::malformed("CircleCurve.radius must be positive and finite")
                    })?,
                ),
            )),
            source_object: Some(source_object(entry, ctx)?),
        });
        ctx.reserve_vec(&mut ir.model.edges, 1, "iges circle neutral edge slots")?;
        ctx.charge_entities(1, "iges_geometry_primitives")?;
        ir.model.edges.push(Edge {
            id: edge.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve), Some([0.0, angle]))
                .map_err(CodecError::malformed)?,
            start: start_vertex,
            end: end_vertex,
            tolerance: None,
        });
        ctx.reserve_vec(&mut wire_edges, 1, "iges circle wire edge slots")?;
        wire_edges.push(edge);
        decoded_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut decoded,
                entry.sequence,
                "iges circle decoded sequences",
            )
        })?;
    }
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges geometry family traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 116 && entry.form == 0) {
            continue;
        }
        let factor = global.length_factor_mm();
        let Some(record) = ctx
            .get_btree_map(&records, &entry.sequence, "iges geometry parameter lookup")?
            .copied()
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let coordinates = [record.number(1), record.number(2), record.number(3)];
        let [Some(x), Some(y), Some(z)] = coordinates else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "X, Y, or Z is not numeric"),
            )?;
            continue;
        };
        if !point_display_symbol_valid(record, &entries, global.global_table(), ctx)? {
            super::push_attributed_loss(ctx, &mut losses, entry, IgesLossCode::DisplayDataNotProjected,
                format_args!("Type 116 display symbol pointer is invalid for the effective specification family"))?;
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
            Err(error) => {
                let message = error.non_resource()?;
                super::push_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                continue;
            }
        };
        let Some(position) = transform.apply_point(Point3::new(x * factor, y * factor, z * factor))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite point"),
            )?;
            continue;
        };
        let point = crate::ids::point_admitted(&crate::ids::Stem::directory(entry.sequence), ctx)?;
        sequences.record_point(&point, &crate::ids::Stem::directory(entry.sequence), ctx)?;
        ctx.reserve_vec(&mut ir.model.points, 1, "iges point neutral point slots")?;
        ctx.charge_entities(1, "iges_geometry_primitives")?;
        ir.model.points.push(Point::new(
            point.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            position,
            None,
        ));
        if entry.status.subordinate() == Some(Subordinate::Independent)
            || !ctx.contains_btree_set(
                &analytic_surface_locations,
                &entry.sequence,
                "iges analytic location lookup",
            )?
        {
            let vertex =
                crate::ids::vertex_admitted(&crate::ids::Stem::directory(entry.sequence), ctx)?;
            ctx.reserve_vec(&mut ir.model.vertices, 1, "iges point neutral vertex slots")?;
            ctx.charge_entities(1, "iges_geometry_primitives")?;
            ir.model.vertices.push(Vertex {
                id: vertex.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                point,
                tolerance: None,
            });
            ctx.reserve_vec(&mut free_vertices, 1, "iges point free vertex slots")?;
            free_vertices.push(vertex);
        }
        decoded_storage.with_storage(|| {
            ctx.insert_btree_set(&mut decoded, entry.sequence, "iges point decoded sequences")
        })?;
    }
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges geometry family traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 125 && (0..=4).contains(&entry.form)) {
            continue;
        }
        let factor = global.length_factor_mm();
        let Some(record) = ctx
            .get_btree_map(&records, &entry.sequence, "iges geometry parameter lookup")?
            .copied()
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let coordinates = [record.number(1), record.number(2)];
        let [Some(x), Some(y)] = coordinates else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "X or Y reference coordinate is not numeric"),
            )?;
            continue;
        };
        let Some(x) = FiniteReal::new(x) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "X or Y reference coordinate is not finite"),
            )?;
            continue;
        };
        let Some(y) = FiniteReal::new(y) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "X or Y reference coordinate is not finite"),
            )?;
            continue;
        };
        let required_real = |index| record.number(index).is_some();
        let optional_real = |index| record.number_or(index, 0.0).is_some();
        let shape_parameters_valid = match entry.form {
            0 => true,
            1 => required_real(3) && optional_real(4) && optional_real(5),
            2 => required_real(3) && required_real(4) && required_real(5),
            3 => required_real(3) && required_real(4) && optional_real(5),
            4 => required_real(3) && required_real(4) && required_real(5),
            _ => false,
        };
        if !shape_parameters_valid {
            super::push_attributed_loss(
                ctx,
                &mut losses,
                entry,
                IgesLossCode::DisplayDataNotProjected,
                format_args!("Type 125 flash shape parameters are incomplete or non-finite"),
            )?;
        }
        if entry.form == 0 && record.integer_or(6, 0).is_none_or(|pointer| pointer == 0) {
            super::push_attributed_loss(
                ctx,
                &mut losses,
                entry,
                IgesLossCode::DisplayDataNotProjected,
                format_args!("Type 125 Form 0 has no defining entity pointer"),
            )?;
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
            Err(error) => {
                let message = error.non_resource()?;
                super::push_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                continue;
            }
        };
        let Some(position) =
            transform.apply_point(Point3::new(x.get() * factor, y.get() * factor, 0.0))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite point"),
            )?;
            continue;
        };
        let point = crate::ids::point_admitted(&crate::ids::Stem::directory(entry.sequence), ctx)?;
        sequences.record_point(&point, &crate::ids::Stem::directory(entry.sequence), ctx)?;
        ctx.reserve_vec(&mut ir.model.points, 1, "iges flash neutral point slots")?;
        ctx.charge_entities(1, "iges_geometry_primitives")?;
        ir.model.points.push(Point::new(
            point.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            position,
            None,
        ));
        if entry.status.subordinate() == Some(Subordinate::Independent)
            || !ctx.contains_btree_set(
                &analytic_surface_locations,
                &entry.sequence,
                "iges analytic location lookup",
            )?
        {
            let vertex =
                crate::ids::vertex_admitted(&crate::ids::Stem::directory(entry.sequence), ctx)?;
            ctx.reserve_vec(&mut ir.model.vertices, 1, "iges flash neutral vertex slots")?;
            ctx.charge_entities(1, "iges_geometry_primitives")?;
            ir.model.vertices.push(Vertex {
                id: vertex.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                point,
                tolerance: None,
            });
            ctx.reserve_vec(&mut free_vertices, 1, "iges flash free vertex slots")?;
            free_vertices.push(vertex);
        }
        decoded_storage.with_storage(|| {
            ctx.insert_btree_set(&mut decoded, entry.sequence, "iges flash decoded sequences")
        })?;
    }
    let mut primitive_entries = directory.iter();
    while let Some(entry) =
        ctx.next_charged(&mut primitive_entries, "iges geometry family traversal")?
    {
        if !(entry.entity_type == 110 && (0..=2).contains(&entry.form)) {
            continue;
        }
        let factor = global.length_factor_mm();
        let Some(record) = ctx
            .get_btree_map(&records, &entry.sequence, "iges geometry parameter lookup")?
            .copied()
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let mut coordinates = [FiniteReal::ZERO; 6];
        let mut malformed = None;
        for (index, coordinate) in coordinates.iter_mut().enumerate() {
            match record.number(index + 1).and_then(FiniteReal::new) {
                Some(value) => *coordinate = value,
                None => malformed = Some(index + 1),
            }
        }
        if let Some(index) = malformed {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("endpoint coordinate {index} is not a finite number"),
            )?;
            continue;
        }
        let coordinates = coordinates.map(|coordinate| coordinate.get() * factor);
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
            Err(error) => {
                let message = error.non_resource()?;
                super::push_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                continue;
            }
        };
        let Some(start) =
            transform.apply_point(Point3::new(coordinates[0], coordinates[1], coordinates[2]))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite point"),
            )?;
            continue;
        };
        let Some(end) =
            transform.apply_point(Point3::new(coordinates[3], coordinates[4], coordinates[5]))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "placement produces a non-finite point"),
            )?;
            continue;
        };
        let delta = end.vector_from(start.get());
        let length = delta.norm();
        if !length.is_finite() || length <= 0.0 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "transformed endpoints are coincident or non-finite"),
            )?;
            continue;
        }
        let direction = UnitVector3::normalized_with_length(delta)
            .map(|(direction, _)| direction)
            .ok_or_else(|| CodecError::malformed("LineCurve.direction must have unit length"))?;
        let stem = crate::ids::Stem::directory(entry.sequence);
        let curve = crate::ids::curve_admitted(&stem, ctx)?;
        sequences.record_curve(&curve, entry.sequence, ctx)?;
        ctx.reserve_vec(&mut ir.model.curves, 1, "iges line neutral curve slots")?;
        ctx.charge_entities(1, "iges_geometry_primitives")?;
        let curve_position = ir.model.curves.len();
        ir.model.curves.push(Curve {
            id: curve,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(start, direction),
            )),
            source_object: Some(source_object(entry, ctx)?),
        });
        if entry.form != 0 {
            decoded_storage.with_storage(|| {
                ctx.insert_btree_set(&mut decoded, entry.sequence, "iges line decoded sequences")
            })?;
            continue;
        }
        let curve = ir.model.curves[curve_position]
            .id
            .try_clone_for_decode(ctx, "iges geometry neutral identity copy")?;
        let start_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        sequences.record_point(&start_point, &stem, ctx)?;
        let end_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
        sequences.record_point(&end_point, &stem, ctx)?;
        let start_vertex = crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        let end_vertex = crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
        let edge = crate::ids::edge_admitted(&stem, ctx)?;
        ctx.reserve_vec(&mut ir.model.points, 2, "iges line neutral point slots")?;
        ctx.charge_entities(2, "iges_geometry_primitives")?;
        ir.model.points.extend([
            Point::new(
                start_point.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                start,
                None,
            ),
            Point::new(
                end_point.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                end,
                None,
            ),
        ]);
        ctx.reserve_vec(&mut ir.model.vertices, 2, "iges line neutral vertex slots")?;
        ctx.charge_entities(2, "iges_geometry_primitives")?;
        ir.model.vertices.extend([
            Vertex {
                id: start_vertex
                    .try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end_vertex.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                point: end_point,
                tolerance: None,
            },
        ]);
        ctx.reserve_vec(&mut ir.model.edges, 1, "iges line neutral edge slots")?;
        ctx.charge_entities(1, "iges_geometry_primitives")?;
        ir.model.edges.push(Edge {
            id: edge.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve), Some([0.0, length]))
                .map_err(CodecError::malformed)?,
            start: start_vertex,
            end: end_vertex,
            tolerance: None,
        });
        ctx.reserve_vec(&mut wire_edges, 1, "iges line wire edge slots")?;
        wire_edges.push(edge);
        decoded_storage.with_storage(|| {
            ctx.insert_btree_set(&mut decoded, entry.sequence, "iges line decoded sequences")
        })?;
    }
    let mut directory_entries = directory.iter();
    while !directory_entries.as_slice().is_empty() {
        let Some(entry) =
            ctx.next_charged(&mut directory_entries, "iges geometry family traversal")?
        else {
            break;
        };
        if !(entry.entity_type == 126 && (0..=5).contains(&entry.form)) {
            continue;
        }
        let factor = global.length_factor_mm();
        let Some(record) = ctx
            .get_btree_map(&records, &entry.sequence, "iges geometry parameter lookup")?
            .copied()
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(k) = record.count(1) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "upper control-point index K is invalid"),
            )?;
            continue;
        };
        let Some(degree) = record
            .integer(2)
            .and_then(|value| u32::try_from(value).ok())
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "basis degree M is invalid"),
            )?;
            continue;
        };
        let degree_usize = index_from_u32(degree);
        if k < degree_usize {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "control-point count is smaller than degree plus one"),
            )?;
            continue;
        }
        let flags = [
            record.integer(3),
            record.integer(4),
            record.integer(5),
            record.integer(6),
        ];
        if flags.iter().any(|flag| !matches!(flag, Some(0 | 1))) {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "one or more spline flags are not 0 or 1"),
            )?;
            continue;
        }
        let Some(control_count) = k.checked_add(1) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "control-point count overflows"),
            )?;
            continue;
        };
        let Some(knot_count) = control_count
            .checked_add(degree_usize)
            .and_then(|value| value.checked_add(1))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "knot count overflows"),
            )?;
            continue;
        };
        let knot_start = 7_usize;
        let Some(weight_start) = knot_start.checked_add(knot_count) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "weight offset overflows"),
            )?;
            continue;
        };
        let Some(pole_start) = weight_start.checked_add(control_count) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "control-point offset overflows"),
            )?;
            continue;
        };
        let Some(pole_value_count) = control_count.checked_mul(3) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "control-point value count overflows"),
            )?;
            continue;
        };
        let Some(range_start) = pole_start.checked_add(pole_value_count) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "parameter-range offset overflows"),
            )?;
            continue;
        };
        let collect_numbers = |start: usize,
                               count: usize,
                               operation: &'static str|
         -> Result<Option<Vec<FiniteReal>>, CodecError> {
            let Some(end) = start.checked_add(count) else {
                return Ok(None);
            };
            ctx.collect_options(
                (start..end).map(|index| record.number(index).and_then(FiniteReal::new)),
                operation,
            )
        };
        let mut knot_storage = ctx.reserve_scoped(0, "iges NURBS source lane storage")?;
        let Some(finite_knots) = knot_storage
            .with_storage(|| collect_numbers(knot_start, knot_count, "iges NURBS source knots"))?
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "knot vector is truncated or non-finite"),
            )?;
            continue;
        };
        let domain_start = finite_knots[degree_usize];
        let domain_end = finite_knots[control_count];
        let mut raw_knots = ctx.collection_vec(finite_knots.len(), "iges NURBS admitted knots")?;
        raw_knots.extend(
            ctx.admit_iter(finite_knots, "iges NURBS admitted knot traversal")?
                .map(FiniteReal::get),
        );
        drop(knot_storage);
        let Ok(knots) = KnotVector::new(ctx, raw_knots)? else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "knot vector is decreasing"),
            )?;
            continue;
        };
        let mut weight_storage = ctx.reserve_scoped(0, "iges NURBS source lane storage")?;
        let native_weights = {
            let mut source_weight_storage = ctx.reserve_scoped(0, "iges NURBS source lane storage")?;
            let Some(finite_weights) = source_weight_storage.with_storage(|| {
                collect_numbers(weight_start, control_count, "iges NURBS source weights")
            })? else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "weight vector is truncated or non-finite"),
                )?;
                continue;
            };
            weight_storage.with_storage(|| {
                ctx.collect_options(
                    finite_weights.into_iter().map(|weight| PositiveReal::try_from(weight).ok()),
                    "iges NURBS positive weights",
                )
            })?
        };
        let Some(native_weights) = native_weights else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "weights are not strictly positive"),
            )?;
            continue;
        };
        let precision = global.real_precision();
        let uncertainty =
            |index: usize, value: f64| record.number_uncertainty(index, value, precision);
        let equal_within_significance =
            |left_index: usize, left: f64, right_index: usize, right: f64| {
                (left - right).abs()
                    <= uncertainty(left_index, left) + uncertainty(right_index, right)
            };
        let equal_weights = if let Some(first) = native_weights.first() {
            ctx.all_by(
                native_weights.iter().enumerate(),
                |(offset, weight)| {
                    Ok(equal_within_significance(
                        weight_start,
                        first.get(),
                        weight_start + offset,
                        weight.get(),
                    ))
                },
                "iges NURBS weight equality",
            )?
        } else {
            false
        };
        let polynomial = flags[2] == Some(1);
        if polynomial && !equal_weights {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "polynomial spline has unequal weights"),
            )?;
            continue;
        }
        if !polynomial && equal_weights {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "rational spline has equal weights but PROP3 declares rational"
                ),
            )?;
            continue;
        }
        let mut pole_storage = ctx.reserve_scoped(0, "iges NURBS source lane storage")?;
        let Some(native_poles) = pole_storage.with_storage(|| {
            collect_numbers(pole_start, pole_value_count, "iges NURBS source poles")
        })?
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "control-point vector is truncated or non-finite"),
            )?;
            continue;
        };
        let [Some(range_start_value), Some(range_end_value)] = [range_start, range_start + 1]
            .map(|index| record.number(index).and_then(FiniteReal::new))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "parameter range is missing or non-finite"),
            )?;
            continue;
        };
        let mut parameter_range = [range_start_value, range_end_value];
        if parameter_range[0].get() < domain_start.get()
            && equal_within_significance(
                range_start,
                parameter_range[0].get(),
                knot_start + degree_usize,
                domain_start.get(),
            )
        {
            parameter_range[0] = domain_start;
        }
        if parameter_range[1].get() > domain_end.get()
            && equal_within_significance(
                range_start + 1,
                parameter_range[1].get(),
                knot_start + control_count,
                domain_end.get(),
            )
        {
            parameter_range[1] = domain_end;
        }
        let parameter_interval =
            IncreasingParameterInterval::between(parameter_range[0], parameter_range[1]).filter(
                |_| {
                    parameter_range[0].get() >= domain_start.get()
                        && parameter_range[1].get() <= domain_end.get()
                },
            );
        let Some(parameter_interval) = parameter_interval else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "parameter range lies outside the spline knot domain"),
            )?;
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
            Err(error) => {
                let message = error.non_resource()?;
                super::push_entity_loss(ctx, &mut losses, entry, format_args!("{message}"))?;
                continue;
            }
        };
        let mut pairing_storage = ctx.reserve_scoped(0, "iges NURBS source lane storage")?;
        let collect_controls = || {
            ctx.collect_options(
                native_poles.chunks_exact(3).map(|point| {
                    transform.apply_point(Point3::new(
                        point[0].get() * factor,
                        point[1].get() * factor,
                        point[2].get() * factor,
                    ))
                }),
                "iges NURBS placed controls",
            )
        };
        let control_points = if polynomial {
            collect_controls()?
        } else {
            pairing_storage.with_storage(collect_controls)?
        };
        drop(native_poles);
        drop(pole_storage);
        let Some(control_points) = control_points else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "transformed control-point vector is non-finite"),
            )?;
            continue;
        };
        let point_scale = ctx
            .admit_iter(&control_points, "iges NURBS control scale")?
            .skip(1)
            .map(|point| point.distance(control_points[0].get()))
            .filter(|distance| distance.is_finite())
            .fold(1.0, f64::max);
        let plane_tolerance = global
            .minimum_resolution_mm()
            .max(point_scale * COMPUTATION_TOLERANCE);
        let plane = classify_control_point_plane(&control_points, plane_tolerance, ctx)?;
        let planar = flags[0] == Some(1);
        if planar {
            let Some(normal_start) = range_start.checked_add(2) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "plane-normal offset overflows"),
                )?;
                continue;
            };
            let [Some(normal_x), Some(normal_y), Some(normal_z)] =
                [normal_start, normal_start + 1, normal_start + 2]
                    .map(|index| record.number(index).and_then(FiniteReal::new))
            else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "plane-normal fields are missing or non-finite"),
                )?;
                continue;
            };
            let normal_definition = Vector3::new(normal_x.get(), normal_y.get(), normal_z.get());
            if declared_unit_vector(record, normal_start, normal_definition, precision).is_none() {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "planar spline normal is not a declared unit vector"),
                )?;
                continue;
            }
            let Some(normal) = transform.apply_vector(normal_definition) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "placement produces a non-finite vector"),
                )?;
                continue;
            };
            let normal_length = normal.get().norm();
            if !normal_length.is_finite()
                || normal_length <= 0.0
                || !control_points_fit_plane(
                    &control_points,
                    normal.get().scale(1.0 / normal_length),
                    plane_tolerance,
                    ctx,
                )?
                || matches!(plane, ControlPointPlane::NonPlanar)
            {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!(
                        "{}",
                        "planar spline flag disagrees with the control-point geometry"
                    ),
                )?;
                continue;
            }
        } else if matches!(plane, ControlPointPlane::Unique) {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "non-planar spline flag disagrees with a unique control-point plane"
                ),
            )?;
            continue;
        }
        let weights = if polynomial {
            drop(native_weights);
            drop(weight_storage);
            None
        } else {
            let mut values = pairing_storage.with_storage(|| {
                ctx.collection_vec(native_weights.len(), "iges NURBS neutral weights")
            })?;
            values.extend(
                ctx.admit_iter(native_weights, "iges NURBS neutral weight traversal")?
                    .map(NonZeroReal::from),
            );
            drop(weight_storage);
            Some(values)
        };
        // IGES PROP4 is informational; neutral evaluation uses the
        // serialized active carrier without periodic parameter wrapping.
        let nurbs_result = NurbsCurve::from_checked_lanes(
            ctx,
            degree,
            knots,
            control_points,
            weights,
            false,
        )?;
        drop(pairing_storage);
        let nurbs = match nurbs_result {
            Ok(nurbs) => nurbs,
            Err(error) => {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("spline cardinalities are inconsistent: {error}"),
                )?;
                continue;
            }
        };
        let Some(start) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, &nurbs, parameter_range[0].get()),
        )?)?
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "spline start point cannot be evaluated"),
            )?;
            continue;
        };
        let Some(end) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, &nurbs, parameter_range[1].get()),
        )?)?
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "spline end point cannot be evaluated"),
            )?;
            continue;
        };
        let endpoint_distance = start.distance(end.get());
        let resolution = global.minimum_resolution_mm();
        let closed = endpoint_distance == 0.0 || endpoint_distance < resolution;
        if flags[1] != Some(i64::from(closed)) {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "closed spline flag disagrees with evaluated endpoints"
                ),
            )?;
            continue;
        }
        let stem = crate::ids::Stem::directory(entry.sequence);
        let start_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        sequences.record_point(&start_point, &stem, ctx)?;
        let end_point = crate::ids::point_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
        sequences.record_point(&end_point, &stem, ctx)?;
        let start_vertex = crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::Start), ctx)?;
        let end_vertex = crate::ids::vertex_admitted(&stem.tail(crate::ids::Word::End), ctx)?;
        let curve = crate::ids::curve_admitted(&stem, ctx)?;
        let edge = crate::ids::edge_admitted(&stem, ctx)?;
        ctx.reserve_vec(&mut ir.model.points, 2, "iges NURBS neutral point slots")?;
        ctx.charge_entities(2, "iges_geometry_primitives")?;
        ir.model.points.extend([
            Point::new(
                start_point.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                start,
                None,
            ),
            Point::new(
                end_point.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                end,
                None,
            ),
        ]);
        ctx.reserve_vec(&mut ir.model.vertices, 2, "iges NURBS neutral vertex slots")?;
        ctx.charge_entities(2, "iges_geometry_primitives")?;
        ir.model.vertices.extend([
            Vertex {
                id: start_vertex
                    .try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end_vertex.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
                point: end_point,
                tolerance: None,
            },
        ]);
        sequences.record_curve(&curve, entry.sequence, ctx)?;
        ctx.reserve_vec(&mut ir.model.curves, 1, "iges NURBS neutral curve slots")?;
        ctx.charge_entities(1, "iges_geometry_primitives")?;
        ir.model.curves.push(Curve {
            id: curve.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
            source_object: Some(source_object(entry, ctx)?),
        });
        ctx.reserve_vec(&mut ir.model.edges, 1, "iges NURBS neutral edge slots")?;
        ctx.charge_entities(1, "iges_geometry_primitives")?;
        ir.model.edges.push(Edge {
            id: edge.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::Bounded(curve, parameter_interval.into()),
            start: start_vertex,
            end: end_vertex,
            tolerance: None,
        });
        ctx.reserve_vec(&mut wire_edges, 1, "iges NURBS wire edge slots")?;
        wire_edges.push(edge);
        decoded_storage.with_storage(|| {
            ctx.insert_btree_set(&mut decoded, entry.sequence, "iges NURBS decoded sequences")
        })?;
    }
    // The stanza sequence keeps source order in losses, wire edges, and free
    // vertices. Each projection admits its model entities before creation.
    let membership_storage;
    let (outcome, result_membership_storage) = super::conics::project(
        ir,
        directory,
        (&entries, &records),
        global,
        ctx,
        &mut sequences,
    )?;
    membership_storage = result_membership_storage;
    outcome.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        &mut wire_edges,
        ctx,
    )?;
    drop(membership_storage);

    super::copious::project(
        ir,
        directory,
        &entries,
        &records,
        global,
        ctx,
        &mut sequences,
    )?
    .merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        &mut wire_edges,
        &mut free_vertices,
        ctx,
    )?;

    super::splines::project(ir, directory, parameters, global, ctx, &mut sequences)?.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        &mut wire_edges,
        ctx,
    )?;

    let membership_storage;
    let (outcome, result_membership_storage) = super::composite::project(
        ir,
        directory,
        (&entries, &records),
        global,
        ctx,
        &mut sequences,
    )?;
    membership_storage = result_membership_storage;
    outcome.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        &mut wire_edges,
        ctx,
    )?;
    drop(membership_storage);

    super::offsets::project(ir, directory, parameters, global, ctx, &mut sequences)?.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        &mut wire_edges,
        ctx,
    )?;

    // A valid V5 Type 130 constituent is deferred until its exact offset
    // carrier has been projected above. The second composite pass consumes
    // that carrier while retaining each entity's ordered child list.
    let membership_storage;
    let (outcome, result_membership_storage) = super::composite::project_type_130_children(
        ir,
        directory,
        (&entries, &records),
        global,
        ctx,
        &mut sequences,
    )?;
    membership_storage = result_membership_storage;
    outcome.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        &mut wire_edges,
        ctx,
    )?;
    drop(membership_storage);

    super::analytic_surfaces::project(ir, directory, parameters, global, ctx, &mut sequences)?
        .merge_into(&mut decoded, &mut decoded_storage, &mut losses, ctx)?;

    super::surfaces::project(ir, directory, parameters, global, ctx, &mut sequences)?.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        ctx,
    )?;

    if !wire_edges.is_empty() || !free_vertices.is_empty() {
        let body = crate::ids::body_admitted(
            &crate::ids::Stem::word(crate::ids::Word::FreeGeometry),
            ctx,
        )?;
        let region = crate::ids::region_admitted(
            &crate::ids::Stem::word(crate::ids::Word::FreeGeometry),
            ctx,
        )?;
        let shell = crate::ids::shell_admitted(
            &crate::ids::Stem::word(crate::ids::Word::FreeGeometry),
            ctx,
        )?;
        let mut body_regions = ctx.collection_vec(1, "iges free wire body regions")?;
        body_regions.push(region.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?);
        ctx.reserve_vec(&mut ir.model.bodies, 1, "iges free wire body slots")?;
        ctx.charge_entities(1, "iges_geometry_wire_topology")?;
        ir.model.bodies.push(Body {
            id: body.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            kind: BodyKind::Wire,
            regions: body_regions,
            transform: None,
            name: Some(ctx.copy_retained_text(
                "IGES free geometry", "iges free geometry body name",
            )?),
            color: None,
            visible: None,
        });
        let mut region_shells = ctx.collection_vec(1, "iges free wire region shells")?;
        region_shells.push(shell.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?);
        ctx.reserve_vec(&mut ir.model.regions, 1, "iges free wire region slots")?;
        ctx.charge_entities(1, "iges_geometry_wire_topology")?;
        ir.model.regions.push(Region {
            id: region.try_clone_for_decode(ctx, "iges geometry neutral identity copy")?,
            body,
            shells: region_shells,
        });
        ctx.reserve_vec(&mut ir.model.shells, 1, "iges free wire shell slots")?;
        ctx.charge_entities(1, "iges_geometry_wire_topology")?;
        let shell = match Shell::new(shell, region, Vec::new(), wire_edges, free_vertices) {
            Ok(shell) => shell,
            Err(message) => {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("{message}"),
                    "iges free-wire shell error",
                )?))
            }
        };
        ir.model.shells.push(shell);
    }

    let (trimming_projection, trimming_vertex_derivations) = super::trimming::project(
        ir,
        directory,
        parameters,
        global,
        (ctx, &mut boundary_storage),
        &mut sequences,
    )?;
    boundary_storage.with_storage(|| {
        ctx.extend_vec(
            &mut boundary_vertex_derivations,
            trimming_vertex_derivations,
            "iges merged boundary vertex derivations",
        )
    })?;
    trimming_projection.merge_into(&mut decoded, &mut decoded_storage, &mut losses, ctx)?;

    super::brep::project(
        ir,
        directory,
        (&entries, &records),
        global,
        ctx,
        &mut sequences,
    )?
    .merge_into(&mut decoded, &mut decoded_storage, &mut losses, ctx)?;

    super::csg::project(ir, directory, &entries, &records, global, ctx)?.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        ctx,
    )?;

    let (structure_projection, placement_rejections) = super::structure::project(
        ir,
        directory,
        (&entries, &records),
        trailing_pointer_analysis,
        global,
        ctx,
        &mut sequences,
    )?;
    structure_projection.merge_into(&mut decoded, &mut decoded_storage, &mut losses, ctx)?;

    super::presentation::project(
        ir,
        directory,
        (&entries, &records),
        trailing_pointer_analysis,
        global,
        ctx,
        &sequences,
    )?
    .merge_into(&mut decoded, &mut decoded_storage, &mut losses, ctx)?;

    super::drawing::project(
        ir,
        directory,
        (&entries, &records),
        trailing_pointer_analysis,
        global,
        ctx,
    )?
    .merge_into(&mut decoded, &mut decoded_storage, &mut losses, ctx)?;

    super::annotation::project(ir, directory, (&entries, &records), global, ctx)?.merge_into(
        &mut decoded,
        &mut decoded_storage,
        &mut losses,
        ctx,
    )?;

    let mut vertex_storage = ctx.reserve_scoped(0, "iges analytic vertex point storage")?;
    let mut vertex_points = BTreeSet::new();
    let mut source_index_entries = ir.model.vertices.iter();
    while source_index_entries.len() != 0 {
        let Some(vertex) =
            ctx.next_charged(&mut source_index_entries, "iges analytic vertex traversal")?
        else {
            break;
        };
        vertex_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut vertex_points,
                &vertex.point,
                "iges analytic-surface vertex point index",
            )
        })?;
    }
    ctx.retain_vec(
        &mut ir.model.points,
        |point| {
            let is_location = match sequences.point(&point.id, ctx)? {
                Some(sequence) => ctx.contains_btree_set(
                    &analytic_surface_locations,
                    &sequence,
                    "iges analytic location lookup",
                )?,
                None => false,
            };
            Ok(!is_location
                || ctx.contains_btree_set(
                    &vertex_points,
                    &point.id,
                    "iges analytic vertex point lookup",
                )?)
        },
        "iges analytic point retention",
    )?;
    Ok(Projection {
        _decoded_storage: Some(decoded_storage),
        _consumed_storage: Some(consumed_storage),
        _boundary_storage: Some(boundary_storage),
        placement_rejections,
        decoded,
        consumed,
        losses,
        boundary_vertex_derivations,
        sequences,
    })
}

#[cfg(test)]
mod tests;
