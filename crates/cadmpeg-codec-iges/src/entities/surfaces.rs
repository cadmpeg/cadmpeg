// SPDX-License-Identifier: Apache-2.0
//! Analytic and free-form surface projection.

use super::composite::{bounded_parameter_range_for_curve, curve_carrier_id, CompositeIndex};
use super::geometry::{
    declared_unit_vector, entity_loss, resolve_transform, source_object, unit_vector,
    DeclaredInterval, ProjectionOutcome,
};
use crate::directory::DirectoryEntry;
use crate::global::{GlobalTable, ProjectedGlobal, RealPrecision};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::bezier::{
    boundaries_within_resolution, homogeneous_spans, positive_controls, HomogeneousBezierSpan,
};
use cadmpeg_ir::geometry::{
    derive_reference_direction,
    nurbs::{
        knots_nondecreasing, NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
        SurfaceParameterAxis,
    },
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, RecordBounds,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{
    NonNegativeLength, NonZeroLength, NonZeroReal, PositiveLength, PositiveReal,
};
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const EPS_SURFACES_SIMILARITY_ORIENTATION_E10: f64 = 1.0e-10;

const MAX_SURFACE_POLES: usize = 1_000_000;

/// Return the source-declared intervals for a Type 128 surface's four
/// parameter-range fields.
///
/// The range fields are after the variable knot, weight, and pole blocks. The
/// projected surface keeps their parsed representatives in `record_bounds`,
/// but a trimming consumer must retain the source real-token interval when it
/// checks a derived p-curve. This is the same declaration interval used when
/// admitting a Type 128 range against its active knot domain below.
pub(super) fn type128_parameter_bound_intervals(
    record: &ParameterRecord,
    precision: RealPrecision,
) -> Option<[DeclaredInterval; 4]> {
    let [u_count, v_count] = [record.count(1), record.count(2)].map(|count| count?.checked_add(1));
    let [u_degree, v_degree] =
        [record.integer(3), record.integer(4)].map(|degree| usize::try_from(degree?).ok());
    let [Some(u_count), Some(v_count), Some(u_degree), Some(v_degree)] =
        [u_count, v_count, u_degree, v_degree]
    else {
        return None;
    };
    let u_knot_count = u_count.checked_add(u_degree)?.checked_add(1)?;
    let v_knot_count = v_count.checked_add(v_degree)?.checked_add(1)?;
    let pole_count = u_count.checked_mul(v_count)?;
    let pole_value_count = pole_count.checked_mul(3)?;
    let range_start = 10usize
        .checked_add(u_knot_count)?
        .checked_add(v_knot_count)?
        .checked_add(pole_count)?
        .checked_add(pole_value_count)?;
    (0..4)
        .map(|offset| {
            let index = range_start.checked_add(offset)?;
            let value = record.number(index).filter(|value| value.is_finite())?;
            Some(DeclaredInterval::around(
                value,
                record.number_uncertainty(index, value, precision),
            ))
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn tabulated_directrix_type_allowed(
    entity_type: i64,
    form: i64,
    global_table: GlobalTable,
) -> bool {
    if matches!(global_table, GlobalTable::V4_0) {
        return matches!(
            (entity_type, form),
            (100 | 102 | 110 | 112, 0) | (104, 0..=3) | (126, 0..=5)
        );
    }
    matches!(
        (entity_type, form),
        (100 | 102 | 110 | 112 | 130 | 142, 0) | (104, 0..=3) | (126, 0..=5)
    )
}

fn similarity_orientation(transform: cadmpeg_ir::transform::Transform) -> Option<f64> {
    let rows = transform.affine_rows();
    let scale = rows
        .iter()
        .flat_map(|row| &row[..3])
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    if scale == 0.0 {
        return None;
    }
    let column = |index| {
        Vector3::new(
            rows[0][index] / scale,
            rows[1][index] / scale,
            rows[2][index] / scale,
        )
    };
    let [x, y, z] = [column(0), column(1), column(2)];
    let squared_scale = x.dot(x);
    if !squared_scale.is_finite() || squared_scale <= 0.0 {
        return None;
    }
    let tolerance = squared_scale * EPS_SURFACES_SIMILARITY_ORIENTATION_E10;
    if (y.dot(y) - squared_scale).abs() > tolerance
        || (z.dot(z) - squared_scale).abs() > tolerance
        || x.dot(y).abs() > tolerance
        || x.dot(z).abs() > tolerance
        || y.dot(z).abs() > tolerance
    {
        return None;
    }
    let determinant = x.dot(y.cross(z));
    let determinant_tolerance =
        squared_scale.sqrt() * squared_scale * EPS_SURFACES_SIMILARITY_ORIENTATION_E10;
    (determinant.is_finite() && determinant.abs() > determinant_tolerance)
        .then(|| determinant.signum())
}

fn bounded_nurbs(
    ir: &CadIr,
    curve_id: &CurveId,
    ctx: Option<&DecodeContext<'_>>,
    index: &CompositeIndex,
) -> Result<Option<(NurbsCurve, [f64; 2])>, super::composite::CompositeCurveError> {
    super::composite::bounded_nurbs_for_curve(ir, curve_id, ctx, Some(index))
}

fn constant_speed_curve(geometry: &CurveGeometry) -> bool {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => true,
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)) => true,
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            ellipse_curve.major_radius().get() == ellipse_curve.minor_radius().get()
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) => {
            curve.degree() == 1
                && curve.weights().is_none()
                && curve.control_points().len() == 2
                && curve.control_points()[0]
                    .distance(curve.control_points()[1].get())
                    .is_finite()
                && curve.control_points()[0].distance(curve.control_points()[1].get()) > 0.0
                && curve.knots()[0] == curve.knots()[1]
                && curve.knots()[2] == curve.knots()[3]
                && curve.knots()[1] < curve.knots()[2]
        }
        _ => false,
    }
}

fn interval_certified_linear_bezier(
    geometry: &NurbsCurve,
    record: &ParameterRecord,
    global: &ProjectedGlobal,
) -> bool {
    if record.integer(0) != Some(126) {
        return false;
    }
    let Ok(degree) = usize::try_from(geometry.degree()) else {
        return false;
    };
    let Some(control_count) = degree.checked_add(1) else {
        return false;
    };
    if degree < 2
        || geometry.weights().is_some()
        || geometry.periodic()
        || geometry.control_points().len() != control_count
    {
        return false;
    }
    let Some(lower) = geometry.knots().first().copied() else {
        return false;
    };
    let Some(upper) = geometry.knots().last().copied() else {
        return false;
    };
    if lower >= upper
        || geometry.knots()[..control_count]
            .iter()
            .any(|knot| *knot != lower)
        || geometry.knots()[control_count..]
            .iter()
            .any(|knot| *knot != upper)
        || geometry
            .control_points()
            .first()
            .zip(geometry.control_points().last())
            .is_none_or(|(first, last)| {
                let distance = first.distance(last.get());
                !distance.is_finite() || distance <= 0.0
            })
    {
        return false;
    }

    let Some(k) = record.count(1) else {
        return false;
    };
    let Some(source_degree) = record
        .integer(2)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if source_degree != degree || k.checked_add(1) != Some(control_count) {
        return false;
    }
    let Some(knot_count) = control_count
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return false;
    };
    let Some(weight_start) = 7usize.checked_add(knot_count) else {
        return false;
    };
    let Some(pole_start) = weight_start.checked_add(control_count) else {
        return false;
    };
    let precision = global.real_precision();
    let mut coordinate_values = [
        Vec::with_capacity(control_count),
        Vec::with_capacity(control_count),
        Vec::with_capacity(control_count),
    ];
    let mut coordinate_uncertainties = [
        Vec::with_capacity(control_count),
        Vec::with_capacity(control_count),
        Vec::with_capacity(control_count),
    ];
    for control_index in 0..control_count {
        for coordinate in 0..3 {
            let Some(index) = pole_start
                .checked_add(control_index * 3)
                .and_then(|index| index.checked_add(coordinate))
            else {
                return false;
            };
            let Some(value) = record.number(index) else {
                return false;
            };
            let uncertainty = record.number_uncertainty(index, value, precision);
            coordinate_values[coordinate].push(value);
            coordinate_uncertainties[coordinate].push(uncertainty);
        }
    }
    coordinate_values
        .into_iter()
        .zip(coordinate_uncertainties)
        .all(|(values, uncertainties)| {
            super::geometry::declared_affine_progression(&values, &uncertainties)
        })
}

fn equal_arc_length_parameterization(
    ir: &CadIr,
    first_sequence: u32,
    second_sequence: u32,
    first_interval: [f64; 2],
    second_interval: [f64; 2],
    records: &BTreeMap<u32, &ParameterRecord>,
    global: &ProjectedGlobal,
) -> bool {
    // A normalized parameter is an arc-length parameter only for a constant-
    // speed carrier. The test is deliberately structural; numerical sampling
    // cannot prove the Form 0 correspondence.
    let curve_geometry = |sequence| {
        ir.model
            .curves
            .iter()
            .find(|curve| curve.id == crate::ids::curve(&crate::ids::Stem::directory(sequence)))
            .map(|curve| &curve.geometry)
    };
    let Some((first, second)) = curve_geometry(first_sequence).zip(curve_geometry(second_sequence))
    else {
        return false;
    };
    let valid_interval = |interval: [f64; 2]| {
        interval[0].is_finite() && interval[1].is_finite() && interval[0] < interval[1]
    };
    if !valid_interval(first_interval) || !valid_interval(second_interval) {
        return false;
    }
    let constant_speed = |sequence: u32, geometry: &CurveGeometry| {
        if constant_speed_curve(geometry) {
            return true;
        }
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = geometry else {
            return false;
        };
        records
            .get(&sequence)
            .is_some_and(|record| interval_certified_linear_bezier(curve, record, global))
    };
    constant_speed(first_sequence, first) && constant_speed(second_sequence, second)
}

