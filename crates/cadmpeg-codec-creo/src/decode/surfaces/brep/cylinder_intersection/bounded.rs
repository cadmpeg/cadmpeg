// SPDX-License-Identifier: Apache-2.0
//! Equal-cylinder arcs selected by complete source parameter bounds.

use crate::surface::PositionalCylinderFrame;
use crate::vecmath::{add, cross, dot, normalize, scale};
use cadmpeg_ir::geometry::{
    analytic::CylinderSurface, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};

use super::{
    agrees, equal_cylinder_curve, projected_pcurve, subtract, EPS_AXIS, EPS_PARAMETER, EPS_POSITION,
};

fn matching_frame(frame: PositionalCylinderFrame, cylinder: &CylinderSurface) -> bool {
    let stored = frame.frame();
    let origin: [f64; 3] = cylinder.origin().get().into();
    let axis: [f64; 3] = (*cylinder.frame().axis().as_raw()).into();
    let reference: [f64; 3] = (*cylinder.frame().reference().as_raw()).into();
    agrees(stored.origin(), origin)
        && dot(stored.axis(), axis) >= 1.0 - EPS_AXIS
        && dot(stored.ref_direction(), reference) >= 1.0 - EPS_AXIS
        && (frame.radius().get() - cylinder.radius().get()).abs()
            <= EPS_POSITION * cylinder.radius().get().max(1.0)
}

fn angle_in_bounds(angle: f64, bounds: [f64; 2]) -> bool {
    let span = bounds[1] - bounds[0];
    if !(span > EPS_PARAMETER && span < std::f64::consts::PI - EPS_PARAMETER) {
        return false;
    }
    let delta = (angle - bounds[0]).rem_euclid(std::f64::consts::TAU);
    delta <= span + EPS_AXIS || std::f64::consts::TAU - delta <= EPS_AXIS
}

fn critical_in_arc(angle: f64, start: f64, sweep: f64) -> bool {
    let delta = if sweep > 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU)
    };
    delta <= sweep.abs() + EPS_PARAMETER
}

fn endpoints_in_angular_bounds(
    cylinder: &CylinderSurface,
    bounds: [[f64; 2]; 2],
    endpoints: [[f64; 3]; 2],
) -> bool {
    let origin: [f64; 3] = cylinder.origin().get().into();
    let axis: [f64; 3] = (*cylinder.frame().axis().as_raw()).into();
    let reference: [f64; 3] = (*cylinder.frame().reference().as_raw()).into();
    let transverse = cross(axis, reference);
    let radius = cylinder.radius().get();
    endpoints.into_iter().all(|point| {
        let delta = subtract(point, origin);
        let x = dot(delta, reference);
        let y = dot(delta, transverse);
        (x.hypot(y) - radius).abs() <= EPS_POSITION * radius.max(1.0)
            && angle_in_bounds(y.atan2(x), bounds[0])
    })
}

struct EllipseArc {
    center: [f64; 3],
    radial: [f64; 3],
    normal: [f64; 3],
    radius: f64,
    start: f64,
    sweep: f64,
}

fn axial_arc_in_bounds(
    cylinder: &CylinderSurface,
    bounds: [[f64; 2]; 2],
    arc: &EllipseArc,
) -> bool {
    let origin: [f64; 3] = cylinder.origin().get().into();
    let axis: [f64; 3] = (*cylinder.frame().axis().as_raw()).into();
    let base = dot(subtract(arc.center, origin), axis);
    let cosine = arc.radius * dot(arc.radial, axis);
    let sine = arc.radius * dot(arc.normal, axis);
    let critical = sine.atan2(cosine);
    let lower = bounds[1][0].min(bounds[1][1]);
    let upper = bounds[1][0].max(bounds[1][1]);
    let tolerance = EPS_POSITION * lower.abs().max(upper.abs()).max(arc.radius).max(1.0);
    [
        Some(arc.start),
        Some(arc.start + arc.sweep),
        critical_in_arc(critical, arc.start, arc.sweep).then_some(critical),
        critical_in_arc(critical + std::f64::consts::PI, arc.start, arc.sweep)
            .then_some(critical + std::f64::consts::PI),
    ]
    .into_iter()
    .flatten()
    .all(|angle| {
        let axial = base + cosine * angle.cos() + sine * angle.sin();
        axial.is_finite() && axial >= lower - tolerance && axial <= upper + tolerance
    })
}

