// SPDX-License-Identifier: Apache-2.0

use super::{helix_nurbs, helix_surface_nurbs};
use crate::eval::admission::EvaluationAdmission;
use crate::eval::decode::{nurbs_curve_point_at, nurbs_surface_point};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{
    HelixCircleProfile, HelixCurveConstruction, HelixFrame, HelixLineProfile,
    HelixPathConstruction, HelixSurfaceConstruction, HelixSurfaceProfile,
};
use crate::math::{Point3, Vector3};
use crate::scalar::PositiveLength;

const DENSE_SAMPLES: u32 = 997;

struct Helix {
    angle_range: [f64; 2],
    center: Point3,
    major: Vector3,
    minor: Vector3,
    pitch: Vector3,
    apex_factor: f64,
}

impl Helix {
    fn construction(&self) -> HelixCurveConstruction {
        HelixCurveConstruction::try_new(
            self.angle_range,
            HelixFrame {
                center: self.center,
                major: self.major,
                minor: self.minor,
                pitch: self.pitch,
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            self.apex_factor,
            None,
        )
        .expect("admitted helix")
    }

    /// `C(t) = center + s·(major·cos t + minor·sin t) + pitch·q`, with
    /// `q = t / 2π` and `s = 1 + apex_factor·q`.
    fn point(&self, parameter: f64) -> Point3 {
        let turns = parameter / std::f64::consts::TAU;
        let scale = 1.0 + self.apex_factor * turns;
        let (sin, cos) = parameter.sin_cos();
        Point3::new(
            self.center.x
                + scale * (self.major.x * cos + self.minor.x * sin)
                + self.pitch.x * turns,
            self.center.y
                + scale * (self.major.y * cos + self.minor.y * sin)
                + self.pitch.y * turns,
            self.center.z
                + scale * (self.major.z * cos + self.minor.z * sin)
                + self.pitch.z * turns,
        )
    }
}

fn thread() -> Helix {
    Helix {
        angle_range: [-6.0 * std::f64::consts::PI, -1.0],
        center: Point3::new(0.0, 0.0, -0.5),
        major: Vector3::new(10.0, 0.0, 0.0),
        minor: Vector3::new(0.0, -10.0, 0.0),
        pitch: Vector3::new(0.0, 0.0, -2.5),
        apex_factor: 0.0,
    }
}

fn cone() -> Helix {
    Helix {
        angle_range: [0.25, 9.0],
        center: Point3::new(1.0, -2.0, 3.0),
        major: Vector3::new(2.0, 0.0, 0.0),
        minor: Vector3::new(0.0, 2.0, 0.0),
        pitch: Vector3::new(0.0, 0.0, 3.0),
        apex_factor: 0.4,
    }
}

fn tolerance(value: f64) -> PositiveLength {
    PositiveLength::new(value).expect("positive tolerance")
}

fn distance(a: Point3, b: Point3) -> f64 {
    Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z).norm()
}

fn max_deviation(helix: &Helix, spline: &NurbsCurve) -> f64 {
    let ctx = cadmpeg_test_support::service_decode_context();
    let [start, end] = helix.angle_range;
    (0..=DENSE_SAMPLES)
        .map(|index| {
            let parameter = start + (end - start) * f64::from(index) / f64::from(DENSE_SAMPLES);
            let fitted = nurbs_curve_point_at(&ctx, spline, parameter)
                .expect("parameter inside the spline domain")
                .get();
            distance(fitted, helix.point(parameter))
        })
        .fold(0.0, f64::max)
}

#[test]
fn the_spline_stays_within_the_tolerance_over_the_whole_thread() {
    let helix = thread();
    let spline = helix_nurbs(&helix.construction(), tolerance(1.0e-6)).expect("fitted thread");

    assert_eq!(spline.degree(), 3);
    assert!(max_deviation(&helix, &spline) <= 1.0e-6);
}

#[test]
fn the_spline_stays_within_the_tolerance_on_a_conical_helix() {
    let helix = cone();
    let spline = helix_nurbs(&helix.construction(), tolerance(1.0e-4)).expect("fitted cone");

    assert!(max_deviation(&helix, &spline) <= 1.0e-4);
}

#[test]
fn the_spline_domain_and_ends_are_the_helix_angle_range_and_ends() {
    let helix = thread();
    let spline = helix_nurbs(&helix.construction(), tolerance(1.0e-3)).expect("fitted thread");
    let knots = spline.knots().as_slice().to_vec();
    let poles = spline.control_points();
    let [start, end] = helix.angle_range;

    assert_eq!(knots.first().copied(), Some(start));
    assert_eq!(knots.last().copied(), Some(end));
    assert!(distance(poles[0].get(), helix.point(start)) <= 1.0e-12);
    assert!(distance(poles[poles.len() - 1].get(), helix.point(end)) <= 1.0e-12);
}

#[test]
fn a_tighter_tolerance_takes_more_segments() {
    let helix = thread();
    let coarse = helix_nurbs(&helix.construction(), tolerance(1.0e-2)).expect("coarse fit");
    let fine = helix_nurbs(&helix.construction(), tolerance(1.0e-6)).expect("fine fit");

    assert!(fine.control_points().len() > coarse.control_points().len());
}

const SURFACE_SAMPLES: u32 = 97;

