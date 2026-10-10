// SPDX-License-Identifier: Apache-2.0
//! Selection of the actual construction or current cache for a model surface.

use crate::eval::admission;
use crate::eval::cacheless_constant_rolling_ball_first_order;
use crate::eval::cacheless_law_sweep_first_order;
use crate::eval::cacheless_variable_blend_first_order;
use crate::eval::depth::ModelEvaluationDepthGuard;
use crate::eval::revision_surface_tail_has_current_cache;
use crate::eval::sweep_has_current_cache;
use crate::eval::variable_blend_has_current_cache;
use crate::eval::{EvaluationFailure, SurfaceFirstOrder, SurfaceJet};
use crate::geometry::ProceduralSurfaceDefinition;
use crate::math::Point3;
use super::SurfaceRequest;

/// The point and first partials of an arena surface, the first partials
/// with their own outcome, or why the point has none.
///
/// A cacheless blend or sweep whose point and first partials both have
/// values is evaluated with the selected admission policy.
/// Otherwise one with a current cache falls back to the cache: the cache's
/// complete evaluation wins, then an evaluation with a point, the cacheless
/// one first; of two failures, the cacheless one outside the finite range
/// wins.
pub(in crate::eval) fn first_order(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    let budget = admission.work_slice();
    let _depth =
        ModelEvaluationDepthGuard::enter(budget).map_err(EvaluationFailure::ResourceLimit)?;
    let cacheless = match index
        .procedural_surface_for_surface(surface.as_str(), admission)
        .map_err(EvaluationFailure::ResourceLimit)?
        .map(crate::geometry::ProceduralSurface::definition)
    {
        Some(ProceduralSurfaceDefinition::Blend(definition_payload)) => {
            definition_payload.native().map(|native| {
                (
                    cacheless_constant_rolling_ball_first_order(
                        admission,
                        index,
                        definition_payload,
                        u,
                        v,
                    ),
                    revision_surface_tail_has_current_cache(&native.cache),
                )
            })
        }
        Some(ProceduralSurfaceDefinition::VariableBlend(definition_payload)) => {
            let construction = definition_payload.construction();
            Some((
                cacheless_variable_blend_first_order(admission, index, definition_payload, u, v),
                variable_blend_has_current_cache(construction),
            ))
        }
        Some(ProceduralSurfaceDefinition::Sweep(definition_payload)) => {
            definition_payload.native().as_deref().map(|construction| {
                (
                    cacheless_law_sweep_first_order(
                        admission,
                        index,
                        definition_payload.profile(),
                        definition_payload.spine(),
                        construction,
                        u,
                        v,
                    ),
                    sweep_has_current_cache(construction),
                )
            })
        }
        _ => None,
    };
    let cached =
        || super::model_jet(admission, index, surface, u, v, SurfaceRequest::First).map(SurfaceJet::first_order);
    let complete = |order: &Result<SurfaceFirstOrder, EvaluationFailure<Point3>>| match order {
        Ok(order) => match &order.first {
            Ok(_) => Ok(true),
            Err(EvaluationFailure::ResourceLimit(limit)) => {
                Err(EvaluationFailure::ResourceLimit(*limit))
            }
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(())) => Ok(false),
        },
        Err(EvaluationFailure::ResourceLimit(limit)) => {
            Err(EvaluationFailure::ResourceLimit(*limit))
        }
        Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => Ok(false),
    };
    let Some((cacheless, has_current_cache)) = cacheless else {
        return cached();
    };
    if complete(&cacheless)? || !has_current_cache {
        return cacheless;
    }
    let cached = cached();
    if complete(&cached)? {
        return cached;
    }
    match (cacheless, cached) {
        (Ok(order), _) | (Err(_), Ok(order)) => Ok(order),
        (Err(cacheless), Err(cached)) => Err(match cacheless {
            EvaluationFailure::NonFinite(_) => cacheless,
            EvaluationFailure::NoValue => cached,
            EvaluationFailure::ResourceLimit(limit) => EvaluationFailure::ResourceLimit(limit),
        }),
    }
}

