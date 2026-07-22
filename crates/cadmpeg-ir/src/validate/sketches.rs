// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for sketches.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::sketches::{
    SketchConstraintDefinition as Constraint, SketchGeometry, SketchLocus, SpatialSketchGeometry,
};
use std::collections::HashMap;

fn finding(findings: &mut Vec<Finding>, check: Check, id: &str, message: &str) {
    findings.push(Finding {
        check,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(id.into()),
    });
}

fn finite2(point: crate::math::Point2) -> bool {
    point.u.is_finite() && point.v.is_finite()
}

fn finite3(point: crate::math::Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn valid_vector(vector: crate::math::Vector3) -> bool {
    let norm = vector.norm();
    norm.is_finite() && norm > 0.0
}

fn perpendicular(first: crate::math::Vector3, second: crate::math::Vector3) -> bool {
    let first_norm = first.norm();
    let second_norm = second.norm();
    valid_vector(first)
        && valid_vector(second)
        && (first.x * second.x + first.y * second.y + first.z * second.z).abs()
            <= 1.0e-9 * first_norm * second_norm
}

pub(super) fn check_sketches(ir: &CadIr, findings: &mut Vec<Finding>) {
    let entity_geometry = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    for sketch in &ir.model.sketches {
        let normal = sketch.normal.norm();
        let u_norm = sketch.u_axis.norm();
        let dot = sketch.normal.x * sketch.u_axis.x
            + sketch.normal.y * sketch.u_axis.y
            + sketch.normal.z * sketch.u_axis.z;
        if !normal.is_finite() || normal <= 0.0 || !u_norm.is_finite() || u_norm <= 0.0 {
            finding(
                findings,
                Check::Bounds,
                &sketch.id.0,
                "sketch plane has a degenerate axis",
            );
        } else if dot.abs() > 1.0e-9 * normal * u_norm {
            finding(
                findings,
                Check::GeometricConsistency,
                &sketch.id.0,
                "sketch plane axes are not perpendicular",
            );
        }
        if !sketch.origin.x.is_finite()
            || !sketch.origin.y.is_finite()
            || !sketch.origin.z.is_finite()
        {
            finding(
                findings,
                Check::Bounds,
                &sketch.id.0,
                "sketch origin is not finite",
            );
        }
        if sketch.profiles.iter().any(Vec::is_empty) {
            finding(
                findings,
                Check::Counts,
                &sketch.id.0,
                "sketch contains an empty profile",
            );
        }
        for profile in &sketch.profiles {
            for adjacent in profile.windows(2) {
                let Some(left) = entity_geometry
                    .get(&adjacent[0].entity)
                    .and_then(|geometry| oriented_endpoints(geometry, adjacent[0].reversed))
                else {
                    continue;
                };
                let Some(right) = entity_geometry
                    .get(&adjacent[1].entity)
                    .and_then(|geometry| oriented_endpoints(geometry, adjacent[1].reversed))
                else {
                    continue;
                };
                if distance2(left.1, right.0) > 1.0e-9 {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &sketch.id.0,
                        "sketch profile has disconnected consecutive entities",
                    );
                }
            }
        }
    }

    for entity in &ir.model.sketch_entities {
        let id = &entity.id.0;
        match &entity.geometry {
            SketchGeometry::Point { position } => {
                if !finite2(*position) {
                    finding(findings, Check::Bounds, id, "sketch point is not finite");
                }
            }
            SketchGeometry::Line { start, end } => {
                if !finite2(*start) || !finite2(*end) {
                    finding(findings, Check::Bounds, id, "sketch line is not finite");
                }
            }
            SketchGeometry::ReferenceLine { origin, direction } => {
                if !finite2(*origin)
                    || !finite2(*direction)
                    || direction.u.hypot(direction.v) <= f64::EPSILON
                {
                    finding(findings, Check::Bounds, id, "invalid sketch reference line");
                }
            }
            SketchGeometry::Circle { center, radius }
            | SketchGeometry::Arc { center, radius, .. } => {
                if !finite2(*center) || nonpositive(radius.0) {
                    finding(
                        findings,
                        Check::Bounds,
                        id,
                        "invalid circular sketch geometry",
                    );
                }
                if let SketchGeometry::Arc {
                    start_angle,
                    end_angle,
                    ..
                } = &entity.geometry
                {
                    if !start_angle.0.is_finite() || !end_angle.0.is_finite() {
                        finding(
                            findings,
                            Check::ParameterDomain,
                            id,
                            "arc angle is not finite",
                        );
                    }
                }
            }
            SketchGeometry::Ellipse {
                center,
                major_angle,
                major_radius,
                minor_radius,
                start_angle,
                end_angle,
            } => {
                if !finite2(*center)
                    || !major_angle.0.is_finite()
                    || nonpositive(major_radius.0)
                    || nonpositive(minor_radius.0)
                    || major_radius.0 < minor_radius.0
                {
                    finding(findings, Check::Bounds, id, "invalid sketch ellipse");
                }
                if start_angle.is_some() != end_angle.is_some()
                    || start_angle
                        .iter()
                        .chain(end_angle)
                        .any(|angle| !angle.0.is_finite())
                {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid elliptical arc parameters",
                    );
                }
            }
            SketchGeometry::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                start_parameter,
                end_parameter,
            } => {
                if !finite2(*center)
                    || !major_angle.0.is_finite()
                    || nonpositive(major_radius.0)
                    || nonpositive(minor_radius.0)
                {
                    finding(findings, Check::Bounds, id, "invalid sketch hyperbola");
                }
                if invalid_optional_parameter_pair(*start_parameter, *end_parameter) {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid hyperbolic arc parameters",
                    );
                }
            }
            SketchGeometry::Parabola {
                vertex,
                axis_angle,
                focal_length,
                start_parameter,
                end_parameter,
            } => {
                if !finite2(*vertex) || !axis_angle.0.is_finite() || nonpositive(focal_length.0) {
                    finding(findings, Check::Bounds, id, "invalid sketch parabola");
                }
                if invalid_optional_parameter_pair(*start_parameter, *end_parameter) {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid parabolic arc parameters",
                    );
                }
            }
            SketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                ..
            } => {
                let expected = control_points.len().checked_add(*degree as usize + 1);
                if *degree == 0
                    || control_points.len() <= *degree as usize
                    || expected != Some(knots.len())
                    || knots.iter().any(|value| !value.is_finite())
                    || knots.windows(2).any(|pair| pair[0] > pair[1])
                    || control_points.iter().any(|point| !finite2(*point))
                    || weights.as_ref().is_some_and(|weights| {
                        weights.len() != control_points.len()
                            || weights.iter().any(|weight| nonpositive(*weight))
                    })
                {
                    finding(findings, Check::ParameterDomain, id, "invalid sketch NURBS");
                }
            }
            SketchGeometry::Native { native_kind } => {
                if native_kind.is_empty() {
                    finding(findings, Check::Counts, id, "empty native sketch kind");
                }
            }
        }
    }

    for entity in &ir.model.spatial_sketch_entities {
        let id = &entity.id.0;
        match &entity.geometry {
            SpatialSketchGeometry::Point { position } => {
                if !finite3(*position) {
                    finding(
                        findings,
                        Check::Bounds,
                        id,
                        "spatial sketch point is not finite",
                    );
                }
            }
            SpatialSketchGeometry::Line { start, end } => {
                if !finite3(*start) || !finite3(*end) || start == end {
                    finding(findings, Check::Bounds, id, "invalid spatial sketch line");
                }
            }
            SpatialSketchGeometry::Circle {
                center,
                normal,
                reference_direction,
                radius,
            } => {
                if !finite3(*center)
                    || !valid_vector(*normal)
                    || !perpendicular(*normal, *reference_direction)
                    || nonpositive(radius.0)
                {
                    finding(findings, Check::Bounds, id, "invalid spatial sketch circle");
                }
            }
            SpatialSketchGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => {
                if !finite3(*center)
                    || !valid_vector(*normal)
                    || !perpendicular(*normal, *reference_direction)
                    || nonpositive(radius.0)
                    || !start_angle.0.is_finite()
                    || !end_angle.0.is_finite()
                {
                    finding(findings, Check::Bounds, id, "invalid spatial sketch arc");
                }
            }
            SpatialSketchGeometry::NurbsSurface {
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                control_points,
            } => {
                let columns = control_points.first().map_or(0, Vec::len);
                if *u_degree == 0
                    || *v_degree == 0
                    || control_points.len() <= *u_degree as usize
                    || columns <= *v_degree as usize
                    || control_points
                        .iter()
                        .any(|row| row.len() != columns || row.iter().any(|point| !finite3(*point)))
                    || u_knots.len() != control_points.len() + *u_degree as usize + 1
                    || v_knots.len() != columns + *v_degree as usize + 1
                    || u_knots
                        .iter()
                        .chain(v_knots)
                        .any(|value| !value.is_finite())
                    || u_knots.windows(2).any(|pair| pair[0] > pair[1])
                    || v_knots.windows(2).any(|pair| pair[0] > pair[1])
                {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid spatial sketch NURBS surface",
                    );
                }
            }
            SpatialSketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                ..
            } => {
                let expected = control_points.len().checked_add(*degree as usize + 1);
                if *degree == 0
                    || control_points.len() <= *degree as usize
                    || expected != Some(knots.len())
                    || knots.iter().any(|value| !value.is_finite())
                    || knots.windows(2).any(|pair| pair[0] > pair[1])
                    || control_points.iter().any(|point| !finite3(*point))
                    || weights.as_ref().is_some_and(|weights| {
                        weights.len() != control_points.len()
                            || weights.iter().any(|weight| nonpositive(*weight))
                    })
                {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid spatial sketch NURBS",
                    );
                }
            }
            SpatialSketchGeometry::Native { native_kind } => {
                if native_kind.is_empty() {
                    finding(
                        findings,
                        Check::Counts,
                        id,
                        "empty native spatial sketch kind",
                    );
                }
            }
        }
    }

    let geometry = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    for constraint in &ir.model.sketch_constraints {
        if constraint
            .label_distance
            .iter()
            .chain(&constraint.label_position)
            .any(|value| !value.is_finite())
        {
            finding(
                findings,
                Check::Bounds,
                &constraint.id.0,
                "sketch constraint label placement is not finite",
            );
        }
        let valid = match &constraint.definition {
            Constraint::Coincident { entities } => entities.len() >= 2,
            Constraint::CoincidentLoci { loci } => loci.len() >= 2,
            Constraint::Distance { entities, .. } => !entities.is_empty(),
            Constraint::AtIntersection { first, second, .. } => first != second,
            Constraint::Group { elements } | Constraint::Text { elements, .. } => {
                !elements.is_empty()
            }
            Constraint::Native {
                native_kind,
                entities,
                operands,
                ..
            } => {
                !native_kind.is_empty()
                    && (!entities.is_empty()
                        || !operands.is_empty()
                        || constraint.native_ref.is_some())
            }
            _ => true,
        };
        if !valid {
            finding(
                findings,
                Check::Counts,
                &constraint.id.0,
                "invalid sketch constraint arity",
            );
        }
        if let Constraint::ArcAngle { entity, angle } = &constraint.definition {
            let valid_angle =
                angle.0.is_finite() && angle.0 > 0.0 && angle.0 <= std::f64::consts::TAU;
            if !valid_angle {
                finding(
                    findings,
                    Check::ParameterDomain,
                    &constraint.id.0,
                    "invalid sketch arc angle",
                );
            }
            match geometry.get(entity) {
                Some(SketchGeometry::Arc {
                    start_angle,
                    end_angle,
                    ..
                }) if valid_angle && start_angle.0.is_finite() && end_angle.0.is_finite() => {
                    let raw = end_angle.0 - start_angle.0;
                    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                    if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
                        sweep = std::f64::consts::TAU;
                    }
                    if (sweep - angle.0).abs() > 1.0e-9 {
                        finding(
                            findings,
                            Check::GeometricConsistency,
                            &constraint.id.0,
                            "sketch arc angle does not match solved geometry",
                        );
                    }
                }
                Some(SketchGeometry::Arc { .. }) | None => {}
                Some(_) => finding(
                    findings,
                    Check::GeometricConsistency,
                    &constraint.id.0,
                    "sketch arc-angle constraint references a non-arc entity",
                ),
            }
        }
        if let Constraint::EllipseAngle { entity, angle } = &constraint.definition {
            let valid_angle =
                angle.0.is_finite() && angle.0 > 0.0 && angle.0 <= std::f64::consts::TAU;
            if !valid_angle {
                finding(
                    findings,
                    Check::ParameterDomain,
                    &constraint.id.0,
                    "invalid sketch ellipse angle",
                );
            }
            match geometry.get(entity) {
                Some(SketchGeometry::Ellipse {
                    start_angle: Some(start),
                    end_angle: Some(end),
                    ..
                }) if valid_angle && start.0.is_finite() && end.0.is_finite() => {
                    let raw = end.0 - start.0;
                    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                    if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
                        sweep = std::f64::consts::TAU;
                    }
                    if (sweep - angle.0).abs() > 1.0e-9 {
                        finding(
                            findings,
                            Check::GeometricConsistency,
                            &constraint.id.0,
                            "sketch ellipse angle does not match solved geometry",
                        );
                    }
                }
                Some(SketchGeometry::Ellipse { .. }) | None => {}
                Some(_) => finding(
                    findings,
                    Check::GeometricConsistency,
                    &constraint.id.0,
                    "sketch ellipse-angle constraint references a non-ellipse entity",
                ),
            }
        }
        if let Constraint::Coradial { first, second } = &constraint.definition {
            let circular = |entity| match geometry.get(entity) {
                Some(
                    SketchGeometry::Circle { center, radius }
                    | SketchGeometry::Arc { center, radius, .. },
                ) => Some((*center, radius.0)),
                _ => None,
            };
            match (circular(first), circular(second)) {
                (Some((first_center, first_radius)), Some((second_center, second_radius))) => {
                    let scale = 1.0
                        + first_radius
                            .abs()
                            .max(second_radius.abs())
                            .max(first_center.u.abs())
                            .max(first_center.v.abs())
                            .max(second_center.u.abs())
                            .max(second_center.v.abs());
                    if distance2(first_center, second_center) > 1.0e-9 * scale
                        || (first_radius - second_radius).abs() > 1.0e-9 * scale
                    {
                        finding(
                            findings,
                            Check::GeometricConsistency,
                            &constraint.id.0,
                            "sketch coradial constraint does not match solved geometry",
                        );
                    }
                }
                (None, _) | (_, None) => finding(
                    findings,
                    Check::GeometricConsistency,
                    &constraint.id.0,
                    "sketch coradial constraint references non-circular geometry",
                ),
            }
        }
        if let Constraint::PointOnObject { point: _, entity } = &constraint.definition {
            if geometry
                .get(entity)
                .is_some_and(|geometry| matches!(geometry, SketchGeometry::Point { .. }))
            {
                finding(
                    findings,
                    Check::GeometricConsistency,
                    &constraint.id.0,
                    "point-on-object support is itself a point",
                );
            }
        }
        for locus in constraint_loci(&constraint.definition) {
            let Some(entity_geometry) = geometry.get(locus_entity(locus)) else {
                continue;
            };
            let valid = match locus {
                SketchLocus::Entity(_) => true,
                SketchLocus::Start(_) | SketchLocus::End(_) => !matches!(
                    entity_geometry,
                    SketchGeometry::Point { .. } | SketchGeometry::Circle { .. }
                ),
                SketchLocus::Center(_) => matches!(
                    entity_geometry,
                    SketchGeometry::Circle { .. }
                        | SketchGeometry::Arc { .. }
                        | SketchGeometry::Ellipse { .. }
                ),
            };
            if !valid {
                finding(
                    findings,
                    Check::GeometricConsistency,
                    &constraint.id.0,
                    "sketch constraint locus is incompatible with its entity",
                );
            }
        }
    }
}

