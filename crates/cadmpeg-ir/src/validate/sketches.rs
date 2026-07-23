// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for sketches.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::sketches::{
    SketchConstraintDefinition as Constraint, SketchGeometry, SketchLocus,
    SpatialSketchConstraintDefinition as SpatialConstraint, SpatialSketchGeometry,
};
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

fn finite3(point: crate::math::Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn valid_spatial_circle_frame(
    normal: crate::math::Vector3,
    reference: crate::math::Vector3,
) -> bool {
    let normal_length = normal.norm();
    let reference_length = reference.norm();
    normal_length.is_finite()
        && reference_length.is_finite()
        && (normal_length - 1.0).abs() <= 1.0e-9
        && (reference_length - 1.0).abs() <= 1.0e-9
        && (normal.x * reference.x + normal.y * reference.y + normal.z * reference.z).abs()
            <= 1.0e-9
}

fn spatial_oriented_endpoints(
    geometry: &SpatialSketchGeometry,
    reversed: bool,
) -> Option<(crate::math::Point3, crate::math::Point3)> {
    let endpoints = match geometry {
        SpatialSketchGeometry::Line { start, end } => (*start, *end),
        SpatialSketchGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            let transverse = crate::math::Vector3::new(
                normal.y * reference_direction.z - normal.z * reference_direction.y,
                normal.z * reference_direction.x - normal.x * reference_direction.z,
                normal.x * reference_direction.y - normal.y * reference_direction.x,
            );
            let at = |angle: f64| {
                crate::math::Point3::new(
                    center.x
                        + radius.0
                            * (reference_direction.x * angle.cos() + transverse.x * angle.sin()),
                    center.y
                        + radius.0
                            * (reference_direction.y * angle.cos() + transverse.y * angle.sin()),
                    center.z
                        + radius.0
                            * (reference_direction.z * angle.cos() + transverse.z * angle.sin()),
                )
            };
            (at(start_angle.0), at(end_angle.0))
        }
        SpatialSketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } => {
            let degree_index = usize::try_from(*degree).ok()?;
            let start = *knots.get(degree_index)?;
            let end = *knots.get(knots.len().checked_sub(degree_index + 1)?)?;
            (
                crate::eval::nurbs_curve_point(
                    *degree,
                    knots,
                    control_points,
                    weights.as_deref(),
                    start,
                )?,
                crate::eval::nurbs_curve_point(
                    *degree,
                    knots,
                    control_points,
                    weights.as_deref(),
                    end,
                )?,
            )
        }
        _ => return None,
    };
    Some(if reversed {
        (endpoints.1, endpoints.0)
    } else {
        endpoints
    })
}

fn line_offset_matches(source: &SketchGeometry, result: &SketchGeometry, expected: f64) -> bool {
    let (
        SketchGeometry::Line {
            start: source_start,
            end: source_end,
        },
        SketchGeometry::Line {
            start: result_start,
            end: result_end,
        },
    ) = (source, result)
    else {
        return false;
    };
    let source_du = source_end.u - source_start.u;
    let source_dv = source_end.v - source_start.v;
    let result_du = result_end.u - result_start.u;
    let result_dv = result_end.v - result_start.v;
    let source_length = source_du.hypot(source_dv);
    let result_length = result_du.hypot(result_dv);
    if source_length <= 1.0e-12 || result_length <= 1.0e-12 {
        return false;
    }
    let scale = 1.0 + expected.abs();
    let parallel = (source_du * result_dv - source_dv * result_du).abs()
        <= 1.0e-9 * source_length * result_length;
    let normal_u = -source_dv / source_length;
    let normal_v = source_du / source_length;
    let distance_at = |point: &crate::math::Point2| {
        (point.u - source_start.u) * normal_u + (point.v - source_start.v) * normal_v
    };
    parallel
        && (distance_at(result_start) - expected).abs() <= 1.0e-9 * scale
        && (distance_at(result_end) - expected).abs() <= 1.0e-9 * scale
}

