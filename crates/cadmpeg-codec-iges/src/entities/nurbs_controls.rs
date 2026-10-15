// SPDX-License-Identifier: Apache-2.0
//! Exact edits of homogeneous NURBS control nets and parameter domains.
use cadmpeg_core::{
    decode::{index_from_u32, DecodeContext},
    CodecError,
};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::{NonZeroReal, PositiveReal};
pub(crate) fn homogeneous_point_is_valid(point: &[f64; 4]) -> bool {
    point.iter().all(|value| value.is_finite()) && point[0] > 0.0
}

pub(crate) fn homogeneous_control_points(curve: &NurbsCurve) -> Option<Vec<[f64; 4]>> {
    let mut homogeneous = Vec::new();
    for index in 0..curve.pole_count() {
        let point = curve.pole_rows().point_at(index)?;
        let weight = if matches!(curve.pole_rows(), NurbsPoles3::Rational { .. }) {
            let weight = curve.pole_rows().weight_at(index)?;
            weight
        } else {
            1.0
        };
        let homogeneous_point = [weight, weight * point.x, weight * point.y, weight * point.z];
        if !homogeneous_point_is_valid(&homogeneous_point) {
            return None;
        }
        homogeneous.push(homogeneous_point);
    }
    Some(homogeneous)
}

pub(crate) struct EuclideanControlNet {
    pub(crate) control_points: Vec<FinitePoint3>,
    pub(crate) weights: Option<Vec<PositiveReal>>,
}

pub(crate) fn euclidean_control_points(
    homogeneous: Vec<[f64; 4]>,
    rational: bool,
) -> Option<EuclideanControlNet> {
    let mut control_points = Vec::new();
    let mut weights = rational.then(Vec::new);
    for [weight, x, y, z] in homogeneous {
        let weight = PositiveReal::new(weight)?;
        let point = Point3::new(x / weight.get(), y / weight.get(), z / weight.get());
        let point = FinitePoint3::new(point)?;
        control_points.push(point);
        if let Some(weights) = &mut weights {
            weights.push(weight);
        }
    }
    Some(EuclideanControlNet {
        control_points,
        weights,
    })
}

#[derive(Debug)]
pub(crate) struct InsertedKnotNet {
    pub(crate) control_points: Vec<[f64; 4]>,
    pub(crate) knots: Vec<f64>,
}

pub(crate) fn insert_homogeneous_knot(
    ctx: &DecodeContext<'_>,
    control_points: &[[f64; 4]],
    knots: &[f64],
    degree: usize,
    value: f64,
) -> Result<Option<InsertedKnotNet>, CodecError> {
    let control_count = control_points.len();
    let Some(last_control) = control_count.checked_sub(1) else {
        return Ok(None);
    };
    let Some(span) = knots.iter().rposition(|knot| *knot <= value) else {
        return Ok(None);
    };
    let span = if degree == 0 {
        span.min(last_control)
    } else {
        span
    };
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    let Some(left_end) = span.checked_sub(degree) else {
        return Ok(None);
    };
    let Some(expected_knots) = control_count
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if control_count <= degree
        || left_end > last_control
        || multiplicity > degree
        || span < degree
        || knots.len() != expected_knots
    {
        return Ok(None);
    }
    let Some(knot_count) = knots.len().checked_add(1) else {
        return Ok(None);
    };
    let mut inserted_knots = ctx.alloc_filled(knot_count, 0.0, "iges composite inserted knots")?;
    inserted_knots.clear();
    let Some(left_knots) = knots.get(..=span) else {
        return Ok(None);
    };
    inserted_knots.extend_from_slice(left_knots);
    inserted_knots.push(value);
    let Some(next_knot) = span.checked_add(1) else {
        return Ok(None);
    };
    let Some(right_knots) = knots.get(next_knot..) else {
        return Ok(None);
    };
    inserted_knots.extend_from_slice(right_knots);

    let Some(inserted_count) = control_count.checked_add(1) else {
        return Ok(None);
    };
    let mut inserted_control_points = ctx.alloc_filled(
        inserted_count,
        [0.0; 4],
        "iges composite knot-insertion control points",
    )?;
    let Some(tail_start) = span.checked_sub(multiplicity) else {
        return Ok(None);
    };
    if tail_start > last_control {
        return Ok(None);
    }
    inserted_control_points[..=left_end].copy_from_slice(&control_points[..=left_end]);
    inserted_control_points[tail_start + 1..]
        .copy_from_slice(&control_points[tail_start..control_count]);
    let Some(first_interior) = left_end.checked_add(1) else {
        return Ok(None);
    };
    for index in first_interior..=tail_start {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return Ok(None);
        }
        let alpha = (value - knots[index]) / denominator;
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Ok(None);
        }
        let previous = control_points[index - 1];
        let current = control_points[index];
        let point = [
            alpha * current[0] + (1.0 - alpha) * previous[0],
            alpha * current[1] + (1.0 - alpha) * previous[1],
            alpha * current[2] + (1.0 - alpha) * previous[2],
            alpha * current[3] + (1.0 - alpha) * previous[3],
        ];
        if !homogeneous_point_is_valid(&point) {
            return Ok(None);
        }
        inserted_control_points[index] = point;
    }
    Ok(Some(InsertedKnotNet {
        control_points: inserted_control_points,
        knots: inserted_knots,
    }))
}