fn bounded_evaluable_curve(
    ir: &CadIr,
    curve_id: &CurveId,
    tolerance: f64,
    index: &CompositeIndex,
) -> Option<(CurveGeometry, [f64; 2])> {
    let curve = index.curve_by_id(ir, curve_id)?;
    let geometry = &curve.geometry;
    if matches!(
        geometry,
        CurveGeometry::Solved(
            SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. }
        ) | CurveGeometry::Procedural { .. }
    ) {
        return None;
    }
    let parameter_interval =
        super::composite::bounded_parameter_range_for_curve(ir, curve_id, tolerance, Some(index))?;
    if !parameter_interval[0].is_finite()
        || !parameter_interval[1].is_finite()
        || parameter_interval[0] >= parameter_interval[1]
    {
        return None;
    }
    let geometry = geometry.clone();
    parameter_interval
        .into_iter()
        .all(|parameter| cadmpeg_ir::eval::curve_point(&geometry, parameter).is_ok())
        .then_some((
            CurveGeometry::Solved(geometry.solved()?.clone()),
            parameter_interval,
        ))
}

/// `cadmpeg_ir::geometry::PlacedCurve::try_new` bounds the chain, so the walk
/// needs no depth of its own.
fn is_line_carrier(geometry: &SolvedCurveGeometry) -> bool {
    match geometry {
        SolvedCurveGeometry::Line(_) => true,
        SolvedCurveGeometry::Transformed(placed) => is_line_carrier(placed.basis()),
        _ => false,
    }
}

fn source_parameter_interval(geometry: &CurveGeometry, carrier_interval: [f64; 2]) -> [f64; 2] {
    if geometry.solved().is_some_and(is_line_carrier) {
        [0.0, 1.0]
    } else {
        carrier_interval
    }
}

fn curve_geometry<'a>(ir: &'a CadIr, curve_id: &CurveId) -> Option<&'a CurveGeometry> {
    ir.model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .map(|curve| &curve.geometry)
}

fn homogeneous_bezier_spans(curve: &NurbsCurve) -> Option<Vec<HomogeneousBezierSpan>> {
    let degree = usize::try_from(curve.degree()).ok()?;
    let count = curve.pole_count();
    let weights = match curve.weights() {
        Some(weights) => {
            if weights.iter().any(|weight| weight.get() <= 0.0) {
                return None;
            }
            weights.into_iter().map(NonZeroReal::get).collect()
        }
        None => {
            cadmpeg_core::decode::alloc_filled(count, 1.0, "iges_surface_closure_weights").ok()?
        }
    };
    let controls = positive_controls(&curve.control_points(), &weights)?;
    homogeneous_spans(degree, curve.knots(), controls)
}

fn bernstein_binomial(n: usize, k: usize) -> Option<f64> {
    if k > n {
        return None;
    }
    let k = k.min(n - k);
    let value = (1..=k).try_fold(1.0, |value, factor| {
        let value = value * (n - k + factor) as f64 / factor as f64;
        value.is_finite().then_some(value)
    })?;
    Some(value)
}

fn homogeneous_product_with_scalar(
    vector_controls: &[[f64; 4]],
    scalar_controls: &[[f64; 4]],
) -> Option<Vec<[f64; 4]>> {
    // For homogeneous rails C1=A/a and C2=B/b, the ruled blend is
    // ((1-v)A*b + v*B*a)/(a*b). Multiplying Bernstein polynomials gives the
    // exact u-direction poles without fitting the Euclidean curve.
    let vector_degree = vector_controls.len().checked_sub(1)?;
    let scalar_degree = scalar_controls.len().checked_sub(1)?;
    let degree = vector_degree.checked_add(scalar_degree)?;
    let mut product = Vec::with_capacity(degree.checked_add(1)?);
    for index in 0..=degree {
        let denominator = bernstein_binomial(degree, index)?;
        let lower = index.saturating_sub(scalar_degree);
        let upper = index.min(vector_degree);
        let mut control = [0.0; 4];
        for (offset, vector_control) in vector_controls[lower..=upper].iter().enumerate() {
            let vector_index = lower + offset;
            let scalar_index = index - vector_index;
            let coefficient = bernstein_binomial(vector_degree, vector_index)?
                * bernstein_binomial(scalar_degree, scalar_index)?
                / denominator;
            let scalar = scalar_controls[scalar_index][3];
            for axis in 0..4 {
                control[axis] += coefficient * vector_control[axis] * scalar;
            }
        }
        if control.iter().any(|value| !value.is_finite()) {
            return None;
        }
        product.push(control);
    }
    Some(product)
}

fn span_fraction(value: f64, domain: [f64; 2]) -> Option<f64> {
    if !value.is_finite() || !domain.into_iter().all(f64::is_finite) || domain[0] >= domain[1] {
        return None;
    }
    let width = domain[1] - domain[0];
    let offset = value - domain[0];
    let fraction = if width.is_finite() && offset.is_finite() {
        offset / width
    } else {
        (0.5 * value - 0.5 * domain[0]) / (0.5 * domain[1] - 0.5 * domain[0])
    };
    fraction.is_finite().then_some(fraction)
}

fn split_homogeneous_bezier_span(
    span: &HomogeneousBezierSpan,
    cut: f64,
) -> Option<(HomogeneousBezierSpan, HomogeneousBezierSpan)> {
    if !cut.is_finite() || cut <= span.domain[0] || cut >= span.domain[1] {
        return None;
    }
    let parameter = span_fraction(cut, span.domain)?;
    if !parameter.is_finite() || parameter <= 0.0 || parameter >= 1.0 {
        return None;
    }
    let degree = span.controls.len().checked_sub(1)?;
    let mut levels = vec![span.controls.clone()];
    for _ in 1..=degree {
        let previous = levels.last()?;
        let current = previous
            .windows(2)
            .map(|pair| {
                std::array::from_fn(|axis| {
                    (1.0 - parameter) * pair[0][axis] + parameter * pair[1][axis]
                })
            })
            .collect::<Vec<_>>();
        if current.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        levels.push(current);
    }
    let left = (0..=degree)
        .map(|level| levels[level][0])
        .collect::<Vec<_>>();
    let right = (0..=degree)
        .map(|index| levels[degree - index][index])
        .collect::<Vec<_>>();
    Some((
        HomogeneousBezierSpan {
            domain: [span.domain[0], cut],
            controls: left,
        },
        HomogeneousBezierSpan {
            domain: [cut, span.domain[1]],
            controls: right,
        },
    ))
}

fn homogeneous_span_domain(spans: &[HomogeneousBezierSpan]) -> Option<[f64; 2]> {
    Some([spans.first()?.domain[0], spans.last()?.domain[1]])
        .filter(|domain| domain[0].is_finite() && domain[1].is_finite() && domain[0] < domain[1])
}

fn normalized_span_boundaries(
    spans: &[HomogeneousBezierSpan],
    domain: [f64; 2],
) -> Option<Vec<f64>> {
    let mut boundaries = Vec::with_capacity(spans.len().checked_add(1)?);
    for span in spans {
        for value in span.domain {
            let normalized = span_fraction(value, domain)?;
            if !normalized.is_finite() || !(0.0..=1.0).contains(&normalized) {
                return None;
            }
            boundaries.push(normalized);
        }
    }
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    (boundaries.first() == Some(&0.0) && boundaries.last() == Some(&1.0)).then_some(boundaries)
}

fn partition_homogeneous_spans(
    spans: &[HomogeneousBezierSpan],
    domain: [f64; 2],
    boundaries: &[f64],
) -> Option<Vec<HomogeneousBezierSpan>> {
    let mut partitioned = Vec::new();
    for span in spans {
        let start = span_fraction(span.domain[0], domain)?;
        let end = span_fraction(span.domain[1], domain)?;
        if !start.is_finite() || !end.is_finite() || start >= end {
            return None;
        }
        let cuts = boundaries
            .iter()
            .copied()
            .filter(|boundary| start < *boundary && *boundary < end)
            .map(|boundary| {
                cadmpeg_ir::math::interpolate(domain[0], domain[1], boundary)
                    .map(cadmpeg_ir::scalar::FiniteReal::get)
            })
            .collect::<Option<Vec<_>>>()?;
        let mut current = span.clone();
        for cut in cuts {
            let (left, right) = split_homogeneous_bezier_span(&current, cut)?;
            partitioned.push(left);
            current = right;
        }
        partitioned.push(current);
    }
    Some(partitioned)
}

fn aligned_homogeneous_spans(
    first: &NurbsCurve,
    second: &NurbsCurve,
) -> Option<Vec<(HomogeneousBezierSpan, HomogeneousBezierSpan)>> {
    let first_spans = homogeneous_bezier_spans(first)?;
    let second_spans = homogeneous_bezier_spans(second)?;
    let first_domain = homogeneous_span_domain(&first_spans)?;
    let second_domain = homogeneous_span_domain(&second_spans)?;
    let mut boundaries = normalized_span_boundaries(&first_spans, first_domain)?;
    boundaries.extend(normalized_span_boundaries(&second_spans, second_domain)?);
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let first_spans = partition_homogeneous_spans(&first_spans, first_domain, &boundaries)?;
    let second_spans = partition_homogeneous_spans(&second_spans, second_domain, &boundaries)?;
    (first_spans.len() == second_spans.len())
        .then(|| first_spans.into_iter().zip(second_spans).collect())
}