fn invalid_optional_parameter_pair(start: Option<f64>, end: Option<f64>) -> bool {
    start.is_some() != end.is_some() || start.into_iter().chain(end).any(|value| !value.is_finite())
}

fn distance2(left: crate::math::Point2, right: crate::math::Point2) -> f64 {
    (left.u - right.u).hypot(left.v - right.v)
}

fn oriented_endpoints(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<(crate::math::Point2, crate::math::Point2)> {
    let endpoints = match geometry {
        SketchGeometry::Line { start, end } => (*start, *end),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (
            circular_point(*center, radius.0, start_angle.0),
            circular_point(*center, radius.0, end_angle.0),
        ),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle: Some(start),
            end_angle: Some(end),
        } => (
            ellipse_point(
                *center,
                major_angle.0,
                major_radius.0,
                minor_radius.0,
                start.0,
            ),
            ellipse_point(
                *center,
                major_angle.0,
                major_radius.0,
                minor_radius.0,
                end.0,
            ),
        ),
        SketchGeometry::Nurbs {
            control_points,
            periodic: false,
            ..
        } if control_points.len() >= 2 => {
            (control_points[0], control_points[control_points.len() - 1])
        }
        _ => return None,
    };
    Some(if reversed {
        (endpoints.1, endpoints.0)
    } else {
        endpoints
    })
}