pub(crate) type TrimmedLanes = (Vec<FinitePoint3>, Option<Vec<PositiveReal>>, Vec<f64>);

pub(crate) fn trim_nurbs_lanes(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    interval: [f64; 2],
) -> Result<Option<TrimmedLanes>, CodecError> {
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    let control_count = curve.pole_count();
    if curve.periodic() {
        return Ok(None);
    }
    let [start, end] = interval;
    if !start.is_finite() || !end.is_finite() || start >= end {
        return Ok(None);
    }
    let (Some(&domain_start), Some(&domain_end)) =
        (curve.knots().get(degree), curve.knots().get(control_count))
    else {
        return Ok(None);
    };
    if !domain_start.is_finite()
        || !domain_end.is_finite()
        || domain_start >= domain_end
        || start < domain_start
        || end > domain_end
    {
        return Ok(None);
    }
    let Some(mut homogeneous) = homogeneous_control_points(curve) else {
        return Ok(None);
    };
    let mut knots = curve.knots().to_vec();
    for value in [start, end] {
        let Some(target_multiplicity) = degree.checked_add(1) else {
            return Ok(None);
        };
        while knots.iter().filter(|knot| **knot == value).count() < target_multiplicity {
            let Some(InsertedKnotNet {
                control_points: new_homogeneous,
                knots: new_knots,
            }) = insert_homogeneous_knot(ctx, &homogeneous, &knots, degree, value)?
            else {
                return Ok(None);
            };
            homogeneous = new_homogeneous;
            knots = new_knots;
        }
    }
    let (Some(start_knot), Some(end_knot)) = (
        knots.iter().position(|knot| *knot == start),
        knots.iter().rposition(|knot| *knot == end),
    ) else {
        return Ok(None);
    };
    let Some(control_end) = end_knot.checked_sub(degree) else {
        return Ok(None);
    };
    if start_knot >= control_end {
        return Ok(None);
    }
    let (Some(homogeneous_slice), Some(knot_slice)) = (
        homogeneous.get(start_knot..control_end),
        knots.get(start_knot..=end_knot),
    ) else {
        return Ok(None);
    };
    let trimmed_homogeneous = homogeneous_slice.to_vec();
    let trimmed_knots = knot_slice.to_vec();
    let Some(expected_knots) = trimmed_homogeneous
        .len()
        .checked_add(degree)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    if trimmed_knots.len() != expected_knots {
        return Ok(None);
    }
    let Some(EuclideanControlNet {
        control_points,
        weights,
    }) = euclidean_control_points(
        trimmed_homogeneous,
        matches!(curve.pole_rows(), NurbsPoles3::Rational { .. }),
    )
    else {
        return Ok(None);
    };
    Ok(Some((control_points, weights, trimmed_knots)))
}

