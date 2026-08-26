// SPDX-License-Identifier: Apache-2.0
//! Analytic and free-form surface projection.

use super::composite::{bounded_parameter_range_for_curve, CompositeIndex};
use super::geometry::{
    declared_unit_vector, entity_loss, resolve_transform, source_object, ProjectionOutcome,
};
use crate::directory::DirectoryEntry;
use crate::global::{Dialect, ProjectedGlobal};
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{alloc_filled, refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    derive_reference_direction, knots_nondecreasing, Curve, CurveGeometry, NurbsCurve,
    NurbsSurface, ProceduralSurface, ProceduralSurfaceDefinition, SplineSurfaceParameters, Surface,
    SurfaceGeometry, SurfaceParameterAxis,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const MAX_SURFACE_POLES: usize = 1_000_000;

fn tabulated_directrix_type_allowed(entity_type: i64, form: i64, dialect: Dialect) -> bool {
    if matches!(dialect, Dialect::V4_0) {
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

fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let length = vector.norm();
    (length.is_finite() && length > 0.0).then(|| vector.scale(1.0 / length))
}

fn similarity_orientation(transform: super::geometry::Affine) -> Option<f64> {
    let column = |index| {
        Vector3::new(
            transform.rows[0][index],
            transform.rows[1][index],
            transform.rows[2][index],
        )
    };
    let [x, y, z] = [column(0), column(1), column(2)];
    let squared_scale = x.dot(x);
    if !squared_scale.is_finite() || squared_scale <= 0.0 {
        return None;
    }
    let tolerance = squared_scale * 1.0e-10;
    if (y.dot(y) - squared_scale).abs() > tolerance
        || (z.dot(z) - squared_scale).abs() > tolerance
        || x.dot(y).abs() > tolerance
        || x.dot(z).abs() > tolerance
        || y.dot(z).abs() > tolerance
    {
        return None;
    }
    let determinant = x.dot(y.cross(z));
    let determinant_tolerance = squared_scale.sqrt() * squared_scale * 1.0e-10;
    (determinant.is_finite() && determinant.abs() > determinant_tolerance)
        .then(|| determinant.signum())
}

fn bounded_nurbs(
    ir: &CadIr,
    sequence: u32,
    ctx: Option<&DecodeContext<'_>>,
    index: &CompositeIndex,
) -> Option<(NurbsCurve, [f64; 2])> {
    let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
    super::composite::bounded_nurbs_for_curve(ir, &curve_id, ctx, Some(index))
}

fn constant_speed_curve(geometry: &CurveGeometry) -> bool {
    match geometry {
        CurveGeometry::Line { .. } => true,
        CurveGeometry::Circle { radius, .. } => radius.is_finite() && *radius > 0.0,
        CurveGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => {
            major_radius.is_finite()
                && minor_radius.is_finite()
                && *major_radius > 0.0
                && *minor_radius > 0.0
                && major_radius == minor_radius
        }
        CurveGeometry::Nurbs(curve) => {
            curve.degree == 1
                && curve.weights.is_none()
                && curve.control_points.len() == 2
                && curve
                    .control_points
                    .iter()
                    .all(|point| [point.x, point.y, point.z].into_iter().all(f64::is_finite))
                && curve.control_points[0]
                    .distance(curve.control_points[1])
                    .is_finite()
                && curve.control_points[0].distance(curve.control_points[1]) > 0.0
                && curve.knots.len() == 4
                && curve.knots[0] == curve.knots[1]
                && curve.knots[2] == curve.knots[3]
                && curve.knots[1].is_finite()
                && curve.knots[1] < curve.knots[2]
                && curve.knots[2].is_finite()
        }
        _ => false,
    }
}

fn equal_arc_length_parameterization(
    ir: &CadIr,
    first_sequence: u32,
    second_sequence: u32,
    first_interval: [f64; 2],
    second_interval: [f64; 2],
) -> bool {
    // A normalized parameter is an arc-length parameter only for a constant-
    // speed carrier. The test is deliberately structural; numerical sampling
    // cannot prove the Form 0 correspondence.
    let curve_geometry = |sequence| {
        ir.model
            .curves
            .iter()
            .find(|curve| curve.id == CurveId(format!("iges:model:curve#D{sequence}")))
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
    constant_speed_curve(first) && constant_speed_curve(second)
}

fn bounded_evaluable_curve(
    ir: &CadIr,
    sequence: u32,
    tolerance: f64,
    index: &CompositeIndex,
) -> Option<(CurveGeometry, [f64; 2])> {
    let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
    let curve = index.curve_by_id(ir, &curve_id)?;
    if matches!(
        &curve.geometry,
        CurveGeometry::Composite { .. }
            | CurveGeometry::Procedural { .. }
            | CurveGeometry::Unknown { .. }
    ) {
        return None;
    }
    let parameter_interval =
        super::composite::bounded_parameter_range_for_curve(ir, &curve_id, tolerance, Some(index))?;
    if !parameter_interval[0].is_finite()
        || !parameter_interval[1].is_finite()
        || parameter_interval[0] >= parameter_interval[1]
    {
        return None;
    }
    let geometry = curve.geometry.clone();
    parameter_interval
        .into_iter()
        .all(|parameter| cadmpeg_ir::eval::curve_point(&geometry, parameter).is_some())
        .then_some((geometry, parameter_interval))
}

fn is_line_carrier(geometry: &CurveGeometry, depth: usize) -> bool {
    if depth > 256 {
        return false;
    }
    match geometry {
        CurveGeometry::Line { .. } => true,
        CurveGeometry::Transformed { basis, .. } => is_line_carrier(basis, depth + 1),
        _ => false,
    }
}

fn source_parameter_interval(geometry: &CurveGeometry, carrier_interval: [f64; 2]) -> [f64; 2] {
    if is_line_carrier(geometry, 0) {
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

#[derive(Clone)]
struct HomogeneousBezierSpan {
    domain: [f64; 2],
    controls: Vec<[f64; 4]>,
}

fn insert_homogeneous_curve_knot(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; 4]>,
    knot: f64,
) -> Option<()> {
    let count = controls.len();
    let span = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1])?;
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Some(());
    }
    let mut inserted = alloc_filled(
        count.checked_add(1)?,
        [0.0; 4],
        "iges surface knot insertion",
    )
    .ok()?;
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[span - multiplicity + 1..].copy_from_slice(&controls[span - multiplicity..]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Some(())
}

fn homogeneous_bezier_spans(curve: &NurbsCurve) -> Option<Vec<HomogeneousBezierSpan>> {
    let degree = usize::try_from(curve.degree).ok()?;
    let count = curve.control_points.len();
    if count <= degree
        || curve.knots.len() != count.checked_add(degree)?.checked_add(1)?
        || !knots_nondecreasing(&curve.knots)
    {
        return None;
    }
    let weights = curve.weights.as_deref().map_or_else(
        || cadmpeg_core::decode::alloc_filled(count, 1.0, "iges_surface_closure_weights").ok(),
        |weights| Some(weights.to_owned()),
    )?;
    if weights.len() != count
        || curve.control_points.iter().any(|point| {
            [point.x, point.y, point.z]
                .into_iter()
                .any(|value| !value.is_finite())
        })
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
    {
        return None;
    }
    let mut controls = curve
        .control_points
        .iter()
        .zip(weights)
        .map(|(point, weight)| [weight * point.x, weight * point.y, weight * point.z, weight])
        .collect::<Vec<_>>();
    if controls.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }

    if degree == 0 {
        let mut spans = Vec::new();
        for (index, window) in curve.knots.windows(2).enumerate() {
            if window[0] < window[1] {
                spans.push(HomogeneousBezierSpan {
                    domain: [window[0], window[1]],
                    controls: vec![*controls.get(index)?],
                });
            }
        }
        return (!spans.is_empty()).then_some(spans);
    }

    let mut knots = curve.knots.clone();
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    let mut internal = knots[degree + 1..count]
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_homogeneous_curve_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let mut boundaries = knots[degree..=controls.len()].to_vec();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let spans = boundaries
        .windows(2)
        .enumerate()
        .filter_map(|(index, domain)| {
            (domain[0] < domain[1]).then(|| {
                let start = index.checked_mul(degree)?;
                Some(HomogeneousBezierSpan {
                    domain: [domain[0], domain[1]],
                    controls: controls.get(start..=start + degree)?.to_vec(),
                })
            })?
        })
        .collect::<Vec<_>>();
    (!spans.is_empty()).then_some(spans)
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

fn split_homogeneous_bezier_span(
    span: &HomogeneousBezierSpan,
    cut: f64,
) -> Option<(HomogeneousBezierSpan, HomogeneousBezierSpan)> {
    if !cut.is_finite() || cut <= span.domain[0] || cut >= span.domain[1] {
        return None;
    }
    let width = span.domain[1] - span.domain[0];
    if !width.is_finite() || width <= 0.0 {
        return None;
    }
    let parameter = (cut - span.domain[0]) / width;
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
    let width = domain[1] - domain[0];
    if !width.is_finite() || width <= 0.0 {
        return None;
    }
    let mut boundaries = Vec::with_capacity(spans.len().checked_add(1)?);
    for span in spans {
        for value in span.domain {
            let normalized = (value - domain[0]) / width;
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
    let width = domain[1] - domain[0];
    if !width.is_finite() || width <= 0.0 {
        return None;
    }
    let mut partitioned = Vec::new();
    for span in spans {
        let start = (span.domain[0] - domain[0]) / width;
        let end = (span.domain[1] - domain[0]) / width;
        if !start.is_finite() || !end.is_finite() || start >= end {
            return None;
        }
        let cuts = boundaries
            .iter()
            .copied()
            .filter(|boundary| start < *boundary && *boundary < end)
            .map(|boundary| domain[0] + boundary * width)
            .collect::<Vec<_>>();
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

fn curve_weights(curve: &NurbsCurve) -> Option<Vec<f64>> {
    let count = curve.control_points.len();
    let weights = curve.weights.as_deref().map_or_else(
        || std::iter::repeat_n(1.0, count).collect(),
        <[f64]>::to_vec,
    );
    (weights.len() == count
        && weights
            .iter()
            .all(|weight| weight.is_finite() && *weight > 0.0))
    .then_some(weights)
}

fn projectively_shared_weights(first: &NurbsCurve, second: &NurbsCurve) -> Option<Vec<f64>> {
    let first_weights = curve_weights(first)?;
    let second_weights = curve_weights(second)?;
    if first_weights.len() != second_weights.len() {
        return None;
    }
    let scale = *second_weights.first()? / *first_weights.first()?;
    if !scale.is_finite()
        || scale <= 0.0
        || first_weights
            .iter()
            .zip(&second_weights)
            .any(|(first, second)| *first * scale != *second)
    {
        return None;
    }
    Some(first_weights)
}

fn same_basis_ruled_surface(
    first: &NurbsCurve,
    second: &NurbsCurve,
    weights: &[f64],
) -> Option<NurbsSurface> {
    let u_count = u32::try_from(first.control_points.len()).ok()?;
    let surface_weights = weights
        .iter()
        .copied()
        .flat_map(|weight| [weight, weight])
        .collect::<Vec<_>>();
    let weights = if surface_weights.iter().all(|weight| *weight == 1.0) {
        None
    } else {
        Some(surface_weights)
    };
    Some(NurbsSurface {
        u_degree: first.degree,
        v_degree: 1,
        u_knots: first.knots.clone(),
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count,
        v_count: 2,
        control_points: first
            .control_points
            .iter()
            .copied()
            .zip(second.control_points.iter().copied())
            .flat_map(|(first, second)| [first, second])
            .collect(),
        weights,
        u_periodic: first.periodic && second.periodic,
        v_periodic: false,
    })
}

fn admit_surface_pole_count(ctx: Option<&DecodeContext<'_>>, pole_count: usize) -> Option<()> {
    if pole_count > MAX_SURFACE_POLES {
        if let Some(ctx) = ctx {
            let _ = ctx.refuse_codec_limit(
                "iges_surface_poles",
                MAX_SURFACE_POLES as u64,
                pole_count as u64,
                None,
            );
        }
        return None;
    }
    Some(())
}

fn ruled_surface_carrier(
    first: &NurbsCurve,
    second: &NurbsCurve,
    ctx: Option<&DecodeContext<'_>>,
) -> Option<NurbsSurface> {
    if first.degree == second.degree
        && first.knots == second.knots
        && first.control_points.len() == second.control_points.len()
    {
        if let Some(weights) = projectively_shared_weights(first, second) {
            admit_surface_pole_count(ctx, first.control_points.len().checked_mul(2)?)?;
            return same_basis_ruled_surface(first, second, &weights);
        }
    }

    let degree = usize::try_from(first.degree)
        .ok()?
        .checked_add(usize::try_from(second.degree).ok()?)?;
    if degree == 0 {
        return None;
    }
    let spans = aligned_homogeneous_spans(first, second)?;
    let u_count = spans.len().checked_mul(degree)?.checked_add(1)?;
    let pole_count = u_count.checked_mul(2)?;
    admit_surface_pole_count(ctx, pole_count)?;
    let mut homogeneous = Vec::with_capacity(pole_count);
    let mut u_knots = Vec::with_capacity(u_count.checked_add(degree)?.checked_add(1)?);
    for (span_index, (first_span, second_span)) in spans.iter().enumerate() {
        if first_span.controls.len() != usize::try_from(first.degree).ok()?.checked_add(1)?
            || second_span.controls.len() != usize::try_from(second.degree).ok()?.checked_add(1)?
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
    Some(NurbsSurface {
        u_degree: u32::try_from(degree).ok()?,
        v_degree: 1,
        u_knots,
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: u32::try_from(u_count).ok()?,
        v_count: 2,
        control_points,
        weights,
        u_periodic: first.periodic && second.periodic,
        v_periodic: false,
    })
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
    if first.degree != second.degree
        || first.knots != second.knots
        || first_spans.len() != second_spans.len()
    {
        return None;
    }
    let degree = usize::try_from(first.degree).ok()?;
    let product_degree = degree.checked_mul(2)?;
    let binomial = |n: usize, k: usize| {
        let k = k.min(n - k);
        (1..=k).fold(1.0, |value, factor| {
            value * (n - k + factor) as f64 / factor as f64
        })
    };
    for (first_span, second_span) in first_spans.iter().zip(second_spans) {
        if first_span.domain[1] <= range[0] || first_span.domain[0] >= range[1] {
            continue;
        }
        if first_span.domain != second_span.domain {
            return None;
        }
        let first_weight = first_span
            .controls
            .iter()
            .map(|control| control[3])
            .fold(f64::INFINITY, f64::min);
        let second_weight = second_span
            .controls
            .iter()
            .map(|control| control[3])
            .fold(f64::INFINITY, f64::min);
        let threshold = resolution * first_weight * second_weight / 3.0_f64.sqrt();
        if !threshold.is_finite() {
            return None;
        }
        for product_index in 0..=product_degree {
            let mut cross = [0.0; 3];
            let lower = product_index.saturating_sub(degree);
            let upper = product_index.min(degree);
            for first_index in lower..=upper {
                let second_index = product_index - first_index;
                let coefficient = binomial(degree, first_index) * binomial(degree, second_index)
                    / binomial(product_degree, product_index);
                for (axis, component) in cross.iter_mut().enumerate() {
                    *component += coefficient
                        * (first_span.controls[first_index][axis]
                            * second_span.controls[second_index][3]
                            - second_span.controls[second_index][axis]
                                * first_span.controls[first_index][3]);
                }
            }
            if cross
                .into_iter()
                .any(|component| !component.is_finite() || component.abs() > threshold)
            {
                return Some(false);
            }
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

fn reverse_knots(knots: &[f64]) -> Option<Vec<f64>> {
    let first = *knots.first()?;
    let last = *knots.last()?;
    Some(knots.iter().rev().map(|knot| first + last - knot).collect())
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
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => Some(SurfaceGeometry::Plane {
            origin: origin.translated(*normal, distance),
            normal: *normal,
            u_axis: *u_axis,
        }),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => Some(SurfaceGeometry::Cylinder {
            origin: *origin,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: radius + distance,
        }),
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => Some(SurfaceGeometry::Sphere {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: radius + distance,
        }),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => Some(SurfaceGeometry::Torus {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            major_radius: *major_radius,
            minor_radius: minor_radius + distance,
        }),
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if *ratio == 1.0 => Some(SurfaceGeometry::Cone {
            origin: origin.translated(*axis, -distance * half_angle.sin()),
            axis: *axis,
            ref_direction: *ref_direction,
            radius: radius + distance * half_angle.cos(),
            ratio: *ratio,
            half_angle: *half_angle,
        }),
        SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn offset_indicator_parameters(bounds: Option<[Option<f64>; 4]>) -> [f64; 2] {
    bounds
        .and_then(|bounds| match bounds {
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
        .find(|procedural| procedural.surface == *surface);
    let parameters =
        procedural.map(|procedural| offset_indicator_parameters(procedural.record_bounds));
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
        let Some(u_axis) = unit_vector(transform.vector(local_u)) else {
            losses.push(entity_loss(
                entry,
                "plane placement collapses its u direction",
            ));
            continue;
        };
        let Some(v_axis) = unit_vector(transform.vector(local_v)) else {
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
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("iges:model:surface#D{}", entry.sequence)),
            geometry: SurfaceGeometry::Plane {
                origin: transform.point(local_origin),
                normal,
                u_axis,
            },
            source_object: Some(source_object(entry)),
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
        let (Some((first, first_interval)), Some((mut second, second_interval))) = (
            bounded_nurbs(ir, first_sequence, ctx, &composite_index),
            bounded_nurbs(ir, second_sequence, ctx, &composite_index),
        ) else {
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
            )
        {
            losses.push(entity_loss(
                entry,
                "equal-arc-length ruled projection has no exact normalized arc-length carrier",
            ));
            continue;
        }
        if direction_flag == 1 {
            second.control_points.reverse();
            let Some(knots) = reverse_knots(&second.knots) else {
                losses.push(entity_loss(entry, "second rail knot vector is empty"));
                continue;
            };
            second.knots = knots;
            if let Some(weights) = &mut second.weights {
                weights.reverse();
            }
        }
        let Some(surface) = ruled_surface_carrier(&first, &second, ctx) else {
            losses.push(entity_loss(
                entry,
                "ruled rails do not have a finite exact NURBS carrier",
            ));
            continue;
        };
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(surface),
            source_object: Some(source_object(entry)),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence)),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Ruled {
                first: CurveId(format!("iges:model:curve#D{first_sequence}")),
                second: CurveId(format!("iges:model:curve#D{second_sequence}")),
            },
            cache_fit_tolerance: None,
            record_bounds: Some([
                Some(first_interval[0]),
                Some(first_interval[1]),
                Some(second_interval[0]),
                Some(second_interval[1]),
            ]),
        });
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
            global.dialect(),
        ) {
            losses.push(entity_loss(
                entry,
                "directrix entity is outside the declared dialect",
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
        let directrix_id = CurveId(format!("iges:model:curve#D{directrix_sequence}"));
        let Some((directrix, cached_interval)) =
            bounded_nurbs(ir, directrix_sequence, ctx, &composite_index)
        else {
            let Some((directrix_geometry, carrier_interval)) = bounded_evaluable_curve(
                ir,
                directrix_sequence,
                global.minimum_resolution_mm(),
                &composite_index,
            ) else {
                losses.push(entity_loss(
                    entry,
                    "directrix has no bounded polynomial, NURBS, or exact evaluable carrier",
                ));
                continue;
            };
            let source_interval = source_parameter_interval(&directrix_geometry, carrier_interval);
            let Some(start) =
                cadmpeg_ir::eval::curve_point(&directrix_geometry, carrier_interval[0])
            else {
                losses.push(entity_loss(entry, "directrix start cannot be evaluated"));
                continue;
            };
            let start = transform.point(start);
            let target = transform.point(Point3::new(x * factor, y * factor, z * factor));
            let direction = target.vector_from(start);
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
                let placed_id = CurveId(format!(
                    "iges:model:curve#D{}-placed-directrix",
                    entry.sequence
                ));
                ir.model.curves.push(Curve {
                    id: placed_id.clone(),
                    geometry: CurveGeometry::Transformed {
                        basis: Box::new(directrix_geometry),
                        transform: transform.body_transform(),
                    },
                    source_object: Some(source_object(entry)),
                });
                placed_id
            };
            let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
            let procedural_id =
                ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence));
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                },
                source_object: Some(source_object(entry)),
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Extrusion {
                    directrix: procedural_directrix,
                    parameter_interval: Some(source_interval),
                    direction,
                    native_position: Some(target),
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: Some([
                    Some(carrier_interval[0]),
                    Some(carrier_interval[1]),
                    None,
                    None,
                ]),
            });
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
        if entry.transform != 0 {
            for point in &mut placed_directrix.control_points {
                *point = transform.point(*point);
            }
        }
        let Some(start) = cadmpeg_ir::eval::nurbs_curve_point(
            placed_directrix.degree,
            &placed_directrix.knots,
            &placed_directrix.control_points,
            placed_directrix.weights.as_deref(),
            cached_interval[0],
        ) else {
            losses.push(entity_loss(entry, "directrix start cannot be evaluated"));
            continue;
        };
        let target = transform.point(Point3::new(x * factor, y * factor, z * factor));
        let direction = target.vector_from(start);
        if !direction.norm().is_finite() || direction.norm() <= 0.0 {
            losses.push(entity_loss(
                entry,
                "tabulated direction is zero or non-finite",
            ));
            continue;
        }
        let control_points = placed_directrix
            .control_points
            .iter()
            .flat_map(|point| [*point, point.translated(direction, 1.0)])
            .collect::<Vec<_>>();
        let Ok(u_count) = u32::try_from(placed_directrix.control_points.len()) else {
            losses.push(entity_loss(entry, "directrix pole count exceeds u32"));
            continue;
        };
        let weights = placed_directrix.weights.as_ref().map(|weights| {
            weights
                .iter()
                .flat_map(|weight| [*weight, *weight])
                .collect()
        });
        let procedural_directrix = if entry.transform == 0 {
            directrix_id
        } else {
            let placed_id = CurveId(format!(
                "iges:model:curve#D{}-placed-directrix",
                entry.sequence
            ));
            ir.model.curves.push(Curve {
                id: placed_id.clone(),
                geometry: CurveGeometry::Nurbs(placed_directrix.clone()),
                source_object: Some(source_object(entry)),
            });
            placed_id
        };
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: placed_directrix.degree,
                v_degree: 1,
                u_knots: placed_directrix.knots,
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count,
                v_count: 2,
                control_points,
                weights,
                u_periodic: placed_directrix.periodic,
                v_periodic: false,
            }),
            source_object: Some(source_object(entry)),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence)),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Extrusion {
                directrix: procedural_directrix,
                parameter_interval: Some(source_interval),
                direction,
                native_position: Some(target),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: Some([
                Some(carrier_interval[0]),
                Some(carrier_interval[1]),
                None,
                None,
            ]),
        });
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
        let axis_id = CurveId(format!("iges:model:curve#D{axis_sequence}"));
        let Some(axis_curve) = ir.model.curves.iter().find(|curve| curve.id == axis_id) else {
            losses.push(entity_loss(entry, "revolution axis carrier is missing"));
            continue;
        };
        let CurveGeometry::Line {
            origin: axis_origin,
            direction: axis_direction,
        } = axis_curve.geometry
        else {
            losses.push(entity_loss(
                entry,
                "revolution axis is not a Line Entity carrier",
            ));
            continue;
        };
        let Some((generatrix, cached_interval)) =
            bounded_nurbs(ir, generatrix_sequence, ctx, &composite_index)
        else {
            let Some((directrix_geometry, carrier_interval)) = bounded_evaluable_curve(
                ir,
                generatrix_sequence,
                global.minimum_resolution_mm(),
                &composite_index,
            ) else {
                losses.push(entity_loss(
                    entry,
                    "generatrix has no bounded polynomial, NURBS, or exact evaluable carrier",
                ));
                continue;
            };
            let source_interval = source_parameter_interval(&directrix_geometry, carrier_interval);
            let mut procedural_directrix =
                CurveId(format!("iges:model:curve#D{generatrix_sequence}"));
            let mut procedural_axis_origin = axis_origin;
            let mut procedural_axis_direction = axis_direction;
            if entry.transform != 0 {
                let Some(orientation) = similarity_orientation(transform) else {
                    losses.push(entity_loss(
                        entry,
                        "placement cannot preserve the exact revolution parameterization",
                    ));
                    continue;
                };
                procedural_directrix = CurveId(format!(
                    "iges:model:curve#D{}-placed-generatrix",
                    entry.sequence
                ));
                ir.model.curves.push(Curve {
                    id: procedural_directrix.clone(),
                    geometry: CurveGeometry::Transformed {
                        basis: Box::new(directrix_geometry),
                        transform: transform.body_transform(),
                    },
                    source_object: Some(source_object(entry)),
                });
                procedural_axis_origin = transform.point(axis_origin);
                let Some(direction) = unit_vector(transform.vector(axis_direction)) else {
                    losses.push(entity_loss(
                        entry,
                        "placement collapses the revolution axis",
                    ));
                    continue;
                };
                procedural_axis_direction = direction.scale(orientation);
            }
            let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
            let procedural_id =
                ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence));
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                },
                source_object: Some(source_object(entry)),
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: procedural_directrix,
                    axis_origin: procedural_axis_origin,
                    axis_direction: procedural_axis_direction,
                    angular_interval: [start_angle, end_angle],
                    angular_parameter_interval: None,
                    parameter_interval: Some(source_interval),
                    transposed: false,
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: Some([
                    Some(carrier_interval[0]),
                    Some(carrier_interval[1]),
                    None,
                    None,
                ]),
            });
            decoded.insert(entry.sequence);
            continue;
        };
        let generatrix_id = CurveId(format!("iges:model:curve#D{generatrix_sequence}"));
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
        let Ok(u_count) = u32::try_from(generatrix.control_points.len()) else {
            losses.push(entity_loss(entry, "generatrix pole count exceeds u32"));
            continue;
        };
        let Ok(v_count) = u32::try_from(angular_controls.len()) else {
            losses.push(entity_loss(entry, "angular pole count exceeds u32"));
            continue;
        };
        let Some(surface_pole_count) = generatrix
            .control_points
            .len()
            .checked_mul(angular_controls.len())
        else {
            return Err(refuse_local_limit(
                "iges_revolution_poles",
                MAX_SURFACE_POLES as u64,
                u64::MAX,
                None,
            ));
        };
        if surface_pole_count > MAX_SURFACE_POLES {
            return Err(refuse_local_limit(
                "iges_revolution_poles",
                MAX_SURFACE_POLES as u64,
                surface_pole_count as u64,
                None,
            ));
        }
        let mut control_points = Vec::with_capacity(surface_pole_count);
        let mut weights = Vec::with_capacity(control_points.capacity());
        for (u_index, point) in generatrix.control_points.iter().enumerate() {
            let delta = point.vector_from(axis_origin);
            let axis_point = axis_origin.translated(axis_direction, delta.dot(axis_direction));
            let radial = point.vector_from(axis_point);
            let u_weight = generatrix
                .weights
                .as_ref()
                .and_then(|values| values.get(u_index))
                .copied()
                .unwrap_or(1.0);
            for (angle, angular_weight) in &angular_controls {
                let rotated = rotate(radial, axis_direction, *angle);
                let radial_control = rotated.scale(1.0 / angular_weight);
                control_points.push(transform.point(axis_point.translated(radial_control, 1.0)));
                weights.push(u_weight * angular_weight);
            }
        }
        let placed_generatrix = (entry.transform != 0).then(|| generatrix.clone());
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: generatrix.degree,
                v_degree: 2,
                u_knots: generatrix.knots,
                v_knots,
                u_count,
                v_count,
                control_points,
                weights: Some(weights),
                u_periodic: generatrix.periodic,
                v_periodic: super::curve_conversion::angularly_equal(
                    end_angle - start_angle,
                    std::f64::consts::TAU,
                ),
            }),
            source_object: Some(source_object(entry)),
        });
        let mut procedural_directrix = CurveId(format!("iges:model:curve#D{generatrix_sequence}"));
        let mut procedural_axis_origin = axis_origin;
        let mut procedural_axis_direction = axis_direction;
        let procedural_is_exact = if entry.transform == 0 {
            true
        } else if let Some(orientation) = similarity_orientation(transform) {
            let mut placed_generatrix = placed_generatrix
                .expect("a transformed revolution retains its generatrix until placement");
            for point in &mut placed_generatrix.control_points {
                *point = transform.point(*point);
            }
            procedural_directrix = CurveId(format!(
                "iges:model:curve#D{}-placed-generatrix",
                entry.sequence
            ));
            ir.model.curves.push(Curve {
                id: procedural_directrix.clone(),
                geometry: CurveGeometry::Nurbs(placed_generatrix),
                source_object: Some(source_object(entry)),
            });
            procedural_axis_origin = transform.point(axis_origin);
            let Some(direction) = unit_vector(transform.vector(axis_direction)) else {
                losses.push(entity_loss(
                    entry,
                    "placement collapses the revolution axis",
                ));
                continue;
            };
            procedural_axis_direction = direction.scale(orientation);
            true
        } else {
            false
        };
        if procedural_is_exact {
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!(
                    "iges:model:procedural-surface#D{}",
                    entry.sequence
                )),
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: procedural_directrix,
                    axis_origin: procedural_axis_origin,
                    axis_direction: procedural_axis_direction,
                    angular_interval: [start_angle, end_angle],
                    angular_parameter_interval: None,
                    parameter_interval: Some(source_interval),
                    transposed: false,
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: Some([
                    Some(carrier_interval[0]),
                    Some(carrier_interval[1]),
                    None,
                    None,
                ]),
            });
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
                    None,
                ));
            }
            Some(requested) if requested > MAX_SURFACE_POLES as u64 => {
                return Err(refuse_local_limit(
                    "iges_surface_poles",
                    MAX_SURFACE_POLES as u64,
                    requested,
                    None,
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
        let (Ok(u_count_u32), Ok(v_count_u32)) = (u32::try_from(u_count), u32::try_from(v_count))
        else {
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
                None,
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
                control_points.push(transform.point(native_points[native_index]));
                if let Some(weights) = &mut weights {
                    weights.push(native_weights[native_index]);
                }
            }
        }
        let surface = NurbsSurface {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            u_count: u_count_u32,
            v_count: v_count_u32,
            control_points,
            weights,
            u_periodic: flags[3] == Some(1),
            v_periodic: flags[4] == Some(1),
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
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(surface),
            source_object: Some(source_object(entry)),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence)),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Exact {
                parameters: SplineSurfaceParameters::OrderedRanges {
                    ranges: [u_range, v_range],
                },
                extension: 0,
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: Some([
                Some(u_range[0]),
                Some(u_range[1]),
                Some(v_range[0]),
                Some(v_range[1]),
            ]),
        });
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
        if !declared_unit_vector(record, 1, indicator, global.real_precision()) {
            losses.push(entity_loss(entry, "offset indicator is not a unit vector"));
            continue;
        }
        let indicator = unit_vector(indicator).expect("validated nonzero finite offset indicator");
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
        let support_id = SurfaceId(format!("iges:model:surface#D{support_sequence}"));
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
            SurfaceGeometry::Cylinder { radius, .. } | SurfaceGeometry::Sphere { radius, .. } => {
                *radius > 0.0
            }
            SurfaceGeometry::Torus {
                major_radius,
                minor_radius,
                ..
            } => *major_radius > 0.0 && *minor_radius > 0.0,
            SurfaceGeometry::Cone { radius, .. } => *radius > 0.0,
            SurfaceGeometry::Plane { .. } => true,
            SurfaceGeometry::Nurbs(_)
            | SurfaceGeometry::Procedural { .. }
            | SurfaceGeometry::Polygonal { .. }
            | SurfaceGeometry::Transformed { .. }
            | SurfaceGeometry::Unknown { .. } => false,
        };
        if !regular {
            losses.push(entity_loss(
                entry,
                "offset collapses or reverses the analytic carrier",
            ));
            continue;
        }
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry,
            source_object: Some(source_object(entry)),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence)),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Offset {
                support: support_id,
                distance: signed_distance,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        decoded.insert(entry.sequence);
    }

    Ok(ProjectionOutcome { decoded, losses })
}

#[cfg(test)]
mod tests;
