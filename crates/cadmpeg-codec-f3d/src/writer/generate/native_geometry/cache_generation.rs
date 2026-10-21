// SPDX-License-Identifier: Apache-2.0
//! Reconstruct a writer cache from authoritative typed construction geometry.

use super::*;
use cadmpeg_ir::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::ProceduralSurface;

fn profile(target: &CadIr, id: &cadmpeg_ir::ids::CurveId) -> Result<NurbsCurve, CodecError> {
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *id)
        .ok_or_else(|| CodecError::malformed(format!("cache profile {id} is missing")))?;
    native_loft_curve(
        target,
        id,
        curve
            .parameter_range
            .map(IncreasingParameterInterval::endpoints),
    )
}

fn tensor(
    first: &NurbsCurve,
    second: &NurbsCurve,
    base: Vector3,
) -> Result<NurbsSurface, CodecError> {
    first
        .pole_count()
        .checked_mul(second.pole_count())
        .ok_or_else(|| CodecError::malformed("cache tensor pole count overflow"))?;
    let first_points = first.control_points();
    let second_points = second.control_points();
    let points = first_points
        .iter()
        .map(|first| {
            second_points
                .iter()
                .map(|second| {
                    let first = first.get();
                    let second = second.get();
                    Point3::new(
                        first.x + second.x - base.x,
                        first.y + second.y - base.y,
                        first.z + second.z - base.z,
                    )
                })
                .collect()
        })
        .collect();
    let first_weights = first.weights();
    let second_weights = second.weights();
    let weights = (first_weights.is_some() || second_weights.is_some()).then(|| {
        (0..first.pole_count())
            .map(|u| {
                (0..second.pole_count())
                    .map(|v| {
                        first_weights
                            .as_ref()
                            .map_or(1.0, |weights| weights[u].get())
                            * second_weights
                                .as_ref()
                                .map_or(1.0, |weights| weights[v].get())
                    })
                    .collect()
            })
            .collect()
    });
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(first.degree(), first.knots().to_vec(), first.periodic()),
        NurbsSurfaceAxis::new(second.degree(), second.knots().to_vec(), second.periodic()),
        NurbsSurfaceLanes::new(points, weights),
        false,
    )
    .map_err(CodecError::malformed)
}

pub(super) fn generate(
    target: &CadIr,
    construction: &ProceduralSurface,
) -> Result<Option<NurbsSurface>, CodecError> {
    let generated = match construction.definition() {
        ProceduralSurfaceDefinition::Extrusion(payload) => {
            if payload
                .revision_form()
                .is_some_and(|form| form.flags.iter().any(|flag| *flag))
            {
                return Ok(None);
            }
            let first = profile(target, payload.directrix())?;
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
            tensor(&first, &second, Vector3::new(0.0, 0.0, 0.0))?
        }
        ProceduralSurfaceDefinition::Sum(payload) => tensor(
            &profile(target, payload.first())?,
            &profile(target, payload.second())?,
            *payload.basepoint(),
        )?,
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            let first = profile(target, first)?;
            let second = profile(target, second)?;
            if first.degree() != second.degree()
                || first.knots() != second.knots()
                || first.weights() != second.weights()
                || first.periodic() != second.periodic()
            {
                return Ok(None);
            }
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
        _ => return Ok(None),
    };
    Ok(Some(generated))
}

#[cfg(test)]
mod tests;
