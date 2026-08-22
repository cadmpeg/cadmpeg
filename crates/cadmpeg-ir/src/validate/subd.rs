// SPDX-License-Identifier: Apache-2.0
//! Validation for `SubD` cages and free-carrier source associations.
#![allow(clippy::wildcard_imports)]

use std::collections::BTreeSet;

use super::*;
use crate::math::Point3;
use crate::validate::geometry_payloads::bounds_err;

fn finite_point(point: &Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn check_source(
    source: Option<&crate::provenance::SourceObjectAssociation>,
    owner: &str,
    findings: &mut Vec<Finding>,
) {
    let Some(source) = source else { return };
    if source.format.is_empty() || source.object_id.is_empty() {
        bounds_err(
            findings,
            owner,
            "source association format and object_id must not be empty",
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
        check_source(surface.source_object.as_ref(), &surface.id.0, findings);
    }
    for curve in &ir.model.curves {
        check_source(curve.source_object.as_ref(), &curve.id.0, findings);
    }
    for point in &ir.model.points {
        check_source(point.source_object.as_ref(), &point.id.0, findings);
    }
    for mesh in &ir.model.tessellations {
        check_source(mesh.source_object.as_ref(), &mesh.id, findings);
    }
    for subd in &ir.model.subds {
        check_source(subd.source_object.as_ref(), &subd.id.0, findings);
    }
}

pub(super) fn check_subds(ir: &CadIr, findings: &mut Vec<Finding>) {
    for subd in &ir.model.subds {
        let vertex_count = subd.vertices.len();
        let edge_count = subd.edges.len();
        let face_count = subd.faces.len();
        let mut grip_indices = BTreeSet::new();
        for (index, vertex) in subd.vertices.iter().enumerate() {
            if !finite_point(&vertex.point) {
                bounds_err(
                    findings,
                    &subd.id.0,
                    &format!("SubD vertex {index} is not finite"),
                );
            }
            let Some(layout) = &vertex.secondary_grips else {
                continue;
            };
            if layout.wedges.is_empty() {
                bounds_err(
                    findings,
                    &subd.id.0,
                    &format!("SubD vertex {index} has an empty secondary-grip layout"),
                );
                continue;
            }
            for (wedge_index, wedge) in layout.wedges.iter().enumerate() {
                let next = &layout.wedges[(wedge_index + 1) % layout.wedges.len()];
                let expected_sectors = wedge.spokes.len().checked_mul(next.spokes.len());
                if expected_sectors != Some(wedge.sectors.len()) {
                    bounds_err(
                        findings,
                        &subd.id.0,
                        &format!(
                            "SubD vertex {index} wedge {wedge_index} has invalid sector arity"
                        ),
                    );
                }
                if wedge.phantom
                    && (wedge.edge.is_some()
                        || wedge.sector_face.is_some()
                        || !wedge.spokes.is_empty()
                        || !wedge.sectors.is_empty())
                {
                    bounds_err(
                        findings,
                        &subd.id.0,
                        &format!(
                            "SubD vertex {index} wedge {wedge_index} has invalid phantom data"
                        ),
                    );
                }
                if wedge.edge.is_some_and(|edge| edge as usize >= edge_count)
                    || wedge
                        .sector_face
                        .is_some_and(|face| face as usize >= face_count)
                {
                    bounds_err(
                        findings,
                        &subd.id.0,
                        &format!(
                            "SubD vertex {index} wedge {wedge_index} has an invalid topology reference"
                        ),
                    );
                }
                for grip in wedge.spokes.iter().chain(&wedge.sectors).flatten() {
                    if !finite_point(&grip.point)
                        || !grip.weight.is_finite()
                        || grip.weight <= 0.0
                        || !grip_indices.insert(grip.source_index)
                    {
                        bounds_err(
                            findings,
                            &subd.id.0,
                            &format!(
                                "SubD vertex {index} has an invalid or repeated secondary grip"
                            ),
                        );
                    }
                }
            }
        }
        for (index, edge) in subd.edges.iter().enumerate() {
            if edge.vertices[0] == edge.vertices[1]
                || edge.vertices.iter().any(|v| *v as usize >= vertex_count)
                || edge.sharpness.iter().any(|v| !v.is_finite() || *v < 0.0)
                || edge.sector_coefficients.iter().any(|v| !v.is_finite())
            {
                bounds_err(
                    findings,
                    &subd.id.0,
                    &format!("SubD edge {index} is invalid"),
                );
            }
        }
        for (face_index, face) in subd.faces.iter().enumerate() {
            if face.edges.len() < 3 {
                bounds_err(
                    findings,
                    &subd.id.0,
                    &format!("SubD face {face_index} has fewer than three edge uses"),
                );
                continue;
            }
            let endpoints = face
                .edges
                .iter()
                .filter_map(|use_| {
                    subd.edges.get(use_.edge as usize).map(|edge| {
                        if use_.reversed {
                            (edge.vertices[1], edge.vertices[0])
                        } else {
                            (edge.vertices[0], edge.vertices[1])
                        }
                    })
                })
                .collect::<Vec<_>>();
            if face
                .edges
                .iter()
                .any(|use_| use_.edge as usize >= edge_count)
                || endpoints.len() != face.edges.len()
                || endpoints
                    .iter()
                    .enumerate()
                    .any(|(i, (_, end))| *end != endpoints[(i + 1) % endpoints.len()].0)
            {
                bounds_err(
                    findings,
                    &subd.id.0,
                    &format!("SubD face {face_index} ring is not directed and closed"),
                );
            }
        }
    }
}

pub(super) fn check_procedural_surfaces(ir: &CadIr, findings: &mut Vec<Finding>) {
    for procedural in &ir.model.procedural_surfaces {
        if let crate::geometry::ProceduralSurfaceDefinition::Revolution {
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            ..
        } = &procedural.definition
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
                    &procedural.id.0,
                    "revolution interval is not finite and ordered",
                );
            }
        }
        if let crate::geometry::ProceduralSurfaceDefinition::AxisRevolution {
            axis_origin,
            axis_direction,
            ..
        } = &procedural.definition
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
                || (axis_direction.norm() - 1.0).abs() > 1e-9
            {
                bounds_err(findings, &procedural.id.0, "invalid revolution axis");
            }
        }
        if let crate::geometry::ProceduralSurfaceDefinition::Sum { basepoint, .. } =
            &procedural.definition
        {
            if !basepoint.x.is_finite() || !basepoint.y.is_finite() || !basepoint.z.is_finite() {
                bounds_err(findings, &procedural.id.0, "sum basepoint is not finite");
            }
        }
    }
}

#[cfg(test)]
mod tests;