/// Positive weights in pole order, unit weights for a polynomial curve.
fn curve_weights(curve: &NurbsCurve) -> Option<Vec<NonZeroReal>> {
    match curve.weights() {
        Some(weights) => weights
            .iter()
            .all(|weight| weight.get() > 0.0)
            .then_some(weights),
        None => Some(
            std::iter::repeat_n(NonZeroReal::from(PositiveReal::ONE), curve.pole_count()).collect(),
        ),
    }
}

fn projectively_shared_weights(
    first: &NurbsCurve,
    second: &NurbsCurve,
) -> Option<Vec<NonZeroReal>> {
    let first_weights = curve_weights(first)?;
    let second_weights = curve_weights(second)?;
    if first_weights.len() != second_weights.len() {
        return None;
    }
    let scale = second_weights.first()?.get() / first_weights.first()?.get();
    if !scale.is_finite()
        || scale <= 0.0
        || first_weights
            .iter()
            .zip(&second_weights)
            .any(|(first, second)| first.get() * scale != second.get())
    {
        return None;
    }
    Some(first_weights)
}

fn same_basis_ruled_surface(
    first: &NurbsCurve,
    second: &NurbsCurve,
    weights: &[NonZeroReal],
) -> Result<NurbsSurface, cadmpeg_ir::geometry::nurbs::NurbsError> {
    let surface_weights = weights
        .iter()
        .copied()
        .flat_map(|weight| [weight, weight])
        .collect::<Vec<_>>();
    let weights = if surface_weights.iter().all(|weight| weight.get() == 1.0) {
        None
    } else {
        Some(surface_weights)
    };
    NurbsSurface::from_checked_lanes(
        NurbsSurfaceAxis::new(
            first.degree(),
            first.knots().to_vec(),
            first.periodic() && second.periodic(),
        ),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            first
                .control_points()
                .into_iter()
                .zip(second.control_points())
                .map(|(first, second)| vec![first, second])
                .collect(),
            weights.map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    )
}

/// Refuses a pole count above the codec limit, naming the limit it exceeds.
///
/// The refusal is the caller's to carry: with a context it is the context's own
/// budget refusal, and without one it is the same local limit error every other
/// pole-count site in this file returns.
fn admit_surface_pole_count(
    ctx: Option<&DecodeContext<'_>>,
    pole_count: usize,
) -> Result<(), cadmpeg_core::CodecError> {
    if pole_count > MAX_SURFACE_POLES {
        return Err(match ctx {
            Some(ctx) => ctx.refuse_codec_limit(
                "iges_surface_poles",
                MAX_SURFACE_POLES as u64,
                pole_count as u64,
            ),
            None => refuse_local_limit(
                "iges_surface_poles",
                MAX_SURFACE_POLES as u64,
                pole_count as u64,
            ),
        });
    }
    Ok(())
}

fn ruled_surface_carrier(
    first: &NurbsCurve,
    second: &NurbsCurve,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<NurbsSurface>, cadmpeg_core::CodecError> {
    if first.degree() == second.degree()
        && first.knots() == second.knots()
        && first.control_points().len() == second.control_points().len()
    {
        if let Some(weights) = projectively_shared_weights(first, second) {
            let Some(pole_count) = first.control_points().len().checked_mul(2) else {
                return Ok(None);
            };
            admit_surface_pole_count(ctx, pole_count)?;
            return same_basis_ruled_surface(first, second, &weights)
                .map(Some)
                .map_err(cadmpeg_core::CodecError::malformed);
        }
    }
    let mut pole_refusal = None;
    let lanes = ruled_surface_span_lanes(first, second, ctx, &mut pole_refusal);
    if let Some(error) = pole_refusal {
        return Err(error);
    }
    let Some((degree, u_knots, control_points, weights)) = lanes else {
        return Ok(None);
    };
    Ok(Some(
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(degree, u_knots, first.periodic() && second.periodic()),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                control_points.chunks(2_usize).map(<[_]>::to_vec).collect(),
                weights.map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
            ),
            false,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    ))
}

type RuledSpanLanes = (u32, Vec<f64>, Vec<Point3>, Option<Vec<f64>>);

/// The span lanes of a ruled carrier, or `None` when the rails state none.
///
/// `pole_refusal` carries the one answer that is a refusal rather than an
/// absent carrier: a pole count above the codec limit. The caller returns it,
/// so the limit is not lost in the `None` that every other exit means.
fn ruled_surface_span_lanes(
    first: &NurbsCurve,
    second: &NurbsCurve,
    ctx: Option<&DecodeContext<'_>>,
    pole_refusal: &mut Option<cadmpeg_core::CodecError>,
) -> Option<RuledSpanLanes> {
    let degree = usize::try_from(first.degree())
        .ok()?
        .checked_add(usize::try_from(second.degree()).ok()?)?;
    if degree == 0 {
        return None;
    }
    let spans = aligned_homogeneous_spans(first, second)?;
    let u_count = spans.len().checked_mul(degree)?.checked_add(1)?;
    let pole_count = u_count.checked_mul(2)?;
    if let Err(error) = admit_surface_pole_count(ctx, pole_count) {
        *pole_refusal = Some(error);
        return None;
    }
    let mut homogeneous = Vec::with_capacity(pole_count);
    let mut u_knots = Vec::with_capacity(u_count.checked_add(degree)?.checked_add(1)?);
    for (span_index, (first_span, second_span)) in spans.iter().enumerate() {
        if first_span.controls.len() != usize::try_from(first.degree()).ok()?.checked_add(1)?
            || second_span.controls.len()
                != usize::try_from(second.degree()).ok()?.checked_add(1)?
        {
            return None;
        }
        let first_times_second =
            homogeneous_product_with_scalar(&first_span.controls, &second_span.controls)?;
        let second_times_first =
            homogeneous_product_with_scalar(&second_span.controls, &first_span.controls)?;
        if first_times_second.len() != degree + 1 || second_times_first.len() != degree + 1 {
            return None;
        }
        if span_index == 0 {
            u_knots.extend(std::iter::repeat_n(first_span.domain[0], degree + 1));
        } else {
            u_knots.extend(std::iter::repeat_n(first_span.domain[0], degree));
        }
        let start = usize::from(span_index > 0);
        for index in start..=degree {
            homogeneous.extend([first_times_second[index], second_times_first[index]]);
        }
        if span_index + 1 == spans.len() {
            u_knots.extend(std::iter::repeat_n(first_span.domain[1], degree + 1));
        }
    }
    if homogeneous.len() != pole_count || u_knots.len() != u_count + degree + 1 {
        return None;
    }
    let mut control_points = Vec::with_capacity(pole_count);
    let mut weights = Vec::with_capacity(pole_count);
    for control in homogeneous {
        let weight = control[3];
        if !weight.is_finite() || weight <= 0.0 {
            return None;
        }
        let point = Point3::new(
            control[0] / weight,
            control[1] / weight,
            control[2] / weight,
        );
        if [point.x, point.y, point.z]
            .into_iter()
            .any(|value| !value.is_finite())
        {
            return None;
        }
        control_points.push(point);
        weights.push(weight);
    }
    let weights = if weights.iter().all(|weight| *weight == 1.0) {
        None
    } else {
        Some(weights)
    };
    Some((
        u32::try_from(degree).ok()?,
        u_knots,
        control_points,
        weights,
    ))
}

fn homogeneous_curve_boundary_matches(
    first: &NurbsCurve,
    second: &NurbsCurve,
    range: [f64; 2],
    resolution: f64,
) -> Option<bool> {
    if !resolution.is_finite()
        || resolution < 0.0
        || !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
    {
        return None;
    }
    let first_spans = homogeneous_bezier_spans(first)?;
    let second_spans = homogeneous_bezier_spans(second)?;
    if first.degree() != second.degree()
        || first.knots() != second.knots()
        || first_spans.len() != second_spans.len()
    {
        return None;
    }
    for (first_span, second_span) in first_spans.iter().zip(second_spans) {
        if first_span.domain[1] <= range[0] || first_span.domain[0] >= range[1] {
            continue;
        }
        if first_span.domain != second_span.domain {
            return None;
        }
        if !boundaries_within_resolution(&first_span.controls, &second_span.controls, resolution)? {
            return Some(false);
        }
    }
    Some(true)
}

fn surface_boundary_is_closed(
    surface: &NurbsSurface,
    fixed_axis: SurfaceParameterAxis,
    fixed_range: [f64; 2],
    varying_range: [f64; 2],
    resolution: f64,
) -> Option<bool> {
    let first = cadmpeg_ir::eval::nurbs_surface_isocurve(surface, fixed_axis, fixed_range[0])?;
    let second = cadmpeg_ir::eval::nurbs_surface_isocurve(surface, fixed_axis, fixed_range[1])?;
    homogeneous_curve_boundary_matches(&first, &second, varying_range, resolution)
}

fn rotate(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cosine = angle.cos();
    let sine = angle.sin();
    let parallel = axis.scale(axis.dot(vector));
    let perpendicular = vector - parallel;
    let tangent = axis.cross(perpendicular);
    parallel + perpendicular.scale(cosine) + tangent.scale(sine)
}

struct AngularBasis {
    knots: Vec<f64>,
    controls: Vec<(f64, f64)>,
}