/// A thread flank: a line swept along a two-turn path.
fn flank() -> HelixSurfaceConstruction {
    HelixSurfaceConstruction::try_new(
        [0.0, 0.05],
        [0.0, 4.0 * std::f64::consts::PI],
        path(2.0, -1.0),
        HelixSurfaceProfile::Line(
            HelixLineProfile::try_new(Vector3::new(4.0, 0.0, 5.0)).expect("admitted line profile"),
        ),
    )
    .expect("admitted flank")
}

/// A spring wire: a circle of radius 0.5 swept along a two-turn path.
fn wire() -> HelixSurfaceConstruction {
    HelixSurfaceConstruction::try_new(
        [-std::f64::consts::PI, std::f64::consts::PI],
        [0.0, 4.0 * std::f64::consts::PI],
        path(6.0, 2.0),
        HelixSurfaceProfile::Circle(
            HelixCircleProfile::try_new(0.0, 0.5).expect("admitted circle profile"),
        ),
    )
    .expect("admitted wire")
}

fn path(radius: f64, rise: f64) -> HelixPathConstruction {
    HelixPathConstruction::try_new(
        [0.0, 4.0 * std::f64::consts::PI],
        HelixFrame {
            center: Point3::new(0.0, 0.0, 1.0),
            major: Vector3::new(radius, 0.0, 0.0),
            minor: Vector3::new(0.0, radius, 0.0),
            pitch: Vector3::new(0.0, 0.0, rise),
            axis: Vector3::new(0.0, 0.0, 1.0),
        },
        0.0,
    )
    .expect("admitted path")
}

/// `C(t)` of the path and its frame `ρ(t)`, `τ(t)`, per `asm.md` §6.3.
fn path_point(surface: &HelixSurfaceConstruction, parameter: f64) -> (Point3, Vector3, Vector3) {
    let path = surface.path();
    let (major, minor, pitch) = (path.major().get(), path.minor().get(), path.pitch().get());
    let center = path.center().as_raw();
    let turns = parameter / std::f64::consts::TAU;
    let (sin, cos) = parameter.sin_cos();
    let radial = Vector3::new(
        major.x * cos + minor.x * sin,
        major.y * cos + minor.y * sin,
        major.z * cos + minor.z * sin,
    );
    let tangential = Vector3::new(
        -major.x * sin + minor.x * cos,
        -major.y * sin + minor.y * cos,
        -major.z * sin + minor.z * cos,
    );
    let point = Point3::new(
        center.x + radial.x + pitch.x * turns,
        center.y + radial.y + pitch.y * turns,
        center.z + radial.z + pitch.z * turns,
    );
    (point, radial, tangential)
}

fn grid(range: [f64; 2], index: u32) -> f64 {
    range[0] + (range[1] - range[0]) * f64::from(index) / f64::from(SURFACE_SAMPLES)
}

fn fitted_point(surface: &crate::geometry::nurbs::NurbsSurface, u: f64, v: f64) -> Point3 {
    nurbs_surface_point(EvaluationAdmission::Standard, surface, u, v)
        .expect("parameter inside the surface domain")
        .get()
}

#[test]
fn the_flank_surface_stays_within_the_tolerance_at_the_same_parameters() {
    let construction = flank();
    let surface = helix_surface_nurbs(&construction, tolerance(1.0e-5)).expect("fitted flank");
    let HelixSurfaceProfile::Line(line) = construction.profile() else {
        panic!("line profile");
    };
    let direction = line.direction().get();
    let profile = construction.angle_range().get();
    let path = construction.dimension_range().get();
    let mut deviation: f64 = 0.0;
    for row in 0..=SURFACE_SAMPLES {
        for column in 0..=SURFACE_SAMPLES {
            let (u, t) = (grid(profile, row), grid(path, column));
            let (center, radial, tangential) = path_point(&construction, t);
            let exact = Point3::new(
                center.x + u * (direction.x * radial.x + direction.y * tangential.x),
                center.y + u * (direction.x * radial.y + direction.y * tangential.y),
                center.z + u * (direction.x * radial.z + direction.y * tangential.z + direction.z),
            );
            deviation = deviation.max(distance(fitted_point(&surface, u, t), exact));
        }
    }

    assert_eq!((surface.u_degree(), surface.v_degree()), (1, 3));
    assert!(deviation <= 1.0e-5, "deviation={deviation}");
}

#[test]
fn the_wire_surface_lies_on_the_profile_circle_of_its_path_parameter() {
    let construction = wire();
    let surface = helix_surface_nurbs(&construction, tolerance(1.0e-5)).expect("fitted wire");
    let path = construction.dimension_range().get();
    let mut deviation: f64 = 0.0;
    for row in 0..=SURFACE_SAMPLES {
        for column in 0..=SURFACE_SAMPLES {
            let (u, t) = (grid([0.0, 4.0], row), grid(path, column));
            let (center, radial, tangential) = path_point(&construction, t);
            let fitted = fitted_point(&surface, u, t);
            let offset = Vector3::new(
                fitted.x - center.x,
                fitted.y - center.y,
                fitted.z - center.z,
            );
            let out_of_plane = offset.dot(tangential).abs() / tangential.norm();
            let in_plane = (offset.dot(radial) / radial.norm()).hypot(offset.z);
            deviation = deviation.max(out_of_plane.hypot(in_plane - 0.5));
        }
    }

    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 3));
    assert!(deviation <= 1.0e-5, "deviation={deviation}");
}
