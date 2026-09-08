// SPDX-License-Identifier: Apache-2.0
//! Validation for `SubD` cages and free-carrier source associations.
#![allow(clippy::wildcard_imports)]

use super::*;
use crate::validate::geometry_payloads::bounds_err;

const EPS_SUBD_CHECK_PROCEDURAL_SURFACES_E9: f64 = 1.0e-9;

fn check_source(
    source: Option<&crate::provenance::SourceObjectAssociation>,
    owner: &str,
    findings: &mut Vec<Finding>,
) {
    let Some(source) = source else { return };
    if source.object_id.is_empty() {
        bounds_err(
            findings,
            owner,
            "source association object_id must not be empty",
        );
    }
    if source.color.is_some_and(|color| {
        [color.r, color.g, color.b, color.a]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    }) {
        bounds_err(
            findings,
            owner,
            "source association color is not finite or outside [0, 1]",
        );
    }
}

pub(super) fn check_source_associations(ir: &CadIr, findings: &mut Vec<Finding>) {
    for surface in &ir.model.surfaces {
        check_source(
            surface.source_object.as_ref(),
            surface.id.as_str(),
            findings,
        );
    }
    for curve in &ir.model.curves {
        check_source(curve.source_object.as_ref(), curve.id.as_str(), findings);
    }
    for point in &ir.model.points {
        check_source(point.source_object.as_ref(), point.id.as_str(), findings);
    }
    for mesh in &ir.model.tessellations {
        check_source(mesh.source_object.as_ref(), mesh.id.as_str(), findings);
    }
    for subd in &ir.model.subds {
        check_source(subd.source_object.as_ref(), subd.id.as_str(), findings);
    }
}

pub(super) fn check_procedural_surfaces(ir: &CadIr, findings: &mut Vec<Finding>) {
    for procedural in &ir.model.procedural_surfaces {
        if let crate::geometry::ProceduralSurfaceDefinition::Revolution {
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            ..
        } = procedural.definition()
        {
            let valid = [
                Some(angular_interval),
                angular_parameter_interval.as_ref(),
                parameter_interval.as_ref(),
            ]
            .into_iter()
            .flatten()
            .all(|interval| {
                interval[0].is_finite() && interval[1].is_finite() && interval[0] < interval[1]
            });
            if !valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "revolution interval is not finite and ordered",
                );
            }
        }
        if let crate::geometry::ProceduralSurfaceDefinition::AxisRevolution {
            axis_origin,
            axis_direction,
            ..
        } = procedural.definition()
        {
            if ![
                axis_origin.x,
                axis_origin.y,
                axis_origin.z,
                axis_direction.x,
                axis_direction.y,
                axis_direction.z,
            ]
            .into_iter()
            .all(f64::is_finite)
                || (axis_direction.norm() - 1.0).abs() > EPS_SUBD_CHECK_PROCEDURAL_SURFACES_E9
            {
                bounds_err(findings, procedural.id.as_str(), "invalid revolution axis");
            }
        }
        if let crate::geometry::ProceduralSurfaceDefinition::Sum { basepoint, .. } =
            procedural.definition()
        {
            if !basepoint.x.is_finite() || !basepoint.y.is_finite() || !basepoint.z.is_finite() {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "sum basepoint is not finite",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