fn spatial_parallel_line_distance(
    first: &SpatialSketchGeometry,
    second: &SpatialSketchGeometry,
) -> Option<f64> {
    let (
        SpatialSketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SpatialSketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first, second)
    else {
        return None;
    };
    let first_direction = crate::math::Vector3::new(
        first_end.x - first_start.x,
        first_end.y - first_start.y,
        first_end.z - first_start.z,
    );
    let second_direction = crate::math::Vector3::new(
        second_end.x - second_start.x,
        second_end.y - second_start.y,
        second_end.z - second_start.z,
    );
    let first_length = first_direction.norm();
    let second_length = second_direction.norm();
    let cross = crate::math::Vector3::new(
        first_direction.y * second_direction.z - first_direction.z * second_direction.y,
        first_direction.z * second_direction.x - first_direction.x * second_direction.z,
        first_direction.x * second_direction.y - first_direction.y * second_direction.x,
    );
    if first_length <= 1.0e-12
        || second_length <= 1.0e-12
        || cross.norm() > 1.0e-9 * first_length * second_length
    {
        return None;
    }
    let offset = crate::math::Vector3::new(
        second_start.x - first_start.x,
        second_start.y - first_start.y,
        second_start.z - first_start.z,
    );
    Some(
        crate::math::Vector3::new(
            offset.y * first_direction.z - offset.z * first_direction.y,
            offset.z * first_direction.x - offset.x * first_direction.z,
            offset.x * first_direction.y - offset.y * first_direction.x,
        )
        .norm()
            / first_length,
    )
}

