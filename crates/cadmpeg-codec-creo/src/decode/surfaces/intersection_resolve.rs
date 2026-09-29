// SPDX-License-Identifier: Apache-2.0
//! Multi-component intersection candidates and FC14 axis selection.

use crate::vecmath::unit_length;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

use crate::decode::analytic::edges::{
    nonperiodic_conic_parameter, periodic_conic_frame, PeriodicConicFrame,
};
use crate::decode::analytic::equations::{
    circle_parameters, intersect_plane_with_circle, CarrierEquation, PlaneEquation,
};
use crate::vecmath::{cross, dot};

use super::intersection_candidates::{
    apex_plane_cone_generator_candidates, axis_containing_plane_torus_circle_candidates,
    axis_normal_plane_torus_circle_candidates, coaxial_cone_cylinder_circle_candidates,
    coaxial_cone_sphere_circle_candidates, coaxial_cone_torus_circle_candidates,
    coaxial_cones_section_candidates, coaxial_cylinder_sphere_circle_candidates,
    coaxial_cylinder_torus_circle_candidates, coaxial_sphere_torus_circle_candidates,
    coaxial_tori_circle_candidates, parallel_cylinder_generator_candidates,
    parallel_plane_cylinder_generator_candidates,
};
use super::intersections::carrier_intersection_curve;

const EPS_ON_CURVE: f64 = 1.0e-7;
const EPS_AXIS_COMPONENT: f64 = 1.0e-10;
const EPS_CENTER_AGREEMENT: f64 = 1.0e-9;

pub(super) fn multi_component_intersection_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> impl Iterator<Item = (CurveGeometry, &'static str)> {
    parallel_plane_cylinder_generator_candidates(first, second)
        .into_iter()
        .chain(parallel_cylinder_generator_candidates(first, second))
        .chain(coaxial_cylinder_sphere_circle_candidates(first, second))
        .chain(coaxial_cone_cylinder_circle_candidates(first, second))
        .chain(coaxial_cones_section_candidates(first, second))
        .chain(apex_plane_cone_generator_candidates(first, second))
        .chain(coaxial_cone_sphere_circle_candidates(first, second))
        .chain(coaxial_cone_torus_circle_candidates(first, second))
        .chain(coaxial_cylinder_torus_circle_candidates(first, second))
        .chain(coaxial_sphere_torus_circle_candidates(first, second))
        .chain(coaxial_tori_circle_candidates(first, second))
        .chain(axis_normal_plane_torus_circle_candidates(first, second))
        .chain(axis_containing_plane_torus_circle_candidates(first, second))
}

fn carrier_intersection_components(
    ctx: &DecodeContext<'_>,
    first: CarrierEquation,
    second: CarrierEquation,
) -> Result<Vec<(CurveGeometry, &'static str)>, CodecError> {
    let mut components = Vec::new();
    for component in carrier_intersection_curve(first, second)
        .into_iter()
        .chain(multi_component_intersection_candidates(first, second))
    {
        ctx.try_reserve_items(&mut components, 1, "creo carrier intersection components")?;
        components.push(component);
    }
    Ok(components)
}

pub(in super::super) fn intersect_plane_with_carrier_components(
    ctx: &DecodeContext<'_>,
    plane: PlaneEquation,
    first: CarrierEquation,
    second: CarrierEquation,
) -> Result<Vec<[f64; 3]>, CodecError> {
    let mut intersections = Vec::new();
    for (geometry, _) in carrier_intersection_components(ctx, first, second)? {
        let Some((center, axis, radius)) = circle_parameters(&geometry) else {
            continue;
        };
        for point in intersect_plane_with_circle(ctx, plane, center, axis, radius)? {
            ctx.try_reserve_items(
                &mut intersections,
                1,
                "creo plane-carrier component intersections",
            )?;
            intersections.push(point);
        }
    }
    Ok(intersections)
}

pub(in super::super) fn curve_contains_points(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> bool {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let origin = [origin.x, origin.y, origin.z];
            let direction = unit_length(line_curve.direction());
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
                let residual = cross(relative, direction);
                // The origin can move along the same infinite carrier. Its
                // distance from the witness cannot enlarge the positional gate.
                let distance = residual[0].hypot(residual[1]).hypot(residual[2]);
                distance.is_finite() && distance <= EPS_ON_CURVE
            })
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)) => {
            let Some(PeriodicConicFrame {
                center,
                normal,
                x_axis,
                y_axis,
                radii,
            }) = periodic_conic_frame(geometry)
            else {
                return false;
            };
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - center[index]);
                let scale = radii.into_iter().fold(1.0, f64::max);
                let x = dot(relative, x_axis) / radii[0];
                let y = dot(relative, y_axis) / radii[1];
                dot(relative, normal).abs() <= EPS_ON_CURVE * scale
                    && x.mul_add(x, y * y).is_finite()
                    && (x.mul_add(x, y * y) - 1.0).abs() <= EPS_ON_CURVE
            })
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(_)) => points
            .into_iter()
            .all(|point| nonperiodic_conic_parameter(geometry, point).is_some()),
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(_)) => points
            .into_iter()
            .all(|point| nonperiodic_conic_parameter(geometry, point).is_some()),
        _ => false,
    }
}

