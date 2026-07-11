// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for geometry payloads.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;

pub(super) fn check_unknown_payloads(ir: &CadIr, findings: &mut Vec<Finding>) {
    for record in &ir.unknowns {
        let Some(data) = &record.data else { continue };
        let hash = Sha256::digest(data)
            .iter()
            .fold(String::new(), |mut acc, byte| {
                use std::fmt::Write as _;
                let _ = write!(acc, "{byte:02x}");
                acc
            });
        if data.len() as u64 != record.byte_len || hash != record.sha256 {
            findings.push(Finding {
                check: Check::PayloadIntegrity,
                severity: Severity::Error,
                message: "preserved payload length or hash does not match its record".into(),
                entity: Some(record.id.0.clone()),
            });
        }
    }
}

pub(super) fn check_tessellations(ir: &CadIr, findings: &mut Vec<Finding>) {
    for mesh in &ir.model.tessellations {
        if mesh
            .vertices
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a non-finite tessellation vertex".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .triangles
            .iter()
            .flatten()
            .any(|index| *index as usize >= mesh.vertices.len())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains an out-of-range tessellation index".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .normals
            .iter()
            .any(|normal| !normal.x.is_finite() || !normal.y.is_finite() || !normal.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a non-finite tessellation normal".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh.channels.iter().any(|channel| {
            channel.data.len() != channel.item_size as usize * channel.count as usize
        }) {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a malformed tessellation channel".into(),
                entity: Some(mesh.id.clone()),
            });
        }
    }
}

pub(super) fn degenerate(v: &Vector3) -> bool {
    v.norm() <= f64::EPSILON
}