pub(super) fn check_sketches(ir: &CadIr, index: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    let entity_geometry = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    for sketch in &ir.model.sketches {
        let Some((origin, normal_axis, u_axis)) = sketch.resolved_placement() else {
            continue;
        };
        let normal = normal_axis.norm();
        let u_norm = u_axis.norm();
        let dot = normal_axis.x * u_axis.x + normal_axis.y * u_axis.y + normal_axis.z * u_axis.z;
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
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
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
            SketchGeometry::Text {
                text,
                font_family,
                height,
                width_factor,
            } => {
                if text.is_empty()
                    || font_family.is_empty()
                    || nonpositive(height.0)
                    || nonpositive(*width_factor)
                {
                    finding(findings, Check::Bounds, id, "invalid sketch text");
                }
            }
            SketchGeometry::Native { native_kind } => {
                if native_kind.is_empty() {
                    finding(findings, Check::Counts, id, "empty native sketch kind");
                }
            }
        }
    }

    let spatial_geometry = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (&entity.id, (&entity.sketch, &entity.geometry)))
        .collect::<HashMap<_, _>>();
    for sketch in &ir.model.spatial_sketches {
        for profile in &sketch.profiles {
            let normal_length = profile.normal.norm();
            let u_length = profile.u_axis.norm();
            let dot = profile.normal.x * profile.u_axis.x
                + profile.normal.y * profile.u_axis.y
                + profile.normal.z * profile.u_axis.z;
            if !finite3(profile.origin)
                || (normal_length - 1.0).abs() > 1.0e-9
                || (u_length - 1.0).abs() > 1.0e-9
                || dot.abs() > 1.0e-9
            {
                finding(
                    findings,
                    Check::GeometricConsistency,
                    &sketch.id.0,
                    "invalid spatial sketch profile plane",
                );
            }
            let unique = profile
                .boundary
                .iter()
                .map(|use_| &use_.entity)
                .collect::<HashSet<_>>();
            if profile.boundary.is_empty() || unique.len() != profile.boundary.len() {
                finding(
                    findings,
                    Check::Counts,
                    &sketch.id.0,
                    "spatial sketch profile boundary is empty or repeats an entity",
                );
            }
            for use_ in &profile.boundary {
                if spatial_geometry.get(&use_.entity).map(|(owner, _)| *owner) != Some(&sketch.id) {
                    finding(
                        findings,
                        Check::ReferentialIntegrity,
                        &sketch.id.0,
                        "spatial sketch profile entity does not belong to its sketch",
                    );
                }
            }
            if profile.boundary.len() == 1 {
                if !matches!(
                    spatial_geometry.get(&profile.boundary[0].entity),
                    Some((_, SpatialSketchGeometry::Circle { .. }))
                ) {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &sketch.id.0,
                        "single-entity spatial sketch profile is not a full circle",
                    );
                }
            } else {
                for index in 0..profile.boundary.len() {
                    let left = &profile.boundary[index];
                    let right = &profile.boundary[(index + 1) % profile.boundary.len()];
                    let endpoints = spatial_geometry
                        .get(&left.entity)
                        .and_then(|(_, geometry)| {
                            spatial_oriented_endpoints(geometry, left.reversed)
                        })
                        .zip(
                            spatial_geometry
                                .get(&right.entity)
                                .and_then(|(_, geometry)| {
                                    spatial_oriented_endpoints(geometry, right.reversed)
                                }),
                        );
                    if endpoints.is_some_and(|(left, right)| {
                        (left.1.x - right.0.x)
                            .hypot(left.1.y - right.0.y)
                            .hypot(left.1.z - right.0.z)
                            > 1.0e-9
                    }) {
                        finding(
                            findings,
                            Check::GeometricConsistency,
                            &sketch.id.0,
                            "spatial sketch profile has disconnected consecutive entities",
                        );
                    }
                }
            }
        }
    }
    for entity in &ir.model.spatial_sketch_entities {
        let id = &entity.id.0;
        if !index
            .spatial_sketches
            .contains_key(entity.sketch.0.as_str())
        {
            finding(
                findings,
                Check::ReferentialIntegrity,
                id,
                "spatial sketch entity references a missing spatial sketch",
            );
        }
        match &entity.geometry {
            SpatialSketchGeometry::Point { position } => {
                if !finite3(*position) {
                    finding(
                        findings,
                        Check::Bounds,
                        id,
                        "non-finite spatial sketch point",
                    );
                }
            }
            SpatialSketchGeometry::Line { start, end } => {
                let distance = (end.x - start.x)
                    .hypot(end.y - start.y)
                    .hypot(end.z - start.z);
                if !finite3(*start) || !finite3(*end) || distance <= 1.0e-12 {
                    finding(findings, Check::Bounds, id, "invalid spatial sketch line");
                }
            }
            SpatialSketchGeometry::Circle {
                center,
                normal,
                reference_direction,
                radius,
            }
            | SpatialSketchGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                ..
            } => {
                if !finite3(*center)
                    || nonpositive(radius.0)
                    || !valid_spatial_circle_frame(*normal, *reference_direction)
                {
                    finding(
                        findings,
                        Check::Bounds,
                        id,
                        "invalid spatial circular sketch geometry",
                    );
                }
                if let SpatialSketchGeometry::Arc {
                    start_angle,
                    end_angle,
                    ..
                } = &entity.geometry
                {
                    if !start_angle.0.is_finite()
                        || !end_angle.0.is_finite()
                        || start_angle == end_angle
                    {
                        finding(
                            findings,
                            Check::ParameterDomain,
                            id,
                            "invalid spatial sketch arc interval",
                        );
                    }
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
            SpatialSketchGeometry::NurbsSurface {
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                control_points,
            } => {
                let u_count = control_points.len();
                let v_count = control_points.first().map_or(0, Vec::len);
                let expected_u_knots = usize::try_from(*u_degree)
                    .ok()
                    .and_then(|degree| u_count.checked_add(degree)?.checked_add(1));
                let expected_v_knots = usize::try_from(*v_degree)
                    .ok()
                    .and_then(|degree| v_count.checked_add(degree)?.checked_add(1));
                if *u_degree == 0
                    || *v_degree == 0
                    || u_count <= *u_degree as usize
                    || v_count <= *v_degree as usize
                    || control_points.iter().any(|row| row.len() != v_count)
                    || expected_u_knots != Some(u_knots.len())
                    || expected_v_knots != Some(v_knots.len())
                    || u_knots.iter().any(|value| !value.is_finite())
                    || v_knots.iter().any(|value| !value.is_finite())
                    || u_knots.windows(2).any(|pair| pair[0] > pair[1])
                    || v_knots.windows(2).any(|pair| pair[0] > pair[1])
                    || control_points
                        .iter()
                        .flatten()
                        .any(|point| !finite3(*point))
                {
                    finding(
                        findings,
                        Check::ParameterDomain,
                        id,
                        "invalid spatial sketch NURBS surface",
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

    let spatial_entities = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (entity.id.clone(), entity.sketch.clone()))
        .collect::<HashMap<_, _>>();
    let spatial_geometry = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    let parameter_values = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, &parameter.value))
        .collect::<HashMap<_, _>>();
    for constraint in &ir.model.spatial_sketch_constraints {
        if !index
            .spatial_sketches
            .contains_key(constraint.sketch.0.as_str())
        {
            finding(
                findings,
                Check::ReferentialIntegrity,
                &constraint.id.0,
                "spatial constraint references a missing spatial sketch",
            );
        }
        let entities = match &constraint.definition {
            SpatialConstraint::Native { .. } => Vec::new(),
            SpatialConstraint::SplineGroup { entities } => entities.clone(),
            SpatialConstraint::Coincident { first, second }
            | SpatialConstraint::Tangent { first, second }
            | SpatialConstraint::ParallelLineDistance { first, second, .. } => {
                vec![first.clone(), second.clone()]
            }
            SpatialConstraint::Midpoint { point, entity } => {
                vec![point.clone(), entity.clone()]
            }
            SpatialConstraint::PointOnSurface { point, surface } => {
                vec![point.clone(), surface.clone()]
            }
            SpatialConstraint::ParallelToDirection { entity, .. } => vec![entity.clone()],
        };
        let distinct = entities.iter().collect::<HashSet<_>>();
        let valid_arity = match &constraint.definition {
            SpatialConstraint::Native { .. } => true,
            SpatialConstraint::ParallelToDirection { .. } => entities.len() == 1,
            _ => entities.len() >= 2,
        };
        if !valid_arity || distinct.len() != entities.len() {
            finding(
                findings,
                Check::Counts,
                &constraint.id.0,
                "invalid spatial constraint arity",
            );
        }
        for entity in &entities {
            if spatial_entities.get(entity) != Some(&constraint.sketch) {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial constraint member does not belong to its sketch",
                );
            }
        }
        match &constraint.definition {
            SpatialConstraint::Native { .. } => {}
            SpatialConstraint::Coincident { first, second }
                if !matches!(
                    spatial_geometry.get(first),
                    Some(SpatialSketchGeometry::Point { .. })
                ) || !matches!(
                    spatial_geometry.get(second),
                    Some(SpatialSketchGeometry::Point { .. })
                ) =>
            {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial coincidence requires two points",
                );
            }
            SpatialConstraint::Midpoint { point, entity }
                if !matches!(
                    spatial_geometry.get(point),
                    Some(SpatialSketchGeometry::Point { .. })
                ) || !matches!(
                    spatial_geometry.get(entity),
                    Some(SpatialSketchGeometry::Line { .. })
                ) =>
            {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial midpoint requires a point and line",
                );
            }
            SpatialConstraint::PointOnSurface { point, surface }
                if !matches!(
                    spatial_geometry.get(point),
                    Some(SpatialSketchGeometry::Point { .. })
                ) || !matches!(
                    spatial_geometry.get(surface),
                    Some(SpatialSketchGeometry::NurbsSurface { .. })
                ) =>
            {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial point-on-surface requires a point and surface",
                );
            }
            SpatialConstraint::Tangent { first, second }
                if !matches!(
                    spatial_geometry.get(first),
                    Some(
                        SpatialSketchGeometry::Line { .. }
                            | SpatialSketchGeometry::Circle { .. }
                            | SpatialSketchGeometry::Arc { .. }
                            | SpatialSketchGeometry::Nurbs { .. }
                    )
                ) || !matches!(
                    spatial_geometry.get(second),
                    Some(
                        SpatialSketchGeometry::Line { .. }
                            | SpatialSketchGeometry::Circle { .. }
                            | SpatialSketchGeometry::Arc { .. }
                            | SpatialSketchGeometry::Nurbs { .. }
                    )
                ) =>
            {
                finding(
                    findings,
                    Check::ReferentialIntegrity,
                    &constraint.id.0,
                    "spatial tangent requires two curves",
                );
            }
            SpatialConstraint::ParallelLineDistance {
                first,
                second,
                parameter,
            } => {
                let measured = spatial_geometry.get(first).and_then(|first| {
                    spatial_geometry
                        .get(second)
                        .and_then(|second| spatial_parallel_line_distance(first, second))
                });
                let expected = match parameter_values.get(parameter) {
                    Some(Some(crate::features::ParameterValue::Length(length))) => {
                        Some(length.0.abs())
                    }
                    _ => None,
                };
                let matches = measured.zip(expected).is_some_and(|(measured, expected)| {
                    let scale = 1.0 + measured.max(expected);
                    (measured - expected).abs() <= 1.0e-9 * scale
                });
                if !matches {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial distance requires parallel lines separated by its length parameter",
                    );
                }
            }
            SpatialConstraint::ParallelToDirection { entity, direction } => {
                let direction_norm = direction.norm();
                let Some(SpatialSketchGeometry::Line { start, end }) = spatial_geometry.get(entity)
                else {
                    finding(
                        findings,
                        Check::ReferentialIntegrity,
                        &constraint.id.0,
                        "spatial directional constraint requires a line",
                    );
                    continue;
                };
                let line =
                    crate::math::Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
                let line_norm = line.norm();
                let cross = crate::math::Vector3::new(
                    line.y * direction.z - line.z * direction.y,
                    line.z * direction.x - line.x * direction.z,
                    line.x * direction.y - line.y * direction.x,
                );
                if !direction_norm.is_finite()
                    || (direction_norm - 1.0).abs() > 1.0e-9
                    || !line_norm.is_finite()
                    || line_norm <= 1.0e-12
                    || cross.norm() > 1.0e-9 * line_norm
                {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "spatial line is not parallel to its constraint direction",
                    );
                }
            }
            _ => {}
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
            Constraint::SplineGroup { entities } => entities.len() >= 2,
            Constraint::RectangularPattern {
                directions,
                instances,
            } => {
                let expected_instances =
                    directions.iter().try_fold(1usize, |product, direction| {
                        product.checked_mul(usize::try_from(direction.count).ok()?)
                    });
                let seed_arity = instances
                    .first()
                    .map_or(0, |instance| instance.entities.len());
                let mut indices = HashSet::new();
                let mut entities = HashSet::new();
                let dot = directions[0].direction[0] * directions[1].direction[0]
                    + directions[0].direction[1] * directions[1].direction[1];
                expected_instances == Some(instances.len())
                    && seed_arity > 0
                    && dot.abs() <= 1.0e-9
                    && instances
                        .first()
                        .is_some_and(|instance| instance.indices == [0, 0])
                    && directions.iter().all(|direction| {
                        let length = direction.direction[0].hypot(direction.direction[1]);
                        direction.count > 0
                            && direction.spacing.0.is_finite()
                            && direction.direction.iter().all(|value| value.is_finite())
                            && (length - 1.0).abs() <= 1.0e-9
                    })
                    && instances.iter().all(|instance| {
                        instance.indices[0] < directions[0].count
                            && instance.indices[1] < directions[1].count
                            && instance.entities.len() == seed_arity
                            && indices.insert(instance.indices)
                            && instance
                                .entities
                                .iter()
                                .all(|entity| entities.insert(entity))
                    })
            }
            Constraint::CircularPattern {
                center,
                angle,
                count,
                instances,
                ..
            } => {
                let seed_arity = instances
                    .first()
                    .map_or(0, |instance| instance.entities.len());
                let mut indices = HashSet::new();
                let mut entities = HashSet::new();
                *count > 0
                    && angle.0.is_finite()
                    && seed_arity > 0
                    && instances.len() == usize::try_from(*count).unwrap_or(usize::MAX)
                    && instances
                        .first()
                        .is_some_and(|instance| instance.index == 0 && instance.angle.0 == 0.0)
                    && !instances
                        .iter()
                        .flat_map(|instance| &instance.entities)
                        .any(|entity| entity == center)
                    && instances.iter().all(|instance| {
                        instance.index < *count
                            && instance.angle.0.is_finite()
                            && instance.entities.len() == seed_arity
                            && indices.insert(instance.index)
                            && instance
                                .entities
                                .iter()
                                .all(|entity| entities.insert(entity))
                    })
            }
            Constraint::TextFrame { text, frame } => {
                matches!(geometry.get(text), Some(SketchGeometry::Text { .. }))
                    && !frame.is_empty()
                    && frame.iter().all(|entity| {
                        entity != text
                            && geometry.get(entity).is_some_and(|geometry| {
                                !matches!(geometry, SketchGeometry::Text { .. })
                            })
                    })
            }
            Constraint::TextPath {
                text,
                path,
                glyph_transforms,
            } => {
                matches!(geometry.get(text), Some(SketchGeometry::Text { .. }))
                    && text != path
                    && geometry.get(path).is_some_and(|geometry| {
                        !matches!(
                            geometry,
                            SketchGeometry::Point { .. } | SketchGeometry::Text { .. }
                        )
                    })
                    && !glyph_transforms.is_empty()
                    && glyph_transforms.iter().all(|transform| {
                        transform
                            .rows
                            .iter()
                            .flatten()
                            .all(|value| value.is_finite())
                            && transform.rows[3] == [0.0, 0.0, 0.0, 1.0]
                    })
            }
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
                distance,
                parameter,
                parameter_factor,
            } => {
                let mut sources = HashSet::new();
                let mut results = HashSet::new();
                let valid_parameter = match (parameter, parameter_factor) {
                    (None, None) => true,
                    (Some(_), Some(factor)) => factor.abs() == 1.0,
                    _ => false,
                };
                !pairs.is_empty()
                    && pairs.iter().all(|pair| {
                        pair.source != pair.result
                            && sources.insert(&pair.source)
                            && results.insert(&pair.result)
                    })
                    && distance.0.is_finite()
                    && distance.0 > 0.0
                    && valid_parameter
            }
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
                    && (!entities.is_empty() || !operands.is_empty())
                    && operands.iter().all(|operand| {
                        !operand.native_kind.is_empty()
                            && operand
                                .native_field
                                .as_ref()
                                .is_none_or(|field| !field.is_empty())
                            && (operand.native_role.is_none() || operand.native_field.is_some())
                    })
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
        if let Constraint::Offset {
            pairs, distance, ..
        } = &constraint.definition
        {
            for pair in pairs {
                let valid = entity_geometry
                    .get(&pair.source)
                    .zip(entity_geometry.get(&pair.result))
                    .is_none_or(|(source, result)| {
                        let expected = if pair.source_reversed {
                            -distance.0
                        } else {
                            distance.0
                        };
                        line_offset_matches(source, result, expected)
                    });
                if !valid {
                    finding(
                        findings,
                        Check::GeometricConsistency,
                        &constraint.id.0,
                        "sketch offset pair does not match its oriented distance",
                    );
                }
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
        Constraint::Midpoint { point, .. } | Constraint::PointOnObject { point, .. } => vec![point],
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