pub(in super::super) fn select_unique_curve_candidate(
    candidates: impl IntoIterator<Item = (CurveGeometry, &'static str)>,
    points: [[f64; 3]; 2],
) -> Option<(CurveGeometry, &'static str)> {
    let mut candidates = candidates
        .into_iter()
        .filter(|(geometry, _)| curve_contains_points(geometry, points));
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(in super::super) fn resolve_curve_candidates(
    candidates: impl IntoIterator<Item = (CurveGeometry, &'static str)>,
    points: Option<[[f64; 3]; 2]>,
) -> Option<(CurveGeometry, &'static str)> {
    if let Some(points) = points {
        return select_unique_curve_candidate(candidates, points);
    }
    let mut candidates = candidates.into_iter();
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(in super::super) fn fc14_held_coordinate(
    coordinates: &[crate::curve::FcCurveCoordinates],
    curve_id: u32,
) -> Option<f64> {
    let mut records = coordinates
        .iter()
        .filter(|record| record.curve_id == curve_id && record.subtype == 0x14);
    let record = records.next()?;
    records.next().is_none().then_some(())?;
    let mut tokens = record
        .tokens
        .iter()
        .filter(|token| token.raw.first() == Some(&0x2d));
    let first = tokens.next()?;
    for _ in 0..3 {
        let token = tokens.next()?;
        (token.raw == first.raw && token.value_mm == first.value_mm).then_some(())?;
    }
    (first.value_mm.is_finite()
        && tokens.all(|token| token.raw == first.raw && token.value_mm == first.value_mm))
    .then_some(first.value_mm)
}

pub(in super::super) fn select_fc14_axis_coordinate_candidate(
    candidates: impl IntoIterator<Item = (CurveGeometry, &'static str)>,
    held_coordinate: f64,
) -> Option<(CurveGeometry, &'static str)> {
    let mut matching = candidates.into_iter().filter(|(geometry, tag)| {
        if *tag != "coaxial_cone_cylinder_secant_circle" {
            return false;
        }
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = geometry else {
            return false;
        };
        let center = circle_curve.center().get();
        let axis = circle_curve.frame().axis().as_raw();
        let axis = [axis.x, axis.y, axis.z];
        let Some(axis_index) = axis.iter().enumerate().find_map(|(index, value)| {
            ((value.abs() - 1.0).abs() <= EPS_AXIS_COMPONENT).then_some(index)
        }) else {
            return false;
        };
        if axis
            .iter()
            .enumerate()
            .any(|(index, value)| index != axis_index && value.abs() > EPS_AXIS_COMPONENT)
        {
            return false;
        }
        let center = [center.x, center.y, center.z];
        let scale = center[axis_index].abs().max(held_coordinate.abs()).max(1.0);
        (center[axis_index] - held_coordinate).abs() <= EPS_CENTER_AGREEMENT * scale
    });
    let candidate = matching.next()?;
    matching.next().is_none().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::{curve_contains_points, CarrierEquation, CurveGeometry, SolvedCurveGeometry};
    use crate::decode::analytic::equations::{ConeEquation, PlaneEquation, SphereEquation};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn cone_sphere_circle_carriers() -> (CarrierEquation, CarrierEquation) {
        let cone = ConeEquation::new(
            [0.0; 3],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            2.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid test cone");
        let sphere = SphereEquation {
            center: [0.0; 3],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0_f64.sqrt(),
        };
        (CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
    }

    #[test]
    fn carrier_intersection_components_refuse_before_vec_growth() {
        let (cone, sphere) = cone_sphere_circle_carriers();
        let run = |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root fits the collection policy");
            super::carrier_intersection_components(&ctx, cone, sphere)
        };
        assert!(
            matches!(run(0), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo carrier intersection components")
        );
        assert!(!run(u64::MAX)
            .expect("service budget admits the circle")
            .is_empty());
    }

    #[test]
    fn plane_carrier_component_intersections_refuse_before_vec_growth() {
        let (cone, sphere) = cone_sphere_circle_carriers();
        let plane = PlaneEquation {
            origin: [0.0; 3],
            normal: [1.0, 0.0, 0.0],
        };
        let run = |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("empty root fits the collection policy");
            super::intersect_plane_with_carrier_components(&ctx, plane, cone, sphere)
        };
        let limit = (0..64)
            .find(|limit| {
                matches!(run(*limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo plane-carrier component intersections")
            })
            .expect("the carrier circle reaches the output boundary");
        assert!(
            matches!(run(limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo plane-carrier component intersections")
        );
        let points = run(u64::MAX).expect("service budget admits the circle cut");
        assert!(points.contains(&[0.0, -1.0, -1.0]));
        assert!(points.contains(&[0.0, 1.0, -1.0]));
    }

    #[test]
    fn audit_regression_line_membership_ignores_along_line_origin() {
        let line = |x| {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(x, 0., 0.),
                    Vector3::new(1., 0., 0.),
                )
                .expect("valid X-axis carrier"),
            ))
        };
        for origin in [0., 1e8] {
            assert!(!curve_contains_points(
                &line(origin),
                [[1e8, 1., 0.], [1e8 + 1., 1., 0.]]
            ));
            assert!(curve_contains_points(
                &line(origin),
                [[1e8, 0., 0.], [1e8 + 1., 0., 0.]]
            ));
        }
    }
}
