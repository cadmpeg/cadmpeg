// SPDX-License-Identifier: Apache-2.0
//! Reconstruct a writer cache from authoritative typed construction geometry.

use super::*;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::ProceduralSurface;

mod revolution;

fn profile(
    target: &CadIr,
    id: &cadmpeg_ir::ids::CurveId,
) -> Result<Option<NurbsCurve>, CodecError> {
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *id)
        .ok_or_else(|| CodecError::malformed(format!("cache profile {id} is missing")))?;
    match &curve.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => Ok(Some(nurbs.clone())),
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) => curve
            .parameter_range
            .map(|range| {
                native_increasing_interval_curve(&SolvedCurveGeometry::Line(line.clone()), range)
                    .map(Some)
            })
            .unwrap_or(Ok(None)),
        // Rational conic conversion preserves a locus, not its angle parameter.
        _ => Ok(None),
    }
}

fn reserve_cache(
    ctx: &DecodeContext<'_>,
    u: usize,
    v: usize,
    knots: usize,
    rational: bool,
) -> Result<(), CodecError> {
    let lanes = if rational { 4usize } else { 3usize };
    let bytes = u
        .checked_mul(v)
        .and_then(|count| count.checked_mul(lanes))
        .and_then(|count| count.checked_add(knots))
        .and_then(|count| count.checked_mul(std::mem::size_of::<f64>()))
        .ok_or_else(|| CodecError::malformed("canonical cache native payload capacity overflow"))?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(bytes),
        "generate F3D canonical cache output",
    )
}

fn chart_matches(construction: &ProceduralSurface, cache: &NurbsSurface) -> bool {
    let Some(bounds) = construction.record_bounds() else {
        return true;
    };
    let [Some(u0), Some(v0), Some(u1), Some(v1)] = bounds.get() else {
        return false;
    };
    let Ok(u_degree) = usize::try_from(cache.u_degree()) else {
        return false;
    };
    let Ok(v_degree) = usize::try_from(cache.v_degree()) else {
        return false;
    };
    let u = [cache.u_knots()[u_degree], cache.u_knots()[cache.u_count()]];
    let v = [cache.v_knots()[v_degree], cache.v_knots()[cache.v_count()]];
    if [u0, u1] != u || [v0, v1] != v {
        return false;
    }
    match construction.definition() {
        ProceduralSurfaceDefinition::Extrusion(payload) => payload
            .parameter_interval()
            .is_none_or(|range| range.get() == u),
        ProceduralSurfaceDefinition::Revolution(payload) => payload
            .parameter_interval()
            .is_none_or(|range| range.endpoints() == u),
        _ => true,
    }
}

fn tensor(
    ctx: &DecodeContext<'_>,
    first: &NurbsCurve,
    second: &NurbsCurve,
    base: Vector3,
) -> Result<NurbsSurface, CodecError> {
    let first_weights = first.weights();
    let second_weights = second.weights();
    let rational = first_weights.is_some() || second_weights.is_some();
    reserve_cache(
        ctx,
        first.pole_count(),
        second.pole_count(),
        first
            .knots()
            .len()
            .checked_add(second.knots().len())
            .ok_or_else(|| CodecError::malformed("cache knot count overflow"))?,
        rational,
    )?;
    let first_points = first.control_points();
    let second_points = second.control_points();
    let mut points = ctx.alloc_filled(
        first.pole_count(),
        Vec::new(),
        "allocate F3D tensor cache rows",
    )?;
    for (row, first) in points.iter_mut().zip(&first_points) {
        *row = ctx.alloc_filled(
            second.pole_count(),
            Point3::new(0.0, 0.0, 0.0),
            "allocate F3D tensor cache poles",
        )?;
        let first = first.get();
        for (point, second) in row.iter_mut().zip(&second_points) {
            let second = second.get();
            *point = Point3::new(
                first.x + second.x - base.x,
                first.y + second.y - base.y,
                first.z + second.z - base.z,
            );
        }
    }
    let weights = if rational {
        let mut rows = ctx.alloc_filled(
            first.pole_count(),
            Vec::new(),
            "allocate F3D tensor cache weight rows",
        )?;
        for (u, row) in rows.iter_mut().enumerate() {
            *row = ctx.alloc_filled(
                second.pole_count(),
                1.0,
                "allocate F3D tensor cache weights",
            )?;
            for (v, weight) in row.iter_mut().enumerate() {
                *weight = first_weights
                    .as_ref()
                    .map_or(1.0, |weights| weights[u].get())
                    * second_weights
                        .as_ref()
                        .map_or(1.0, |weights| weights[v].get());
            }
        }
        Some(rows)
    } else {
        None
    };
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(first.degree(), first.knots().to_vec(), first.periodic()),
        NurbsSurfaceAxis::new(second.degree(), second.knots().to_vec(), second.periodic()),
        NurbsSurfaceLanes::new(points, weights),
        false,
    )
    .map_err(CodecError::malformed)
}

