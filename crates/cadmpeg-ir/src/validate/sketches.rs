// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for sketches.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::sketches::{SketchConstraintDefinition as Constraint, SketchGeometry, SketchLocus};
use std::collections::{HashMap, HashSet};

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

    let geometry = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    for constraint in &ir.model.sketch_constraints {
        let valid = match &constraint.definition {
            Constraint::Coincident { entities } => entities.len() >= 2,
            Constraint::CoincidentLoci { loci } => loci.len() >= 2,
            Constraint::Distance { entities, .. } => !entities.is_empty(),
            Constraint::RepeatedDistance { measurements, .. } => {
                let mut entities = HashSet::new();
                !measurements.is_empty()
                    && measurements.iter().all(|measurement| {
                        use crate::sketches::SketchDistanceMeasurement as Measurement;
                        let (first, second) = match measurement {
                            Measurement::Distance { first, second }
                            | Measurement::Horizontal { first, second }
                            | Measurement::Vertical { first, second } => (first, second),
                        };
                        let first = locus_entity(first);
                        let second = locus_entity(second);
                        first != second
                            && entities.insert(first.clone())
                            && entities.insert(second.clone())
                    })
            }
            Constraint::Offset {
                pairs,
                signed_distance,
                parameter,
                parameter_factor,
            } => {
                let valid_parameter = match (parameter, parameter_factor) {
                    (None, None) => true,
                    (Some(_), Some(factor)) => factor.abs() == 1.0,
                    _ => false,
                };
                !pairs.is_empty()
                    && signed_distance.0.is_finite()
                    && signed_distance.0.abs() > 0.0
                    && valid_parameter
            }
            Constraint::Native {
                native_kind,
                entities,
                operands,
                ..
            } => !native_kind.is_empty() && (!entities.is_empty() || !operands.is_empty()),
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
        if let Constraint::Offset { pairs, .. } = &constraint.definition {
            for pair in pairs {
                let valid = entity_geometry
                    .get(&pair.source)
                    .zip(entity_geometry.get(&pair.result))
                    .is_none_or(|(source, result)| {
                        matches!(source, SketchGeometry::Line { .. })
                            && matches!(result, SketchGeometry::Line { .. })
                    });
                if !valid {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "sketch offset pair requires two line entities",
                    );
                }
            }
        }
    }
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
        Constraint::Midpoint { point, .. } => vec![point],
        Constraint::Symmetric { first, second, .. } => vec![first, second],
        Constraint::DistanceLoci { first, second, .. }
        | Constraint::HorizontalDistance { first, second, .. }
        | Constraint::VerticalDistance { first, second, .. } => vec![first, second],
        Constraint::RepeatedDistance { measurements, .. } => measurements
            .iter()
            .flat_map(|measurement| {
                use crate::sketches::SketchDistanceMeasurement as Measurement;
                let (first, second) = match measurement {
                    Measurement::Distance { first, second }
                    | Measurement::Horizontal { first, second }
                    | Measurement::Vertical { first, second } => (first, second),
                };
                [first, second]
            })
            .collect(),
        _ => Vec::new(),
    }
}
