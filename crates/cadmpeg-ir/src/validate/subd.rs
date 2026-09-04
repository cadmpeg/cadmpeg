// SPDX-License-Identifier: Apache-2.0
//! Validation for `SubD` cages and free-carrier source associations.
#![allow(clippy::wildcard_imports)]

use std::collections::BTreeSet;

use super::*;
use crate::math::{Point3, Vector3};
use crate::subd::{SubdGripWedge, SubdSurface, SubdSymmetryKind};
use crate::validate::geometry_payloads::bounds_err;

const EPS_SUBD_CHECK_PROCEDURAL_SURFACES_E9: f64 = 1.0e-9;

const SUBD_SYMMETRY_FRAME_EPS: f64 = 1.0e-9;

fn finite_point(point: &Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn finite_vector(vector: &Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn check_symmetry_pairs(
    id: &str,
    symmetry_index: usize,
    element: &str,
    pairs: &[[u32; 2]],
    count: usize,
    findings: &mut Vec<Finding>,
) {
    let mut sources = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for (pair_index, [source, target]) in pairs.iter().copied().enumerate() {
        let source_in_range = usize::try_from(source).is_ok_and(|index| index < count);
        let target_in_range = usize::try_from(target).is_ok_and(|index| index < count);
        let source_unique = sources.insert(source);
        let target_unique = targets.insert(target);
        if !source_in_range || !target_in_range || !source_unique || !target_unique {
            bounds_err(
                findings,
                id,
                &format!("SubD symmetry {symmetry_index} {element} pair {pair_index} is invalid"),
            );
        }
    }
}

fn check_radial_maps(
    id: &str,
    symmetry_index: usize,
    maps: &[crate::subd::SubdRadialSymmetryMap],
    findings: &mut Vec<Finding>,
) {
    let mut selectors = BTreeSet::new();
    for (map_index, map) in maps.iter().enumerate() {
        if !selectors.insert(map.selector) {
            bounds_err(
                findings,
                id,
                &format!("SubD symmetry {symmetry_index} radial map selector {map_index} repeats"),
            );
        }
        let mut sources = BTreeSet::new();
        for (pair_index, [source, _]) in map.pairs.iter().copied().enumerate() {
            if !sources.insert(source) {
                bounds_err(
                    findings,
                    id,
                    &format!(
                        "SubD symmetry {symmetry_index} radial map {map_index} pair {pair_index} repeats a source"
                    ),
                );
            }
        }
    }
}

fn check_symmetries(
    subd: &SubdSurface,
    vertex_count: usize,
    edge_count: usize,
    face_count: usize,
    findings: &mut Vec<Finding>,
) {
    for (symmetry_index, symmetry) in subd.symmetries.iter().enumerate() {
        let plane = &symmetry.plane;
        let frame_valid = finite_point(&plane.origin)
            && finite_vector(&plane.first_axis)
            && finite_vector(&plane.second_axis)
            && (plane.first_axis.norm() - 1.0).abs() <= SUBD_SYMMETRY_FRAME_EPS
            && (plane.second_axis.norm() - 1.0).abs() <= SUBD_SYMMETRY_FRAME_EPS
            && plane.first_axis.dot(plane.second_axis).abs() <= SUBD_SYMMETRY_FRAME_EPS;
        if !frame_valid {
            bounds_err(
                findings,
                &subd.id.0,
                &format!("SubD symmetry {symmetry_index} plane frame is invalid"),
            );
        }
        if let SubdSymmetryKind::Radial {
            segments,
            sweep,
            radial_maps,
        } = &symmetry.kind
        {
            if *segments == 0 || !sweep.is_finite() {
                bounds_err(
                    findings,
                    &subd.id.0,
                    &format!("SubD symmetry {symmetry_index} radial controls are invalid"),
                );
            }
            check_radial_maps(&subd.id.0, symmetry_index, radial_maps, findings);
        }
        check_symmetry_pairs(
            &subd.id.0,
            symmetry_index,
            "face",
            &symmetry.face_pairs,
            face_count,
            findings,
        );
        check_symmetry_pairs(
            &subd.id.0,
            symmetry_index,
            "edge",
            &symmetry.edge_pairs,
            edge_count,
            findings,
        );
        check_symmetry_pairs(
            &subd.id.0,
            symmetry_index,
            "vertex",
            &symmetry.vertex_pairs,
            vertex_count,
            findings,
        );
    }
}

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
                let spoke_count = |wedge: &SubdGripWedge| match wedge {
                    SubdGripWedge::Phantom => 0,
                    SubdGripWedge::Slot { spokes, .. } => spokes.len(),
                };
                let sector_count = match wedge {
                    SubdGripWedge::Phantom => 0,
                    SubdGripWedge::Slot { sectors, .. } => sectors.len(),
                };
                let expected_sectors = spoke_count(wedge).checked_mul(spoke_count(next));
                if expected_sectors != Some(sector_count) {
                    bounds_err(
                        findings,
                        &subd.id.0,
                        &format!(
                            "SubD vertex {index} wedge {wedge_index} has invalid sector arity"
                        ),
                    );
                }
                let SubdGripWedge::Slot {
                    edge,
                    sector_face,
                    spokes,
                    sectors,
                } = wedge
                else {
                    continue;
                };
                if edge.is_some_and(|edge| edge as usize >= edge_count)
                    || sector_face.is_some_and(|face| face as usize >= face_count)
                {
                    bounds_err(
                        findings,
                        &subd.id.0,
                        &format!(
                            "SubD vertex {index} wedge {wedge_index} has an invalid topology reference"
                        ),
                    );
                }
                if edge
                    .and_then(|edge| subd.edges.get(edge as usize))
                    .is_some_and(|edge| {
                        u32::try_from(index).map_or(true, |owner| !edge.vertices.contains(&owner))
                    })
                {
                    bounds_err(
                        findings,
                        &subd.id.0,
                        &format!(
                            "SubD vertex {index} wedge {wedge_index} edge is not incident to its owner"
                        ),
                    );
                }
                if let Some(face) = sector_face.and_then(|face| subd.faces.get(face as usize)) {
                    let incident = match u32::try_from(index) {
                        Ok(owner) => face.edges.iter().any(|use_| {
                            subd.edges
                                .get(use_.edge as usize)
                                .is_some_and(|edge| edge.vertices.contains(&owner))
                        }),
                        Err(_) => false,
                    };
                    if !incident {
                        bounds_err(
                            findings,
                            &subd.id.0,
                            &format!(
                                "SubD vertex {index} wedge {wedge_index} sector face is not incident to its owner"
                            ),
                        );
                    }
                }
                for grip in spokes.iter().chain(sectors).flatten() {
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
                || edge
                    .knot_interval
                    .is_some_and(|interval| !interval.is_finite() || interval <= 0.0)
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
        check_symmetries(subd, vertex_count, edge_count, face_count, findings);
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
                    &procedural.id.0,
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
                bounds_err(findings, &procedural.id.0, "invalid revolution axis");
            }
        }
        if let crate::geometry::ProceduralSurfaceDefinition::Sum { basepoint, .. } =
            procedural.definition()
        {
            if !basepoint.x.is_finite() || !basepoint.y.is_finite() || !basepoint.z.is_finite() {
                bounds_err(findings, &procedural.id.0, "sum basepoint is not finite");
            }
        }
    }
}

#[cfg(test)]
mod tests;