fn angular_basis(start: f64, end: f64) -> Option<AngularBasis> {
    let sweep = end - start;
    if !sweep.is_finite()
        || sweep <= 0.0
        || sweep > std::f64::consts::TAU + super::curve_conversion::ANGULAR_TOLERANCE
    {
        return None;
    }
    let sweep = sweep.min(std::f64::consts::TAU);
    let end = start + sweep;
    let segment_count = super::curve_conversion::quarter_turn_spans(sweep);
    let segment_angle = sweep / segment_count as f64;
    let mut knots = vec![start; 3];
    let mut controls = Vec::with_capacity(segment_count * 2 + 1);
    controls.push((start, 1.0));
    for segment in 0..segment_count {
        let segment_start = start + segment as f64 * segment_angle;
        let midpoint = segment_start + segment_angle / 2.0;
        let segment_end = segment_start + segment_angle;
        controls.push((midpoint, (segment_angle / 2.0).cos()));
        controls.push((segment_end, 1.0));
        if segment + 1 < segment_count {
            knots.extend([segment_end; 2]);
        }
    }
    knots.extend([end; 3]);
    Some(AngularBasis { knots, controls })
}

fn offset_analytic(geometry: &SurfaceGeometry, distance: f64) -> Option<SurfaceGeometry> {
    let offset = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => {
            let origin = plane.origin().get();
            let origin =
                FinitePoint3::new(origin.translated(*plane.frame().axis().as_raw(), distance))?;
            SolvedSurfaceGeometry::Plane(cadmpeg_ir::geometry::analytic::PlaneSurface::new(
                origin,
                *plane.frame(),
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) => {
            let radius = cylinder.radius().get();
            let radius = PositiveLength::new(radius + distance)?;
            SolvedSurfaceGeometry::Cylinder(cadmpeg_ir::geometry::analytic::CylinderSurface::new(
                cylinder.origin(),
                *cylinder.frame(),
                radius,
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) => {
            let radius = sphere.radius().get();
            let radius = NonZeroLength::new(radius + distance)?;
            SolvedSurfaceGeometry::Sphere(cadmpeg_ir::geometry::analytic::SphereSurface::new(
                sphere.center(),
                *sphere.frame(),
                radius,
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus)) => {
            let minor_radius = torus.minor_radius().get();
            let minor_radius = NonZeroLength::new(minor_radius + distance)?;
            SolvedSurfaceGeometry::Torus(cadmpeg_ir::geometry::analytic::TorusSurface::new(
                torus.center(),
                *torus.frame(),
                torus.major_radius(),
                minor_radius,
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone)) if cone.ratio().get() == 1.0 => {
            let origin = cone.origin().get();
            let radius = cone.radius().get();
            let half_angle = cone.half_angle().get();
            let origin = FinitePoint3::new(
                origin.translated(*cone.frame().axis().as_raw(), -distance * half_angle.sin()),
            )?;
            let radius = NonNegativeLength::new(radius + distance * half_angle.cos())?;
            SolvedSurfaceGeometry::Cone(cadmpeg_ir::geometry::analytic::ConeSurface::new(
                origin,
                *cone.frame(),
                radius,
                cone.ratio(),
                cone.half_angle(),
            ))
        }
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Cone(_)
            | SolvedSurfaceGeometry::Nurbs(_)
            | SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Transformed(_)
            | SolvedSurfaceGeometry::Unknown { .. },
        )
        | SurfaceGeometry::Procedural { .. } => return None,
    };
    Some(SurfaceGeometry::Solved(offset))
}

fn offset_indicator_parameters(bounds: Option<cadmpeg_ir::geometry::RecordBounds>) -> [f64; 2] {
    bounds
        .and_then(|bounds| match bounds.get() {
            [Some(u0), Some(u1), Some(v0), Some(v1)] => Some([u0.midpoint(u1), v0.midpoint(v1)]),
            _ => None,
        })
        .unwrap_or([0.0, 0.0])
}

fn indicator_normal(ir: &CadIr, surface: &SurfaceId) -> Option<Vector3> {
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| ir.model.procedural_surface_owner(&procedural.id) == Some(surface));
    let parameters =
        procedural.map(|procedural| offset_indicator_parameters(procedural.record_bounds()));
    let parameters = parameters.unwrap_or([0.0, 0.0]);
    let partials = match procedural {
        Some(_) => {
            let index = cadmpeg_ir::index::ModelIndex::new(ir);
            cadmpeg_ir::eval::model_surface_partials_by_id(
                &index,
                surface,
                parameters[0],
                parameters[1],
            )?
        }
        None => {
            // A support with no procedural entry takes `model_surface_mapping`'s
            // direct arm: `surface_partials` on the carrier geometry with zero
            // offset and unit scales. Building the whole `ModelIndex` to serve
            // that one arena lookup is the bulk of this function's cost, so the
            // carrier is resolved here instead. The reverse scan is deliberate:
            // the index maps an arena through a `HashMap` where a repeated
            // identity is won by the last entry, and directory sequence numbers
            // come straight from the card, so duplicate ids are not excluded.
            let carrier = ir
                .model
                .surfaces
                .iter()
                .rev()
                .find(|carrier| carrier.id == *surface)?;
            cadmpeg_ir::eval::surface_partials(&carrier.geometry, parameters[0], parameters[1])?
        }
    };
    unit_vector(partials.du.cross(partials.dv))
}

fn indicator_orientation(
    record: &ParameterRecord,
    indicator: Vector3,
    normal: Vector3,
    global: &ProjectedGlobal,
) -> Option<f64> {
    let precision = global.real_precision();
    let values = [indicator.x, indicator.y, indicator.z];
    let contains = |candidate: Vector3| {
        [candidate.x, candidate.y, candidate.z]
            .into_iter()
            .enumerate()
            .all(|(offset, component)| {
                super::geometry::DeclaredInterval::around(
                    values[offset],
                    record.number_uncertainty(offset + 1, values[offset], precision),
                )
                .contains(component)
            })
    };
    if contains(normal) {
        Some(1.0)
    } else if contains(normal.scale(-1.0)) {
        Some(-1.0)
    } else {
        None
    }
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<ProjectionOutcome, CodecError> {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let composite_index = CompositeIndex::from_ir(ir);
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 108 && matches!(entry.form, -1..=1))
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let coefficients = [
            record.number(1),
            record.number(2),
            record.number(3),
            record.number(4),
        ];
        let [Some(a), Some(b), Some(c), Some(d)] = coefficients else {
            losses.push(entity_loss(entry, "plane coefficients are not numeric"));
            continue;
        };
        if coefficients
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite())
        {
            losses.push(entity_loss(entry, "plane coefficients are not finite"));
            continue;
        }
        let Some(boundary) = record.integer(5) else {
            losses.push(entity_loss(
                entry,
                "plane boundary pointer is not an integer",
            ));
            continue;
        };
        let boundary_sequence = u32::try_from(boundary)
            .ok()
            .filter(|sequence| sequence % 2 == 1)
            .filter(|sequence| entries.contains_key(sequence));
        if (entry.form == 0 && boundary != 0) || (entry.form != 0 && boundary_sequence.is_none()) {
            losses.push(entity_loss(
                entry,
                "plane form and boundary pointer are inconsistent or the boundary target is missing",
            ));
            continue;
        }
        let local_normal = Vector3::new(a, b, c);
        let normal_squared = a * a + b * b + c * c;
        if !normal_squared.is_finite() || normal_squared <= 0.0 {
            losses.push(entity_loss(entry, "plane normal is degenerate"));
            continue;
        }
        let Some(local_normal_unit) = unit_vector(local_normal) else {
            losses.push(entity_loss(entry, "plane normal cannot be normalized"));
            continue;
        };
        let local_u = derive_reference_direction(local_normal_unit);
        let local_v = local_normal_unit.cross(local_u);
        let local_origin = Point3::new(
            a * d / normal_squared * factor,
            b * d / normal_squared * factor,
            c * d / normal_squared * factor,
        );
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
        let Some(u_axis) = transform
            .apply_vector(local_u)
            .and_then(|axis| unit_vector(axis.get()))
        else {
            losses.push(entity_loss(
                entry,
                "plane placement collapses its u direction",
            ));
            continue;
        };
        let Some(v_axis) = transform
            .apply_vector(local_v)
            .and_then(|axis| unit_vector(axis.get()))
        else {
            losses.push(entity_loss(
                entry,
                "plane placement collapses its v direction",
            ));
            continue;
        };
        let Some(normal) = unit_vector(u_axis.cross(v_axis)) else {
            losses.push(entity_loss(entry, "plane placement collapses its normal"));
            continue;
        };
        sequences.record_surface(
            &crate::ids::surface(&crate::ids::Stem::directory(entry.sequence)),
            entry.sequence,
        );
        ir.model.surfaces.push(Surface {
            id: crate::ids::surface(&crate::ids::Stem::directory(entry.sequence)),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    transform
                        .apply_point(local_origin)
                        .ok_or_else(|| {
                            CodecError::malformed("plane placement produces a non-finite origin")
                        })?
                        .get(),
                    normal,
                    u_axis,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
            )),
            source_object: Some(source_object(entry)?),
        });
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 118 && matches!(entry.form, 0 | 1))
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(first_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "first rail pointer is invalid"));
            continue;
        };
        let Some(second_sequence) = record
            .integer(2)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "second rail pointer is invalid"));
            continue;
        };
        let (Some(direction_flag), Some(developable_flag)) = (record.integer(3), record.integer(4))
        else {
            losses.push(entity_loss(entry, "ruled-surface flags are not integers"));
            continue;
        };
        if !matches!(direction_flag, 0 | 1) || !matches!(developable_flag, 0 | 1) {
            losses.push(entity_loss(entry, "ruled-surface flags are not 0 or 1"));
            continue;
        }
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "placed ruled surfaces require transformed child-carrier projection",
            ));
            continue;
        }
        let first_id = crate::ids::curve(&crate::ids::Stem::directory(first_sequence));
        let second_id = crate::ids::curve(&crate::ids::Stem::directory(second_sequence));
        let rails = (
            bounded_nurbs(ir, &first_id, ctx, &composite_index),
            bounded_nurbs(ir, &second_id, ctx, &composite_index),
        );
        let rails = match rails {
            (Ok(first), Ok(second)) => (first, second),
            (Err(error), _) | (_, Err(error)) => {
                losses.push(entity_loss(
                    entry,
                    format!("a rail curve states no NURBS carrier: {error}"),
                ));
                continue;
            }
        };
        let (Some((first, first_interval)), Some((mut second, second_interval))) = rails else {
            losses.push(entity_loss(
                entry,
                "rail curves do not have bounded polynomial or NURBS carriers",
            ));
            continue;
        };
        if entry.form == 0
            && !equal_arc_length_parameterization(
                ir,
                first_sequence,
                second_sequence,
                first_interval,
                second_interval,
                &records,
                global,
            )
        {
            losses.push(entity_loss(
                entry,
                "equal-arc-length ruled projection has no exact normalized arc-length carrier",
            ));
            continue;
        }
        if direction_flag == 1 {
            let knot_sum = second.knots()[0] + second.knots()[second.knots().len() - 1];
            second.reverse_parameterization();
            if second
                .edit_knots(|knots| {
                    for knot in knots {
                        *knot += knot_sum;
                    }
                })
                .is_err()
            {
                losses.push(
                    IgesLossCode::NurbsTransformNonFinite
                        .note("IGES reversed second rail knots are non-finite")
                        .with_provenance(entry.loss_provenance()),
                );
                continue;
            }
        }
        let surface = match ruled_surface_carrier(&first, &second, ctx) {
            Ok(Some(surface)) => surface,
            Ok(None) => {
                losses.push(entity_loss(
                    entry,
                    "ruled rails do not have a finite exact NURBS carrier",
                ));
                continue;
            }
            Err(error) => {
                losses.push(entity_loss(
                    entry,
                    format!("ruled rails state no NURBS carrier: {error}"),
                ));
                continue;
            }
        };
        let surface_id = crate::ids::surface(&crate::ids::Stem::directory(entry.sequence));
        sequences.record_surface(&surface_id, entry.sequence);
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            source_object: Some(source_object(entry)?),
        });
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            ProceduralSurface::new(
                crate::ids::procedural_surface(&crate::ids::Stem::directory(entry.sequence)),
                ProceduralSurfaceDefinition::Ruled {
                    first: crate::ids::curve(&crate::ids::Stem::directory(first_sequence)),
                    second: crate::ids::curve(&crate::ids::Stem::directory(second_sequence)),
                    cache: None,
                },
                Some(
                    RecordBounds::try_new([
                        Some(first_interval[0]),
                        Some(first_interval[1]),
                        Some(second_interval[0]),
                        Some(second_interval[1]),
                    ])
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            ),
        );
        losses.push(
            IgesLossCode::RuledDevelopabilityNotTransferred
                .note("Type 118 developability is retained only in the native entity record")
                .with_provenance(entry.loss_provenance()),
        );
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 122 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(directrix_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "directrix pointer is invalid"));
            continue;
        };
        let Some(directrix_entry) = entries.get(&directrix_sequence).copied() else {
            losses.push(entity_loss(entry, "directrix entity is missing"));
            continue;
        };
        if !tabulated_directrix_type_allowed(
            directrix_entry.entity_type,
            directrix_entry.form,
            global.global_table(),
        ) {
            losses.push(entity_loss(
                entry,
                "directrix entity is outside the effective specification family",
            ));
            continue;
        }
        let coordinates = [record.number(2), record.number(3), record.number(4)];
        let [Some(x), Some(y), Some(z)] = coordinates else {
            losses.push(entity_loss(entry, "generatrix endpoint is not numeric"));
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
        let Some(directrix_id) = curve_carrier_id(directrix_sequence, &entries, &records) else {
            losses.push(entity_loss(
                entry,
                "directrix model-space carrier pointer is invalid",
            ));
            continue;
        };
        let directrix_carrier = match bounded_nurbs(ir, &directrix_id, ctx, &composite_index) {
            Ok(carrier) => carrier,
            Err(error) => {
                losses.push(entity_loss(
                    entry,
                    format!("the directrix states no NURBS carrier: {error}"),
                ));
                continue;
            }
        };
        let Some((directrix, cached_interval)) = directrix_carrier else {
            let Some((directrix_geometry, carrier_interval)) = bounded_evaluable_curve(
                ir,
                &directrix_id,
                global.minimum_resolution_mm(),
                &composite_index,
            ) else {
                losses.push(entity_loss(
                    entry,
                    "directrix has no bounded polynomial, NURBS, or exact evaluable carrier",
                ));
                continue;
            };
            let Some(directrix_solved) = directrix_geometry.solved() else {
                continue;
            };
            let source_interval = source_parameter_interval(&directrix_geometry, carrier_interval);
            let Ok(start) = cadmpeg_ir::eval::curve_point(&directrix_geometry, carrier_interval[0])
            else {
                losses.push(entity_loss(entry, "directrix start cannot be evaluated"));
                continue;
            };
            let Some(start) = transform.apply_point(start.get()) else {
                losses.push(entity_loss(entry, "placement produces a non-finite point"));
                continue;
            };
            let Some(target) = transform
                .apply_point(Point3::new(x * factor, y * factor, z * factor))
                .map(cadmpeg_ir::features::FinitePoint3::get)
            else {
                losses.push(entity_loss(entry, "placement produces a non-finite point"));
                continue;
            };
            let direction = target.vector_from(start.get());
            if !direction.norm().is_finite() || direction.norm() <= 0.0 {
                losses.push(entity_loss(
                    entry,
                    "tabulated direction is zero or non-finite",
                ));
                continue;
            }
            let procedural_directrix = if entry.transform == 0 {
                directrix_id
            } else {
                let placed_id = crate::ids::curve(
                    &crate::ids::Stem::directory(entry.sequence)
                        .tail(crate::ids::Word::PlacedDirectrix),
                );
                sequences.record_curve(&placed_id, entry.sequence);
                ir.model.curves.push(Curve {
                    id: placed_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
                        cadmpeg_ir::geometry::PlacedCurve::try_new(
                            Box::new(directrix_solved.clone()),
                            transform,
                        )
                        .map_err(CodecError::malformed)?,
                    )),
                    source_object: Some(source_object(entry)?),
                });
                placed_id
            };
            let surface_id = crate::ids::surface(&crate::ids::Stem::directory(entry.sequence));
            let procedural_id =
                crate::ids::procedural_surface(&crate::ids::Stem::directory(entry.sequence));
            sequences.record_surface(&surface_id, entry.sequence);
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                    cache: None,
                },
                source_object: Some(source_object(entry)?),
            });
            let _attached = ir.model.add_procedural_surface(
                surface_id,
                cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                    procedural_directrix,
                    Some(source_interval),
                    direction,
                    Some(target),
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .and_then(|admitted_payload| {
                    Ok(ProceduralSurface::new(
                        procedural_id,
                        ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                        Some(
                            RecordBounds::try_new([
                                Some(carrier_interval[0]),
                                Some(carrier_interval[1]),
                                None,
                                None,
                            ])
                            .map_err(|_| {
                                cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
                                    "record bounds must be finite",
                                )
                            })?,
                        ),
                    ))
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            );
            decoded.insert(entry.sequence);
            continue;
        };
        let carrier_interval = bounded_parameter_range_for_curve(
            ir,
            &directrix_id,
            global.minimum_resolution_mm(),
            Some(&composite_index),
        )
        .unwrap_or(cached_interval);
        let source_interval = curve_geometry(ir, &directrix_id)
            .map_or(cached_interval, |geometry| {
                source_parameter_interval(geometry, cached_interval)
            });
        let mut placed_directrix = directrix;
        if entry.transform != 0
            && placed_directrix
                .map_control_points(|point| {
                    transform.apply_point(point.get()).ok_or_else(|| {
                        cadmpeg_ir::geometry::nurbs::NurbsError::EditRefused(
                            "placement produces a non-finite pole".into(),
                        )
                    })
                })
                .is_err()
        {
            losses.push(
                IgesLossCode::NurbsTransformNonFinite
                    .note("IGES placement produces non-finite directrix poles")
                    .with_provenance(entry.loss_provenance()),
            );
            continue;
        }
        let Some(start) =
            cadmpeg_ir::eval::nurbs_curve_point_at(&placed_directrix, cached_interval[0])
        else {
            losses.push(entity_loss(entry, "directrix start cannot be evaluated"));
            continue;
        };
        let Some(target) = transform
            .apply_point(Point3::new(x * factor, y * factor, z * factor))
            .map(cadmpeg_ir::features::FinitePoint3::get)
        else {
            losses.push(entity_loss(entry, "placement produces a non-finite point"));
            continue;
        };
        let direction = target.vector_from(start.get());
        if !direction.norm().is_finite() || direction.norm() <= 0.0 {
            losses.push(entity_loss(
                entry,
                "tabulated direction is zero or non-finite",
            ));
            continue;
        }
        let control_points = placed_directrix
            .pole_rows()
            .raw_points()
            .into_iter()
            .flat_map(|point| [point, point.translated(direction, 1.0)])
            .collect::<Vec<_>>();
        let Ok(_) = u32::try_from(placed_directrix.control_points().len()) else {
            losses.push(entity_loss(entry, "directrix pole count exceeds u32"));
            continue;
        };
        let weights: Option<Vec<Vec<NonZeroReal>>> = placed_directrix.weights().map(|weights| {
            weights
                .iter()
                .map(|weight| vec![*weight, *weight])
                .collect()
        });
        let procedural_directrix = if entry.transform == 0 {
            directrix_id
        } else {
            let placed_id = crate::ids::curve(
                &crate::ids::Stem::directory(entry.sequence)
                    .tail(crate::ids::Word::PlacedDirectrix),
            );
            sequences.record_curve(&placed_id, entry.sequence);
            ir.model.curves.push(Curve {
                id: placed_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                    placed_directrix.clone(),
                )),
                source_object: Some(source_object(entry)?),
            });
            placed_id
        };
        let surface_id = crate::ids::surface(&crate::ids::Stem::directory(entry.sequence));
        let surface = match NurbsSurface::from_checked_lanes(
            NurbsSurfaceAxis::new(
                placed_directrix.degree(),
                placed_directrix.knots().to_vec(),
                placed_directrix.periodic(),
            ),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                control_points.chunks(2).map(<[_]>::to_vec).collect(),
                weights,
            ),
            false,
        ) {
            Ok(nurbs) => nurbs,
            Err(error) => {
                losses.push(entity_loss(
                    entry,
                    format!("tabulated-cylinder carrier cardinalities are inconsistent: {error}"),
                ));
                continue;
            }
        };
        sequences.record_surface(&surface_id, entry.sequence);
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            source_object: Some(source_object(entry)?),
        });
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                procedural_directrix,
                Some(source_interval),
                direction,
                Some(target),
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .and_then(|admitted_payload| {
                Ok(ProceduralSurface::new(
                    crate::ids::procedural_surface(&crate::ids::Stem::directory(entry.sequence)),
                    ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                    Some(
                        RecordBounds::try_new([
                            Some(carrier_interval[0]),
                            Some(carrier_interval[1]),
                            None,
                            None,
                        ])
                        .map_err(|_| {
                            cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
                                "record bounds must be finite",
                            )
                        })?,
                    ),
                ))
            })
            .map_err(cadmpeg_core::CodecError::malformed)?,
        );
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 120 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(axis_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "revolution axis pointer is invalid"));
            continue;
        };
        let Some(generatrix_sequence) = record
            .integer(2)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(
                entry,
                "revolution generatrix pointer is invalid",
            ));
            continue;
        };
        let (Some(start_angle), Some(end_angle)) = (record.number(3), record.number(4)) else {
            losses.push(entity_loss(entry, "revolution angles are not numeric"));
            continue;
        };
        let Some(AngularBasis {
            knots: v_knots,
            controls: angular_controls,
        }) = angular_basis(start_angle, end_angle)
        else {
            losses.push(entity_loss(
                entry,
                "revolution angular interval is not in (0, 2*pi]",
            ));
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
        let axis_id = crate::ids::curve(&crate::ids::Stem::directory(axis_sequence));
        let Some(axis_curve) = ir.model.curves.iter().find(|curve| curve.id == axis_id) else {
            losses.push(entity_loss(entry, "revolution axis carrier is missing"));
            continue;
        };
        let Some(SolvedCurveGeometry::Line(line_curve)) = axis_curve.geometry.solved() else {
            losses.push(entity_loss(
                entry,
                "revolution axis is not a Line Entity carrier",
            ));
            continue;
        };
        let admitted_axis = (line_curve.origin(), line_curve.direction());
        let axis_origin = admitted_axis.0.get();
        let axis_direction = *admitted_axis.1.as_raw();
        let Some(generatrix_id) = curve_carrier_id(generatrix_sequence, &entries, &records) else {
            losses.push(entity_loss(
                entry,
                "revolution model-space carrier pointer is invalid",
            ));
            continue;
        };
        let generatrix_carrier = match bounded_nurbs(ir, &generatrix_id, ctx, &composite_index) {
            Ok(carrier) => carrier,
            Err(error) => {
                losses.push(entity_loss(
                    entry,
                    format!("the generatrix states no NURBS carrier: {error}"),
                ));
                continue;
            }
        };
        let Some((generatrix, cached_interval)) = generatrix_carrier else {
            let Some((directrix_geometry, carrier_interval)) = bounded_evaluable_curve(
                ir,
                &generatrix_id,
                global.minimum_resolution_mm(),
                &composite_index,
            ) else {
                losses.push(entity_loss(
                    entry,
                    "generatrix has no bounded polynomial, NURBS, or exact evaluable carrier",
                ));
                continue;
            };
            let Some(directrix_solved) = directrix_geometry.solved() else {
                continue;
            };
            let source_interval = source_parameter_interval(&directrix_geometry, carrier_interval);
            let mut procedural_directrix = generatrix_id.clone();
            let mut procedural_axis = admitted_axis;
            if entry.transform != 0 {
                let Some(orientation) = similarity_orientation(transform) else {
                    losses.push(entity_loss(
                        entry,
                        "placement cannot preserve the exact revolution parameterization",
                    ));
                    continue;
                };
                procedural_directrix = crate::ids::curve(
                    &crate::ids::Stem::directory(entry.sequence)
                        .tail(crate::ids::Word::PlacedGeneratrix),
                );
                sequences.record_curve(&procedural_directrix, entry.sequence);
                ir.model.curves.push(Curve {
                    id: procedural_directrix.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
                        cadmpeg_ir::geometry::PlacedCurve::try_new(
                            Box::new(directrix_solved.clone()),
                            transform,
                        )
                        .map_err(CodecError::malformed)?,
                    )),
                    source_object: Some(source_object(entry)?),
                });
                let placed_origin =
                    transform
                        .apply_point(admitted_axis.0.get())
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "placement produces a non-finite revolution origin",
                            )
                        })?;
                let Some(placed_direction) = transform
                    .apply_vector(axis_direction)
                    .and_then(|direction| unit_vector(direction.get()))
                    .and_then(|direction| UnitVector3::new(direction.scale(orientation)))
                else {
                    losses.push(entity_loss(
                        entry,
                        "placement collapses the revolution axis",
                    ));
                    continue;
                };
                procedural_axis = (placed_origin, placed_direction);
            }
            let surface_id = crate::ids::surface(&crate::ids::Stem::directory(entry.sequence));
            let procedural_id =
                crate::ids::procedural_surface(&crate::ids::Stem::directory(entry.sequence));
            sequences.record_surface(&surface_id, entry.sequence);
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                    cache: None,
                },
                source_object: Some(source_object(entry)?),
            });
            let _attached = ir.model.add_procedural_surface(
                surface_id,
                cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                    procedural_directrix,
                    procedural_axis,
                    [start_angle, end_angle],
                    None,
                    Some(source_interval),
                    false,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .and_then(|admitted_payload| {
                    Ok(ProceduralSurface::new(
                        procedural_id,
                        ProceduralSurfaceDefinition::Revolution(admitted_payload),
                        Some(
                            RecordBounds::try_new([
                                Some(carrier_interval[0]),
                                Some(carrier_interval[1]),
                                None,
                                None,
                            ])
                            .map_err(|_| {
                                cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
                                    "record bounds must be finite",
                                )
                            })?,
                        ),
                    ))
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            );
            decoded.insert(entry.sequence);
            continue;
        };
        let carrier_interval = bounded_parameter_range_for_curve(
            ir,
            &generatrix_id,
            global.minimum_resolution_mm(),
            Some(&composite_index),
        )
        .unwrap_or(cached_interval);
        let source_interval = curve_geometry(ir, &generatrix_id)
            .map_or(cached_interval, |geometry| {
                source_parameter_interval(geometry, cached_interval)
            });
        let Ok(_) = u32::try_from(generatrix.control_points().len()) else {
            losses.push(entity_loss(entry, "generatrix pole count exceeds u32"));
            continue;
        };
        let Ok(v_count) = u32::try_from(angular_controls.len()) else {
            losses.push(entity_loss(entry, "angular pole count exceeds u32"));
            continue;
        };
        let Some(surface_pole_count) = generatrix
            .control_points()
            .len()
            .checked_mul(angular_controls.len())
        else {
            return Err(refuse_local_limit(
                "iges_revolution_poles",
                MAX_SURFACE_POLES as u64,
                u64::MAX,
            ));
        };
        if surface_pole_count > MAX_SURFACE_POLES {
            return Err(refuse_local_limit(
                "iges_revolution_poles",
                MAX_SURFACE_POLES as u64,
                surface_pole_count as u64,
            ));
        }
        let mut control_points = Vec::with_capacity(surface_pole_count);
        let mut weights = Vec::with_capacity(control_points.capacity());
        let generatrix_points = generatrix.control_points();
        let generatrix_weights = generatrix.pole_rows().weights();
        for (u_index, point) in generatrix_points.iter().enumerate() {
            let delta = point.vector_from(axis_origin);
            let axis_point = axis_origin.translated(axis_direction, delta.dot(axis_direction));
            let radial = point.vector_from(axis_point);
            let u_weight = generatrix_weights
                .as_ref()
                .and_then(|values| values.get(u_index))
                .copied()
                .unwrap_or(1.0);
            for (angle, angular_weight) in &angular_controls {
                let rotated = rotate(radial, axis_direction, *angle);
                let radial_control = rotated.scale(1.0 / angular_weight);
                control_points.push(
                    transform
                        .apply_point(axis_point.translated(radial_control, 1.0))
                        .ok_or_else(|| {
                            CodecError::malformed("placement produces a non-finite revolution pole")
                        })?,
                );
                weights.push(u_weight * angular_weight);
            }
        }
        let surface_id = crate::ids::surface(&crate::ids::Stem::directory(entry.sequence));
        let surface = match NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(
                generatrix.degree(),
                generatrix.knots().to_vec(),
                generatrix.periodic(),
            ),
            NurbsSurfaceAxis::new(
                2,
                v_knots,
                super::curve_conversion::angularly_equal(
                    end_angle - start_angle,
                    std::f64::consts::TAU,
                ),
            ),
            NurbsSurfaceLanes::new(
                control_points
                    .chunks(v_count as usize)
                    .map(<[_]>::to_vec)
                    .collect(),
                Some(weights)
                    .map(|values| values.chunks(v_count as usize).map(<[_]>::to_vec).collect()),
            ),
            false,
        ) {
            Ok(nurbs) => nurbs,
            Err(error) => {
                losses.push(entity_loss(
                    entry,
                    format!(
                        "surface-of-revolution carrier cardinalities are inconsistent: {error}"
                    ),
                ));
                continue;
            }
        };
        sequences.record_surface(&surface_id, entry.sequence);
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            source_object: Some(source_object(entry)?),
        });
        let mut procedural_directrix =
            crate::ids::curve(&crate::ids::Stem::directory(generatrix_sequence));
        let mut procedural_axis = admitted_axis;
        let procedural_is_exact = if entry.transform == 0 {
            true
        } else if let Some(orientation) = similarity_orientation(transform) {
            // This arm is the transformed route, so the generatrix is placed
            // here rather than carried past the untransformed one.
            let mut placed_generatrix = generatrix.clone();
            if placed_generatrix
                .map_control_points(|point| {
                    transform.apply_point(point.get()).ok_or_else(|| {
                        cadmpeg_ir::geometry::nurbs::NurbsError::EditRefused(
                            "placement produces a non-finite pole".into(),
                        )
                    })
                })
                .is_err()
            {
                losses.push(
                    IgesLossCode::NurbsTransformNonFinite
                        .note("IGES placement produces non-finite generatrix poles")
                        .with_provenance(entry.loss_provenance()),
                );
                continue;
            }
            procedural_directrix = crate::ids::curve(
                &crate::ids::Stem::directory(entry.sequence)
                    .tail(crate::ids::Word::PlacedGeneratrix),
            );
            sequences.record_curve(&procedural_directrix, entry.sequence);
            ir.model.curves.push(Curve {
                id: procedural_directrix.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(placed_generatrix)),
                source_object: Some(source_object(entry)?),
            });
            let placed_origin = transform
                .apply_point(admitted_axis.0.get())
                .ok_or_else(|| {
                    CodecError::malformed("placement produces a non-finite revolution origin")
                })?;
            let Some(placed_direction) = transform
                .apply_vector(axis_direction)
                .and_then(|direction| unit_vector(direction.get()))
                .and_then(|direction| UnitVector3::new(direction.scale(orientation)))
            else {
                losses.push(entity_loss(
                    entry,
                    "placement collapses the revolution axis",
                ));
                continue;
            };
            procedural_axis = (placed_origin, placed_direction);
            true
        } else {
            false
        };
        if procedural_is_exact {
            let _attached = ir.model.add_procedural_surface(
                surface_id,
                cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                    procedural_directrix,
                    procedural_axis,
                    [start_angle, end_angle],
                    None,
                    Some(source_interval),
                    false,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .and_then(|admitted_payload| {
                    Ok(ProceduralSurface::new(
                        crate::ids::procedural_surface(&crate::ids::Stem::directory(
                            entry.sequence,
                        )),
                        ProceduralSurfaceDefinition::Revolution(admitted_payload),
                        Some(
                            RecordBounds::try_new([
                                Some(carrier_interval[0]),
                                Some(carrier_interval[1]),
                                None,
                                None,
                            ])
                            .map_err(|_| {
                                cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
                                    "record bounds must be finite",
                                )
                            })?,
                        ),
                    ))
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            );
        }
        decoded.insert(entry.sequence);
    }

    'surface: for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 128 && (0..=9).contains(&entry.form))
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let indices = [record.integer(1), record.integer(2)];
        let degrees = [record.integer(3), record.integer(4)];
        let [Some(raw_k1), Some(raw_k2)] = indices else {
            losses.push(entity_loss(
                entry,
                "surface upper indices K1 or K2 are invalid",
            ));
            continue;
        };
        let [Some(k1), Some(k2)] = [raw_k1, raw_k2].map(|value| usize::try_from(value).ok()) else {
            losses.push(entity_loss(
                entry,
                "surface upper indices K1 or K2 are invalid",
            ));
            continue;
        };
        let [Some(u_degree), Some(v_degree)] =
            degrees.map(|value| value.and_then(|v| u32::try_from(v).ok()))
        else {
            losses.push(entity_loss(entry, "surface degrees M1 or M2 are invalid"));
            continue;
        };
        let [u_degree_usize, v_degree_usize] = [u_degree, v_degree].map(|degree| degree as usize);
        if k1 < u_degree_usize || k2 < v_degree_usize {
            losses.push(entity_loss(
                entry,
                "surface pole counts are smaller than their degrees plus one",
            ));
            continue;
        }
        let requested = u64::try_from(raw_k1)
            .ok()
            .and_then(|value| value.checked_add(1))
            .and_then(|u_count| {
                u64::try_from(raw_k2)
                    .ok()
                    .and_then(|value| value.checked_add(1))
                    .and_then(|v_count| u_count.checked_mul(v_count))
            });
        match requested {
            None => {
                return Err(refuse_local_limit(
                    "iges_surface_poles",
                    MAX_SURFACE_POLES as u64,
                    u64::MAX,
                ));
            }
            Some(requested) if requested > MAX_SURFACE_POLES as u64 => {
                return Err(refuse_local_limit(
                    "iges_surface_poles",
                    MAX_SURFACE_POLES as u64,
                    requested,
                ));
            }
            Some(_) => {}
        }
        let flags = (5..=9)
            .map(|index| record.integer(index))
            .collect::<Vec<_>>();
        if flags.iter().any(|flag| !matches!(flag, Some(0 | 1))) {
            losses.push(entity_loss(
                entry,
                "one or more surface flags are not 0 or 1",
            ));
            continue;
        }
        let (Some(u_count), Some(v_count)) = (k1.checked_add(1), k2.checked_add(1)) else {
            losses.push(entity_loss(entry, "surface pole count overflows"));
            continue;
        };
        let (Ok(_), Ok(v_count_u32)) = (u32::try_from(u_count), u32::try_from(v_count)) else {
            losses.push(entity_loss(entry, "surface pole dimensions exceed u32"));
            continue;
        };
        let Some(pole_count) = u_count.checked_mul(v_count) else {
            losses.push(entity_loss(entry, "surface pole grid size overflows"));
            continue;
        };
        if pole_count > MAX_SURFACE_POLES {
            return Err(refuse_local_limit(
                "iges_surface_poles",
                MAX_SURFACE_POLES as u64,
                pole_count as u64,
            ));
        }
        let Some(u_knot_count) = u_count
            .checked_add(u_degree_usize)
            .and_then(|value| value.checked_add(1))
        else {
            losses.push(entity_loss(entry, "u-knot count overflows"));
            continue;
        };
        let Some(v_knot_count) = v_count
            .checked_add(v_degree_usize)
            .and_then(|value| value.checked_add(1))
        else {
            losses.push(entity_loss(entry, "v-knot count overflows"));
            continue;
        };
        let u_knot_start = 10_usize;
        let Some(v_knot_start) = u_knot_start.checked_add(u_knot_count) else {
            losses.push(entity_loss(entry, "v-knot offset overflows"));
            continue;
        };
        let Some(weight_start) = v_knot_start.checked_add(v_knot_count) else {
            losses.push(entity_loss(entry, "surface weight offset overflows"));
            continue;
        };
        let Some(pole_start) = weight_start.checked_add(pole_count) else {
            losses.push(entity_loss(entry, "surface pole offset overflows"));
            continue;
        };
        let Some(pole_value_count) = pole_count.checked_mul(3) else {
            losses.push(entity_loss(entry, "surface pole value count overflows"));
            continue;
        };
        let Some(range_start) = pole_start.checked_add(pole_value_count) else {
            losses.push(entity_loss(
                entry,
                "surface parameter-range offset overflows",
            ));
            continue;
        };
        let collect_numbers = |start: usize, count: usize| -> Option<Vec<f64>> {
            (start..start.checked_add(count)?)
                .map(|index| record.number(index).filter(|value| value.is_finite()))
                .collect()
        };
        let Some(u_knots) = collect_numbers(u_knot_start, u_knot_count) else {
            losses.push(entity_loss(
                entry,
                "u-knot vector is truncated or non-finite",
            ));
            continue;
        };
        let Some(v_knots) = collect_numbers(v_knot_start, v_knot_count) else {
            losses.push(entity_loss(
                entry,
                "v-knot vector is truncated or non-finite",
            ));
            continue;
        };
        if !knots_nondecreasing(&u_knots) || !knots_nondecreasing(&v_knots) {
            losses.push(entity_loss(entry, "surface knot vector is decreasing"));
            continue;
        }
        let Some(native_weights) = collect_numbers(weight_start, pole_count) else {
            losses.push(entity_loss(
                entry,
                "surface weight vector is truncated or non-finite",
            ));
            continue;
        };
        if native_weights.iter().any(|weight| *weight <= 0.0) {
            losses.push(entity_loss(
                entry,
                "surface weights are not strictly positive",
            ));
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
            losses.push(entity_loss(entry, "polynomial surface has unequal weights"));
            continue;
        }
        if !polynomial && equal_weights {
            losses.push(entity_loss(
                entry,
                "rational surface has equal weights but PROP3 declares rational",
            ));
            continue;
        }
        let Some(native_poles) = collect_numbers(pole_start, pole_value_count) else {
            losses.push(entity_loss(
                entry,
                "surface poles are truncated or non-finite",
            ));
            continue;
        };
        let Some(ranges) = collect_numbers(range_start, 4) else {
            losses.push(entity_loss(entry, "surface parameter ranges are missing"));
            continue;
        };
        let clamp_range =
            |start_index: usize, values: [f64; 2], domain: [f64; 2]| -> Option<[f64; 2]> {
                let mut clamped = values;
                for (offset, bound) in clamped.iter_mut().enumerate() {
                    let uncertainty =
                        record.number_uncertainty(start_index + offset, *bound, precision);
                    if *bound < domain[0]
                        && super::geometry::DeclaredInterval::around(*bound, uncertainty)
                            .contains(domain[0])
                    {
                        *bound = domain[0];
                    } else if *bound > domain[1]
                        && super::geometry::DeclaredInterval::around(*bound, uncertainty)
                            .contains(domain[1])
                    {
                        *bound = domain[1];
                    }
                }
                (clamped[0] < clamped[1] && clamped[0] >= domain[0] && clamped[1] <= domain[1])
                    .then_some(clamped)
            };
        let Some(u_range) = clamp_range(
            range_start,
            [ranges[0], ranges[1]],
            [u_knots[u_degree_usize], u_knots[u_count]],
        ) else {
            losses.push(entity_loss(
                entry,
                "u parameter range is empty or lies outside its knot domain",
            ));
            continue;
        };
        let Some(v_range) = clamp_range(
            range_start + 2,
            [ranges[2], ranges[3]],
            [v_knots[v_degree_usize], v_knots[v_count]],
        ) else {
            losses.push(entity_loss(
                entry,
                "v parameter range is empty or lies outside its knot domain",
            ));
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
        let native_points = native_poles
            .chunks_exact(3)
            .map(|point| Point3::new(point[0] * factor, point[1] * factor, point[2] * factor))
            .collect::<Vec<_>>();
        let mut control_points = Vec::with_capacity(pole_count);
        let mut weights = (!polynomial).then(|| Vec::with_capacity(pole_count));
        for u in 0..u_count {
            for v in 0..v_count {
                let native_index = v * u_count + u;
                control_points.push(
                    transform
                        .apply_point(native_points[native_index])
                        .ok_or_else(|| {
                            CodecError::malformed("placement produces a non-finite surface pole")
                        })?,
                );
                if let Some(weights) = &mut weights {
                    weights.push(native_weights[native_index]);
                }
            }
        }
        let surface = match NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(u_degree, u_knots, flags[3] == Some(1)),
            NurbsSurfaceAxis::new(v_degree, v_knots, flags[4] == Some(1)),
            NurbsSurfaceLanes::new(
                control_points
                    .chunks(v_count_u32 as usize)
                    .map(<[_]>::to_vec)
                    .collect(),
                weights.map(|values| {
                    values
                        .chunks(v_count_u32 as usize)
                        .map(<[_]>::to_vec)
                        .collect()
                }),
            ),
            false,
        ) {
            Ok(nurbs) => nurbs,
            Err(error) => {
                losses.push(entity_loss(
                    entry,
                    format!("spline surface cardinalities are inconsistent: {error}"),
                ));
                continue;
            }
        };
        for (declared, fixed_axis, fixed_range, varying_range, direction) in [
            (
                flags[0] == Some(1),
                SurfaceParameterAxis::U,
                u_range,
                v_range,
                "U",
            ),
            (
                flags[1] == Some(1),
                SurfaceParameterAxis::V,
                v_range,
                u_range,
                "V",
            ),
        ] {
            let Some(actual) = surface_boundary_is_closed(
                &surface,
                fixed_axis,
                fixed_range,
                varying_range,
                global.minimum_resolution_mm(),
            ) else {
                losses.push(entity_loss(
                    entry,
                    format!("{direction}-closed surface boundary cannot be evaluated"),
                ));
                continue 'surface;
            };
            if actual != declared {
                losses.push(entity_loss(
                    entry,
                    format!("{direction}-closed surface flag disagrees with boundary curves"),
                ));
                continue 'surface;
            }
        }
        let surface_id = crate::ids::surface(&crate::ids::Stem::directory(entry.sequence));
        sequences.record_surface(&surface_id, entry.sequence);
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
            source_object: Some(source_object(entry)?),
        });
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            ProceduralSurface::new(
                crate::ids::procedural_surface(&crate::ids::Stem::directory(entry.sequence)),
                ProceduralSurfaceDefinition::Exact(
                    cadmpeg_ir::geometry::surface_payloads::ExactSurfacePayload::try_new(
                        cadmpeg_ir::geometry::ExactSpline::Legacy {
                            ranges: [u_range, v_range],
                            extension: 0,
                            cache: None,
                        },
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
                Some(
                    RecordBounds::try_new([
                        Some(u_range[0]),
                        Some(u_range[1]),
                        Some(v_range[0]),
                        Some(v_range[1]),
                    ])
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            ),
        );
        decoded.insert(entry.sequence);
    }

    // No `ModelIndex` can be hoisted out of this loop: every accepted offset
    // surface appends to `ir.model`, and an offset may serve as the support
    // of a later one in the same pass, so an index built up front would miss
    // surfaces that must be resolvable by the time they are referenced.
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 140 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let components = [record.number(1), record.number(2), record.number(3)];
        let [Some(x), Some(y), Some(z)] = components else {
            losses.push(entity_loss(entry, "offset indicator is not numeric"));
            continue;
        };
        let indicator = Vector3::new(x, y, z);
        let Some(indicator) = declared_unit_vector(record, 1, indicator, global.real_precision())
        else {
            losses.push(entity_loss(entry, "offset indicator is not a unit vector"));
            continue;
        };
        let Some(distance) = record
            .number(4)
            .filter(|value| value.is_finite() && *value != 0.0)
        else {
            losses.push(entity_loss(entry, "offset distance is zero or non-finite"));
            continue;
        };
        let Some(support_sequence) = record
            .integer(5)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "offset support pointer is invalid"));
            continue;
        };
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "placed offset surfaces require transformed support projection",
            ));
            continue;
        }
        let support_id = crate::ids::surface(&crate::ids::Stem::directory(support_sequence));
        let Some(support) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == support_id)
        else {
            losses.push(entity_loss(entry, "offset support surface is missing"));
            continue;
        };
        let distance = distance * factor;
        let Some(normal) = indicator_normal(ir, &support_id) else {
            losses.push(entity_loss(
                entry,
                "support normal cannot be evaluated at the offset-indicator parameters",
            ));
            continue;
        };
        let Some(orientation) = indicator_orientation(record, indicator, normal, global) else {
            losses.push(entity_loss(
                entry,
                "offset indicator is not the support normal at the designated parameters",
            ));
            continue;
        };
        let signed_distance = distance * orientation;
        let Some(geometry) = offset_analytic(&support.geometry, signed_distance) else {
            losses.push(entity_loss(
                entry,
                "support surface has no exact analytic offset carrier",
            ));
            continue;
        };
        let regular = match &geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => true,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                let radius = sphere_surface.radius().get();
                radius > 0.0
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                torus_surface.minor_radius().get() > 0.0
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                let radius = cone_surface.radius().get();
                radius > 0.0
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => true,
            SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Nurbs(_)
                | SolvedSurfaceGeometry::Polygonal(_)
                | SolvedSurfaceGeometry::Transformed(_)
                | SolvedSurfaceGeometry::Unknown { .. },
            )
            | SurfaceGeometry::Procedural { .. } => false,
        };
        if !regular {
            losses.push(entity_loss(
                entry,
                "offset collapses or reverses the analytic carrier",
            ));
            continue;
        }
        let surface_id = crate::ids::surface(&crate::ids::Stem::directory(entry.sequence));
        sequences.record_surface(&surface_id, entry.sequence);
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry,
            source_object: Some(source_object(entry)?),
        });
        let _attached = ir.model.add_procedural_surface(
            surface_id,
            cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                support_id,
                signed_distance,
                Some(0),
                Some(0),
                false,
                cadmpeg_ir::geometry::OffsetExtension::Legacy {
                    flags: cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                    cache: None,
                },
            )
            .map(|admitted_payload| {
                ProceduralSurface::new(
                    crate::ids::procedural_surface(&crate::ids::Stem::directory(entry.sequence)),
                    ProceduralSurfaceDefinition::Offset(admitted_payload),
                    None,
                )
            })
            .map_err(cadmpeg_core::CodecError::malformed)?,
        );
        decoded.insert(entry.sequence);
    }

    Ok(ProjectionOutcome { decoded, losses })
}

#[cfg(test)]
mod tests;