pub(in crate::decode::surfaces::brep) fn carrier(
    surfaces: [&SurfaceGeometry; 2],
    frames: &[PositionalCylinderFrame; 2],
    endpoints: [[f64; 3]; 2],
) -> Option<CurveGeometry> {
    let [SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(first)), SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(second))] =
        surfaces
    else {
        return None;
    };
    let bounds = [frames[0].parameter_bounds()?, frames[1].parameter_bounds()?];
    for ((frame, cylinder), bounds) in frames.iter().copied().zip([first, second]).zip(bounds) {
        let matches = matching_frame(frame, cylinder);
        let in_bounds = endpoints_in_angular_bounds(cylinder, bounds, endpoints);
        if !matches || !in_bounds {
            return None;
        }
    }
    let radius = first.radius().get();
    let tolerance = EPS_POSITION * radius.max(1.0);
    ((radius - second.radius().get()).abs() <= tolerance).then_some(())?;
    let a: [f64; 3] = (*first.frame().axis().as_raw()).into();
    let b: [f64; 3] = (*second.frame().axis().as_raw()).into();
    let cosine = dot(a, b);
    let denominator = 1.0 - cosine * cosine;
    (denominator > EPS_AXIS).then_some(())?;
    let normal = normalize(cross(a, b))?;
    let first_origin: [f64; 3] = first.origin().get().into();
    let second_origin: [f64; 3] = second.origin().get().into();
    let displacement = subtract(second_origin, first_origin);
    (dot(displacement, normal).abs() <= tolerance).then_some(())?;
    let along = (dot(displacement, a) - cosine * dot(displacement, b)) / denominator;
    let center = add(first_origin, scale(a, along));
    let mut selected = None;
    for branch in [-1.0, 1.0] {
        let radial = scale(add(a, scale(b, branch)), 1.0 / denominator.sqrt());
        let radial_scale = dot(radial, a);
        let angles = endpoints.map(|point| {
            let delta = subtract(point, center);
            let first = dot(delta, a);
            let second = dot(delta, b);
            let normal_coordinate = dot(delta, normal);
            ((second - branch * first).abs() <= tolerance
                && ((first / radial_scale).hypot(normal_coordinate) - radius).abs() <= tolerance)
                .then_some(normal_coordinate.atan2(first / radial_scale))
        });
        let [Some(start), Some(end)] = angles else {
            continue;
        };
        let sweep = (end - start + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        if sweep.abs() <= EPS_PARAMETER || sweep.abs() >= std::f64::consts::PI - EPS_PARAMETER {
            continue;
        }
        let arc = EllipseArc {
            center,
            radial,
            normal,
            radius,
            start,
            sweep,
        };
        if ![first, second]
            .into_iter()
            .zip(bounds)
            .all(|(cylinder, bounds)| axial_arc_in_bounds(cylinder, bounds, &arc))
        {
            continue;
        }
        let mid = start + sweep / 2.0;
        let midpoint = add(
            center,
            scale(
                add(scale(radial, mid.cos()), scale(normal, mid.sin())),
                radius,
            ),
        );
        let Some(curve) = equal_cylinder_curve(
            first,
            second,
            &[endpoints[0], midpoint, endpoints[1]],
            endpoints,
        ) else {
            continue;
        };
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
        if surfaces
            .iter()
            .any(|surface| projected_pcurve(surface, &geometry).is_none())
        {
            continue;
        }
        if selected.replace(geometry).is_some() {
            return None;
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::math::{Point3, Vector3};

    #[test]
    fn angular_bounds_reject_an_ambiguous_half_turn() {
        let half = std::f64::consts::FRAC_PI_2;
        assert!(angle_in_bounds(-1.0e-14, [0.0, half]));
        assert!(angle_in_bounds(half, [0.0, half]));
        assert!(!angle_in_bounds(-0.1, [0.0, half]));
        assert!(!angle_in_bounds(0.5, [0.0, std::f64::consts::PI]));
    }

    #[test]
    fn axial_bounds_check_the_arc_interior() {
        let cylinder = CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("cylinder");
        let center = [0.0, 0.0, 0.2];
        let radial = [0.0, 0.0, 0.7];
        let arc = EllipseArc {
            center,
            radial,
            normal: [1.0, 0.0, 0.0],
            radius: 1.0,
            start: -std::f64::consts::FRAC_PI_2,
            sweep: std::f64::consts::PI,
        };
        assert!(axial_arc_in_bounds(
            &cylinder,
            [[0.0, 1.0], [0.0, 1.0]],
            &arc
        ));
        assert!(!axial_arc_in_bounds(
            &cylinder,
            [[0.0, 1.0], [0.0, 0.8]],
            &arc
        ));
    }
}