fn circular_point(center: crate::math::Point2, radius: f64, angle: f64) -> crate::math::Point2 {
    crate::math::Point2::new(
        center.u + radius * angle.cos(),
        center.v + radius * angle.sin(),
    )
}

fn ellipse_point(
    center: crate::math::Point2,
    angle: f64,
    major: f64,
    minor: f64,
    parameter: f64,
) -> crate::math::Point2 {
    crate::math::Point2::new(
        center.u + angle.cos() * major * parameter.cos() - angle.sin() * minor * parameter.sin(),
        center.v + angle.sin() * major * parameter.cos() + angle.cos() * minor * parameter.sin(),
    )
}

fn locus_entity(locus: &SketchLocus) -> &crate::sketches::SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

fn constraint_loci(definition: &Constraint) -> Vec<&SketchLocus> {
    match definition {
        Constraint::CoincidentLoci { loci } => loci.iter().collect(),
        Constraint::HorizontalPoints { first, second }
        | Constraint::VerticalPoints { first, second } => vec![first, second],
        Constraint::Midpoint { point, .. }
        | Constraint::AtIntersection { point, .. }
        | Constraint::PointOnObject { point, .. } => vec![point],
        Constraint::Symmetric { first, second, .. } => vec![first, second],
        Constraint::DistanceLoci { first, second, .. }
        | Constraint::HorizontalDistance { first, second, .. }
        | Constraint::VerticalDistance { first, second, .. } => vec![first, second],
        Constraint::SnellsLaw {
            incident,
            refracted,
            ..
        } => vec![incident, refracted],
        Constraint::Group { elements } | Constraint::Text { elements, .. } => {
            elements.iter().collect()
        }
        _ => Vec::new(),
    }
}