/// Restrict the tensor-product basis to its active rectangle without changing
/// its parameter chart or approximating any pole or rational weight.
pub(crate) fn trim_surface(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    ranges: [[f64; 2]; 2],
) -> Result<Option<NurbsSurface>, CodecError> {
    let u_domain = [
        surface.u_knots()[index_from_u32(surface.u_degree())],
        surface.u_knots()[surface.u_count()],
    ];
    let v_domain = [
        surface.v_knots()[index_from_u32(surface.v_degree())],
        surface.v_knots()[surface.v_count()],
    ];
    let mut result = if ranges[0] == u_domain {
        surface.clone()
    } else {
        let Some(trimmed) = trim_surface_u(ctx, surface, ranges[0])? else {
            return Ok(None);
        };
        trimmed
    };
    if ranges[1] != v_domain {
        result.transpose_parameter_axes();
        let Some(trimmed) = trim_surface_u(ctx, &result, ranges[1])? else {
            return Ok(None);
        };
        result = trimmed;
        result.transpose_parameter_axes();
    }
    Ok(Some(result))
}

fn trim_surface_u(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    interval: [f64; 2],
) -> Result<Option<NurbsSurface>, CodecError> {
    let degree = index_from_u32(surface.u_degree());
    let end = interval[1];
    // At a fully repeated interior knot the source selects the right patch.
    // A clamped restriction selects the left patch at its terminal endpoint.
    // Distinct endpoint poles therefore cannot represent the same restriction.
    if end < surface.u_knots()[surface.u_count()]
        && surface
            .u_knots()
            .iter()
            .filter(|knot| **knot == end)
            .count()
            > degree
    {
        let Some(right) = surface
            .u_knots()
            .iter()
            .rposition(|knot| *knot <= end)
            .and_then(|span| span.checked_sub(degree))
        else {
            return Ok(None);
        };
        let Some(left) = right.checked_sub(1) else {
            return Ok(None);
        };
        if (0..surface.v_count()).any(|v| surface.pole(left, v) != surface.pole(right, v)) {
            return Ok(None);
        }
    }
    let mut columns = Vec::new();
    for v in 0..surface.v_count() {
        let points = (0..surface.u_count())
            .map(|u| surface.pole(u, v))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                CodecError::malformed("admitted NURBS surface has an incomplete pole grid")
            })?;
        let weights = if surface.weight(0, v).is_some() {
            Some(
                (0..surface.u_count())
                    .map(|u| surface.weight(u, v))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "admitted NURBS surface has an incomplete weight grid",
                        )
                    })?,
            )
        } else {
            None
        };
        let knots = surface.u_knots().to_vec();
        let curve = NurbsCurve::from_checked_lanes(
            surface.u_degree(),
            knots,
            points,
            weights,
            surface.u_periodic(),
        )
        .map_err(CodecError::malformed)?;
        let Some(column) = trim_nurbs_lanes(ctx, &curve, interval)? else {
            return Ok(None);
        };
        columns.push(column);
    }
    let Some(first) = columns.first() else {
        return Ok(None);
    };
    let count = first.0.len();
    if columns.iter().any(|column| {
        column.0.len() != count
            || column.2 != first.2
            || column.1.is_some() != first.1.is_some()
            || column
                .1
                .as_ref()
                .is_some_and(|weights| weights.len() != count)
    }) {
        return Ok(None);
    }
    let mut points = Vec::new();
    let mut weights = if first.1.is_some() {
        Some(Vec::new())
    } else {
        None
    };
    for u in 0..count {
        let mut row = Vec::new();
        let mut weight_row = if weights.is_some() {
            Some(Vec::new())
        } else {
            None
        };
        for column in &columns {
            row.push(column.0[u]);
            if let (Some(row), Some(column)) = (&mut weight_row, &column.1) {
                row.push(NonZeroReal::from(column[u]));
            }
        }
        points.push(row);
        if let (Some(rows), Some(row)) = (&mut weights, weight_row) {
            rows.push(row);
        }
    }
    let u_knots = first.2.clone();
    let v_knots = surface.v_knots().to_vec();
    NurbsSurface::from_checked_lanes(
        NurbsSurfaceAxis::new(surface.u_degree(), u_knots, false),
        NurbsSurfaceAxis::new(surface.v_degree(), v_knots, surface.v_periodic()),
        NurbsSurfaceLanes::new(points, weights),
        surface.normal_reversed(),
    )
    .map(Some)
    .map_err(CodecError::malformed)
}

#[cfg(test)]
mod tests;