pub(super) fn check_bounds(ir: &CadIr, findings: &mut Vec<Finding>) {
    for (id, tolerance) in ir
        .model
        .vertices
        .iter()
        .map(|entity| (&entity.id.0, entity.tolerance))
        .chain(
            ir.model
                .edges
                .iter()
                .map(|entity| (&entity.id.0, entity.tolerance)),
        )
        .chain(
            ir.model
                .faces
                .iter()
                .map(|entity| (&entity.id.0, entity.tolerance)),
        )
    {
        if tolerance.is_some_and(nonpositive) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Error,
                message: "topology tolerance is not positive and finite".into(),
                entity: Some(id.clone()),
            });
        } else if tolerance.is_some_and(|value| value > 1.0e6) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Warning,
                message: "topology tolerance is outside a sane canonical range".into(),
                entity: Some(id.clone()),
            });
        }
    }
    for s in &ir.model.surfaces {
        match &s.geometry {
            SurfaceGeometry::Plane { normal, u_axis, .. } => {
                if degenerate(normal) {
                    bounds_err(findings, &s.id.0, "plane normal is degenerate");
                }
                if degenerate(u_axis) {
                    bounds_err(findings, &s.id.0, "plane u axis is degenerate");
                }
            }
            SurfaceGeometry::Cylinder {
                axis,
                ref_direction,
                radius,
                ..
            } => {
                if degenerate(axis) {
                    bounds_err(findings, &s.id.0, "cylinder axis is degenerate");
                }
                if degenerate(ref_direction) {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "cylinder reference direction is degenerate",
                    );
                }
                if nonpositive(*radius) {
                    bounds_err(findings, &s.id.0, "cylinder radius is not positive");
                }
            }
            SurfaceGeometry::Cone {
                axis,
                ref_direction,
                radius,
                ..
            } => {
                if degenerate(axis) {
                    bounds_err(findings, &s.id.0, "cone axis is degenerate");
                }
                if degenerate(ref_direction) {
                    bounds_err(findings, &s.id.0, "cone reference direction is degenerate");
                }
                if *radius < 0.0 {
                    bounds_err(findings, &s.id.0, "cone radius is negative");
                }
            }
            SurfaceGeometry::Sphere {
                axis,
                ref_direction,
                radius,
                ..
            } => {
                if degenerate(axis) {
                    bounds_err(findings, &s.id.0, "sphere axis is degenerate");
                }
                if degenerate(ref_direction) {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "sphere reference direction is degenerate",
                    );
                }
                if radius.abs() <= f64::EPSILON {
                    bounds_err(findings, &s.id.0, "sphere radius is zero");
                }
            }
            SurfaceGeometry::Torus {
                axis,
                ref_direction,
                major_radius,
                minor_radius,
                ..
            } => {
                if degenerate(axis) {
                    bounds_err(findings, &s.id.0, "torus axis is degenerate");
                }
                if degenerate(ref_direction) {
                    bounds_err(findings, &s.id.0, "torus reference direction is degenerate");
                }
                if nonpositive(*major_radius) || minor_radius.abs() <= f64::EPSILON {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "torus major radius is not positive or minor radius is zero",
                    );
                }
            }
            SurfaceGeometry::Nurbs(n) => {
                let expected = (n.u_count as usize) * (n.v_count as usize);
                if n.control_points.len() != expected {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "NURBS surface pole count does not match u_count*v_count",
                    );
                }
                check_knots(findings, &s.id.0, &n.u_knots, "u");
                check_knots(findings, &s.id.0, &n.v_knots, "v");
            }
            // An unknown surface carries no numeric geometry to bounds-check; its
            // record link is checked in `check_references`. A face resting on it
            // is legal (topology known, shape opaque).
            SurfaceGeometry::Unknown { .. } => {}
        }
    }
    for c in &ir.model.curves {
        match &c.geometry {
            CurveGeometry::Line { direction, .. } => {
                if degenerate(direction) {
                    bounds_err(findings, &c.id.0, "line direction is degenerate");
                }
            }
            CurveGeometry::Circle { axis, radius, .. } => {
                if degenerate(axis) {
                    bounds_err(findings, &c.id.0, "circle axis is degenerate");
                }
                if nonpositive(*radius) {
                    bounds_err(findings, &c.id.0, "circle radius is not positive");
                }
            }
            CurveGeometry::Ellipse {
                major_radius,
                minor_radius,
                ..
            } => {
                if nonpositive(*major_radius) || nonpositive(*minor_radius) {
                    bounds_err(findings, &c.id.0, "ellipse radius is not positive");
                }
            }
            CurveGeometry::Parabola {
                axis,
                major_direction,
                focal_distance,
                ..
            } => {
                if degenerate(axis) || degenerate(major_direction) {
                    bounds_err(findings, &c.id.0, "parabola frame is degenerate");
                }
                if nonpositive(*focal_distance) {
                    bounds_err(findings, &c.id.0, "parabola focal distance is not positive");
                }
            }
            CurveGeometry::Hyperbola {
                axis,
                major_direction,
                major_radius,
                minor_radius,
                ..
            } => {
                if degenerate(axis) || degenerate(major_direction) {
                    bounds_err(findings, &c.id.0, "hyperbola frame is degenerate");
                }
                if nonpositive(*major_radius) || nonpositive(*minor_radius) {
                    bounds_err(findings, &c.id.0, "hyperbola radius is not positive");
                }
            }
            CurveGeometry::Degenerate { point } => {
                if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
                    bounds_err(findings, &c.id.0, "degenerate curve point is not finite");
                }
            }
            CurveGeometry::Nurbs(n) => {
                if n.control_points.len() < (n.degree as usize + 1) {
                    bounds_err(
                        findings,
                        &c.id.0,
                        "NURBS curve has too few poles for its degree",
                    );
                }
                check_knots(findings, &c.id.0, &n.knots, "");
            }
            CurveGeometry::Unknown { .. } => {}
        }
    }
    for procedural in &ir.model.procedural_curves {
        if let ProceduralCurveDefinition::Compound {
            parameters,
            component_parameters,
            components,
        } = &procedural.definition
        {
            if components.is_empty() || component_parameters.len() != components.len() {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "compound components are empty or do not match component parameters",
                );
            }
            if parameters
                .iter()
                .chain(component_parameters)
                .any(|value| !value.is_finite())
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "compound parameters are not finite",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Subset {
            parameter_range, ..
        } = &procedural.definition
        {
            if !parameter_range.iter().all(|value| value.is_finite())
                || parameter_range[0] > parameter_range[1]
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "subset-curve range is not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::VectorOffset {
            parameter_range,
            offset,
            ..
        } = &procedural.definition
        {
            if !parameter_range.iter().all(|value| value.is_finite())
                || parameter_range[0] > parameter_range[1]
                || !offset.x.is_finite()
                || !offset.y.is_finite()
                || !offset.z.is_finite()
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "vector-offset fields are not finite and ordered",
                );
            }
            continue;
        }
        let ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        } = &procedural.definition
        else {
            continue;
        };
        let finite = angle_range.iter().all(|value| value.is_finite())
            && center.x.is_finite()
            && center.y.is_finite()
            && center.z.is_finite()
            && [major, minor, pitch, axis]
                .into_iter()
                .flat_map(|vector| [vector.x, vector.y, vector.z])
                .all(f64::is_finite)
            && apex_factor.is_finite();
        if !finite || angle_range[0] > angle_range[1] {
            bounds_err(
                findings,
                &procedural.id.0,
                "helix fields are not finite and ordered",
            );
        }
        if degenerate(major) || degenerate(minor) || degenerate(axis) {
            bounds_err(findings, &procedural.id.0, "helix frame is degenerate");
        }
        if (major.norm() - minor.norm()).abs() > 1e-9 {
            bounds_err(
                findings,
                &procedural.id.0,
                "helix major and minor radii differ",
            );
        }
    }
}

pub(super) fn check_knots(findings: &mut Vec<Finding>, id: &str, knots: &[f64], dir: &str) {
    if knots.windows(2).any(|w| w[1] < w[0]) {
        let label = if dir.is_empty() {
            "knot vector is not non-decreasing".to_string()
        } else {
            format!("{dir}-knot vector is not non-decreasing")
        };
        bounds_err(findings, id, &label);
    }
}

pub(super) fn bounds_err(findings: &mut Vec<Finding>, id: &str, msg: &str) {
    findings.push(Finding {
        check: Check::Bounds,
        severity: Severity::Error,
        message: msg.to_string(),
        entity: Some(id.to_string()),
    });
}
