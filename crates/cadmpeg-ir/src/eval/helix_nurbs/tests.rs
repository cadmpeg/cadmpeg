// SPDX-License-Identifier: Apache-2.0

use super::helix_nurbs;
use crate::eval::decode::nurbs_curve_point_at;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{HelixCurveConstruction, HelixFrame};
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
    /// `q = (t - t0) / 2π` and `s = 1 + apex_factor·q`.
    fn point(&self, parameter: f64) -> Point3 {
        let turns = (parameter - self.angle_range[0]) / std::f64::consts::TAU;
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