pub(super) fn generate(
    ctx: &DecodeContext<'_>,
    target: &CadIr,
    construction: &ProceduralSurface,
) -> Result<Option<NurbsSurface>, CodecError> {
    // Preserve parameterized and genuinely cacheless native forms unchanged.
    if construction.cache_fit_tolerance().is_none()
        && !matches!(
            construction.definition(),
            ProceduralSurfaceDefinition::Revolution(_)
        )
    {
        return Ok(None);
    }
    let generated = match construction.definition() {
        ProceduralSurfaceDefinition::Extrusion(payload) => {
            if payload
                .revision_form()
                .is_some_and(|form| form.flags.iter().any(|flag| *flag))
            {
                return Ok(None);
            }
            let Some(first) = profile(target, payload.directrix())? else {
                return Ok(None);
            };
            if let Some(curve) = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *payload.directrix())
            {
                if matches!(
                    curve.geometry,
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
                ) && payload.parameter_interval().is_some_and(|range| {
                    curve
                        .parameter_range
                        .is_none_or(|physical| physical.endpoints() != range.get())
                }) {
                    return Ok(None);
                }
            }
            let direction = payload.direction().get();
            let second = NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(direction.x, direction.y, direction.z),
                ],
                None,
                false,
            )
            .map_err(CodecError::malformed)?;
            tensor(ctx, &first, &second, Vector3::new(0.0, 0.0, 0.0))?
        }
        ProceduralSurfaceDefinition::Sum(payload) => {
            let (Some(first), Some(second)) = (
                profile(target, payload.first())?,
                profile(target, payload.second())?,
            ) else {
                return Ok(None);
            };
            tensor(ctx, &first, &second, payload.basepoint().get())?
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            let (Some(first), Some(second)) = (profile(target, first)?, profile(target, second)?)
            else {
                return Ok(None);
            };
            if first.degree() != second.degree()
                || first.knots() != second.knots()
                || first.weights() != second.weights()
                || first.periodic() != second.periodic()
            {
                return Ok(None);
            }
            reserve_cache(
                ctx,
                first.pole_count(),
                2,
                first
                    .knots()
                    .len()
                    .checked_add(4)
                    .ok_or_else(|| CodecError::malformed("ruled knot count overflow"))?,
                first.weights().is_some(),
            )?;
            let points = first
                .control_points()
                .into_iter()
                .zip(second.control_points())
                .map(|(first, second)| vec![first.get(), second.get()])
                .collect();
            let weights = first.weights().map(|weights| {
                weights
                    .into_iter()
                    .map(|weight| vec![weight.get(), weight.get()])
                    .collect()
            });
            NurbsSurface::from_lanes(
                NurbsSurfaceAxis::new(first.degree(), first.knots().to_vec(), first.periodic()),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceLanes::new(points, weights),
                false,
            )
            .map_err(CodecError::malformed)?
        }
        ProceduralSurfaceDefinition::Revolution(payload) => {
            return revolution::generate(ctx, target, construction, payload)
        }
        _ => return Ok(None),
    };
    Ok(chart_matches(construction, &generated).then_some(generated))
}

#[cfg(test)]
mod tests;
