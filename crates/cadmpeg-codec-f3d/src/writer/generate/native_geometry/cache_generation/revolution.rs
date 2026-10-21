// SPDX-License-Identifier: Apache-2.0
//! A canonical angular chord cache with a bound from the actual fit contract.

use super::{
    chart_matches, f64_from_index, profile, truncate_f64_to_usize, CadIr, CodecError,
    CurveGeometry, DecodeContext, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes, Point3,
    ProceduralSurface, SolvedCurveGeometry,
};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction;

pub(super) fn generate(
    ctx: &DecodeContext<'_>,
    target: &CadIr,
    construction: &ProceduralSurface,
    payload: &RevolutionSurfaceConstruction,
) -> Result<Option<NurbsSurface>, CodecError> {
    if *payload.transposed()
        || payload
            .angular_parameter_interval()
            .is_some_and(|range| range != payload.angular_interval())
    {
        return Ok(None);
    }
    let Some(curve) = profile(target, payload.directrix())? else {
        return Ok(None);
    };
    if target.model.curves.iter().any(|curve| {
        curve.id == *payload.directrix()
            && matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
            )
            && payload
                .parameter_interval()
                .is_some_and(|range| curve.parameter_range != Some(range))
    }) {
        return Ok(None);
    }
    let weights = curve.weights();
    if weights.as_ref().is_some_and(|weights| {
        !(weights.iter().all(|weight| weight.get() > 0.0)
            || weights.iter().all(|weight| weight.get() < 0.0))
    }) {
        return Ok(None);
    }
    let origin = payload.axis_origin().get();
    let axis = payload
        .axis_direction()
        .as_raw()
        .unit()
        .ok_or_else(|| CodecError::malformed("revolution axis is not finite"))?;
    let points = curve.control_points();
    let radius = points
        .iter()
        .map(|point| point.get().vector_from(origin).cross(axis).norm())
        .fold(0.0_f64, f64::max);
    let tolerance = construction.cache_fit_tolerance().map_or(
        target.tolerances.linear.get(),
        cadmpeg_ir::geometry::FitTolerance::get,
    );
    if !radius.is_finite() || (radius > 0.0 && tolerance <= 0.0) {
        return Ok(None);
    }
    let scale = points
        .iter()
        .map(|point| point.get())
        .chain(std::iter::once(origin))
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .fold(radius, f64::max);
    // Reject a requested fit below the precision budget of the finite affine
    // rotation and rational-pole accumulation; never enlarge that fit.
    let operations = f64_from_index(points.len())
        .ok_or_else(|| CodecError::malformed("revolution profile count is not exact in f64"))?
        * 128.0;
    if scale * f64::EPSILON * operations > tolerance / 2.0 {
        return Err(CodecError::UnsupportedContent(
            "F3D revolution fit is below the coordinate precision budget".into(),
        ));
    }
    let [start, end] = payload.angular_interval().endpoints();
    // Same-sign rational poles bound the radius of every directrix point.
    // Angular chord interpolation error <= R*delta_angle^2/8 throughout each
    // interval, not only at samples. Reserve half the actual fit for rounding.
    let count = if radius == 0.0 {
        1
    } else {
        truncate_f64_to_usize(((end - start) / (2.0 * (tolerance / radius).sqrt())).ceil())
            .ok_or_else(|| CodecError::malformed("revolution cache subdivision count overflow"))?
            .max(1)
    };
    let width = count
        .checked_add(1)
        .ok_or_else(|| CodecError::malformed("revolution cache row count overflow"))?;
    let point_count = curve
        .pole_count()
        .checked_mul(width)
        .ok_or_else(|| CodecError::malformed("revolution cache pole count overflow"))?;
    let knot_count = curve
        .knots()
        .len()
        .checked_add(width)
        .and_then(|count| count.checked_add(2))
        .ok_or_else(|| CodecError::malformed("revolution cache knot count overflow"))?;
    let coordinate_lanes = if weights.is_some() { 4usize } else { 3usize };
    let payload_bytes = point_count
        .checked_mul(coordinate_lanes)
        .and_then(|count| count.checked_add(knot_count))
        .and_then(|count| count.checked_mul(std::mem::size_of::<f64>()))
        .ok_or_else(|| {
            CodecError::malformed("revolution cache native payload capacity overflow")
        })?;
    // These scalars become retained native SMBH output. Charge the output
    // before allocating the temporary cache; one context spans the whole SMBH.
    ctx.retain_bytes(
        u64_from_index(payload_bytes),
        "generate F3D revolution cache output",
    )?;
    let denominator = f64_from_index(count)
        .ok_or_else(|| CodecError::malformed("revolution cache count is not exact in f64"))?;
    let mut angles = ctx.alloc_filled(width, 0.0, "allocate F3D revolution cache angles")?;
    for (index, angle) in angles.iter_mut().enumerate() {
        let numerator = f64_from_index(index)
            .ok_or_else(|| CodecError::malformed("revolution cache index is not exact in f64"))?;
        *angle = if index == count {
            end
        } else {
            start + (end - start) * numerator / denominator
        };
    }
    let mut grid = ctx.alloc_filled(
        points.len(),
        Vec::new(),
        "allocate F3D revolution cache rows",
    )?;
    for (row, point) in grid.iter_mut().zip(points) {
        *row = ctx.alloc_filled(
            width,
            Point3::new(0.0, 0.0, 0.0),
            "allocate F3D revolution cache poles",
        )?;
        let delta = point.get().vector_from(origin);
        let parallel = axis.dot(delta);
        let cross = axis.cross(delta);
        for (pole, angle) in row.iter_mut().zip(&angles) {
            let (sin, cos) = angle.sin_cos();
            *pole = Point3::new(
                origin.x + delta.x * cos + cross.x * sin + axis.x * parallel * (1.0 - cos),
                origin.y + delta.y * cos + cross.y * sin + axis.y * parallel * (1.0 - cos),
                origin.z + delta.z * cos + cross.z * sin + axis.z * parallel * (1.0 - cos),
            );
        }
    }
    let weights = weights
        .map(|weights| {
            weights
                .into_iter()
                .map(|weight| {
                    ctx.alloc_filled(width, weight.get(), "allocate F3D revolution cache weights")
                })
                .collect::<Result<Vec<_>, CodecError>>()
        })
        .transpose()?;
    let mut knots = ctx.alloc_filled(
        width
            .checked_add(2)
            .ok_or_else(|| CodecError::malformed("revolution knot allocation overflow"))?,
        start,
        "allocate F3D revolution cache knots",
    )?;
    knots[1..=width].copy_from_slice(&angles);
    knots[width + 1] = end;
    let generated = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(curve.degree(), curve.knots().to_vec(), curve.periodic()),
        NurbsSurfaceAxis::new(1, knots, false),
        NurbsSurfaceLanes::new(grid, weights),
        false,
    )
    .map_err(CodecError::malformed)?;
    Ok(chart_matches(construction, &generated).then_some(generated))
}
