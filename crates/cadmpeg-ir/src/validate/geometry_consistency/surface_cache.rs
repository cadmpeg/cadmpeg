// SPDX-License-Identifier: Apache-2.0
//! Compare a procedural surface's solved cache with its construction.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::{decode::DecodeContext, CodecError};

use crate::eval::{admission::EvaluationAdmission, EvaluationFailure};
use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::{ids::SurfaceId, CadIr};

use super::{measured_point, procedural_support_allowance, solved_surface_parameter_domains};

/// Surfaces evaluated by a feature whose stored output is explicitly stale.
fn stale_surfaces(ir: &CadIr) -> BTreeSet<&SurfaceId> {
    let bodies: BTreeSet<_> = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.needs_recompute)
        .flat_map(|feature| feature.evaluation.outputs())
        .collect();
    let regions: BTreeMap<_, _> = ir
        .model
        .regions
        .iter()
        .map(|region| (&region.id, &region.body))
        .collect();
    let shells: BTreeMap<_, _> = ir
        .model
        .shells
        .iter()
        .map(|shell| (&shell.id, &shell.region))
        .collect();
    ir.model
        .faces
        .iter()
        .filter_map(|face| {
            let body = regions.get(shells.get(&face.shell)?)?;
            bodies.contains(body).then_some(&face.surface)
        })
        .collect()
}

fn finite_domain(
    ctx: &DecodeContext<'_>,
    cache: &SolvedSurfaceGeometry,
    bounds: Option<crate::geometry::RecordBounds>,
) -> Result<Option<[[f64; 2]; 2]>, CodecError> {
    if let Some(domain) = solved_surface_parameter_domains(ctx, cache)? {
        return Ok(Some(domain));
    }
    // Unbounded analytic carriers need a finite construction interval. Do not
    // invent a unit interval when the document supplies no evaluation domain.
    Ok(bounds.and_then(|bounds| match bounds.get() {
        [Some(u0), Some(v0), Some(u1), Some(v1)] if u0 <= u1 && v0 <= v1 => {
            Some([[u0, u1], [v0, v1]])
        }
        _ => None,
    }))
}

pub(super) fn check(
    ctx: &DecodeContext<'_>,
    index: &crate::index::ModelIndex<'_>,
    evaluations: &crate::eval::admission::ModelEvaluationPass,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let ir = index.ir();
    let stale = stale_surfaces(ir);
    let constructions: BTreeMap<_, _> = ir
        .model
        .procedural_surfaces
        .iter()
        .map(|construction| (&construction.id, construction))
        .collect();
    for surface in &ir.model.surfaces {
        let SurfaceGeometry::Procedural {
            construction,
            cache: Some(cache),
        } = &surface.geometry
        else {
            continue;
        };
        if stale.contains(&surface.id) {
            continue;
        }
        let Some(definition) = constructions.get(construction) else {
            continue;
        };
        let Some([u, v]) = finite_domain(ctx, cache, definition.record_bounds())? else {
            continue;
        };
        let cacheless = SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        };
        let tolerance = procedural_support_allowance(
            ir.tolerances.linear,
            definition
                .cache_fit_tolerance()
                .map(crate::geometry::FitTolerance::get),
        );
        let mut maximum = 0.0_f64;
        let mut compared = false;
        for u in [u[0], u[0].midpoint(u[1]), u[1]] {
            for v in [v[0], v[0].midpoint(v[1]), v[1]] {
                let intended =
                    evaluations.evaluate(EvaluationAdmission::Decode(ctx), |admission| {
                        crate::eval::model_surface_point_in_index(
                            admission, index, &cacheless, u, v,
                        )
                    });
                let cached = evaluations.evaluate(EvaluationAdmission::Decode(ctx), |admission| {
                    crate::eval::decode::surface_point_solved(admission, cache, u, v)
                });
                if matches!(intended, Err(EvaluationFailure::WorkExhausted))
                    || matches!(cached, Err(EvaluationFailure::WorkExhausted))
                {
                    continue;
                }
                let (Some(intended), Some(cached)) =
                    (measured_point(intended)?, measured_point(cached)?)
                else {
                    continue;
                };
                compared = true;
                let distance = crate::math::Point3::distance(intended, cached);
                maximum = if distance.is_finite() {
                    maximum.max(distance)
                } else {
                    f64::INFINITY
                };
            }
        }
        if compared && maximum > tolerance {
            super::super::record_finding(findings, Check::GeometricConsistency, Severity::Error,
                Some(surface.id.as_str()), format_args!(
                    "procedural surface solved cache disagrees with its construction by {maximum:.6}; cache fit allowance is {tolerance:.6}"
                ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
