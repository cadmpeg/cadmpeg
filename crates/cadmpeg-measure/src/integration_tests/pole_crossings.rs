// SPDX-License-Identifier: Apache-2.0
//! Faces whose boundary edge passes through a sphere pole or a cone apex
//! inside its parameter interval, with expectations from geometry.

use std::f64::consts::PI;

use cadmpeg_ir::ids::{EdgeId, VertexId};
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::UnavailableReason;
use crate::report::{FaceMeasures, MeasureReport};
use crate::select::{Selection, SurfaceKind};
use crate::test_support::builders::{
    assert_measured, measure, Builder, Circle, Frame, TEST_ABSOLUTE_FLOOR,
};
use crate::value::{Quantity, Reading, Scalar};

fn faces() -> Selection {
    Selection {
        all_faces: true,
        ..Selection::default()
    }
}

fn only_face(report: &MeasureReport) -> &FaceMeasures {
    let [face] = report.faces().as_slice() else {
        panic!("one face row");
    };
    face
}

/// The value of the one complement candidate of a measured scalar.
fn complement(quantity: &Quantity<Scalar>) -> f64 {
    let measured = quantity.measured().expect("measured");
    let [candidate] = measured.candidates() else {
        panic!("one candidate");
    };
    assert_eq!(candidate.reading(), Reading::Complement);
    candidate.value().value.get()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let norm = dot(a, a).sqrt();
    [a[0] / norm, a[1] / norm, a[2] / norm]
}

/// The minor great-circle arc of the sphere `radius` about the origin from
/// the unit direction `start` to the unit direction `end`.
fn great_arc(
    builder: &mut Builder,
    radius: f64,
    [start, end]: [[f64; 3]; 2],
    [from, to]: [&VertexId; 2],
) -> EdgeId {
    builder.arc(
        Circle {
            frame: Frame {
                origin: [0.0; 3],
                axis: unit(cross(start, end)),
                reference: start,
            },
            radius,
        },
        [0.0, dot(start, end).acos()],
        from,
        to,
    )
}

/// The geodesic triangle of the sphere `R = 10` about the origin with the
/// corners `A` at 30° from the north pole toward `+y`, `B` at 20° from the
/// north pole toward `−y`, and `C = (R, 0, 0)`. The arc `AB` lies in the
/// plane `x = 0` and passes through the north pole 30° from `A`, between
/// samples. `C` is the pole of the plane `x = 0`, so the corners at `A` and
/// `B` are right angles and the corner at `C` is 50°. By Girard's theorem
/// the area is `R²·(π/2 + π/2 + 5π/18 − π) = 5πR²/18`.
///
/// The face sense forward runs `A → B → C`; the face sense reversed runs
/// `C → B → A` on the reversed coedges. Both bound the triangle on the left
/// of the face normal.
#[test]
fn a_geodesic_triangle_with_an_edge_through_the_pole() {
    let radius = 10.0_f64;
    let (sin30, cos30) = (PI / 6.0).sin_cos();
    let (sin20, cos20) = (PI / 9.0).sin_cos();
    let corners = [[0.0, sin30, cos30], [0.0, -sin20, cos20], [1.0, 0.0, 0.0]];
    let triangle = 5.0 * PI * radius * radius / 18.0;
    let whole = 4.0 * PI * radius * radius;
    for sense in [Sense::Forward, Sense::Reversed] {
        let mut builder = Builder::new();
        let vertices: Vec<VertexId> = corners
            .iter()
            .map(|corner| builder.vertex(corner.map(|value| radius * value)))
            .collect();
        let mut ring = Vec::new();
        for index in 0..3 {
            let next = (index + 1) % 3;
            let arc = great_arc(
                &mut builder,
                radius,
                [corners[index], corners[next]],
                [&vertices[index], &vertices[next]],
            );
            ring.push((arc, sense));
        }
        if sense == Sense::Reversed {
            ring.reverse();
        }
        let sphere = builder.sphere(Frame::z_up([0.0; 3]), radius);
        builder.face(&sphere, sense, &[ring], false);
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(&ir, &faces());
        let measures = only_face(&report).measures();
        assert_measured(measures.area(), triangle, radius * radius);
        assert!(
            (complement(measures.area()) - (whole - triangle)).abs() <= TEST_ABSOLUTE_FLOOR * whole,
            "{sense:?}"
        );
        let edges = radius * (5.0 * PI / 18.0 + 2.0 * PI / 2.0);
        assert_measured(measures.boundary_length(), edges, radius);
    }
}

/// The half ball of radius 5 about `(1, 2, 3)` on the `+y` or `−y` side of
/// the plane `y = 2`. Its sphere face is bounded by the one closed great
/// circle in that plane, which passes through both poles of the `+z` carrier
/// axis: through the south pole at a sample and through the north pole
/// between samples. The disc in the plane closes the body. The volume is
/// `2πR³/3`, the sphere face's area `2πR²`, and the centroid lies `3R/8`
/// from the plane.
#[test]
fn a_half_ball_bounded_by_a_great_circle_through_both_poles() {
    let radius = 5.0_f64;
    let center = [1.0, 2.0, 3.0];
    for side in [1.0_f64, -1.0] {
        let mut builder = Builder::new();
        let vertex = builder.vertex([center[0] + radius, center[1], center[2]]);
        let circle = builder.circle(center, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], radius, &vertex);
        let sphere = builder.sphere(Frame::z_up(center), radius);
        let plane = builder.plane(center, [0.0, -side, 0.0], [1.0, 0.0, 0.0]);
        let (on_sphere, on_plane) = if side > 0.0 {
            (Sense::Forward, Sense::Reversed)
        } else {
            (Sense::Reversed, Sense::Forward)
        };
        builder.face(
            &sphere,
            Sense::Forward,
            &[vec![(circle.clone(), on_sphere)]],
            false,
        );
        builder.face(&plane, Sense::Forward, &[vec![(circle, on_plane)]], true);
        let ir = builder.finish(BodyKind::Solid);
        let report = measure(&ir, &Selection::default());
        let [body] = report.bodies().as_slice() else {
            panic!("one body row");
        };
        let cube = radius.powi(3);
        assert_measured(
            body.volume().as_ref().expect("volume"),
            2.0 * PI * cube / 3.0,
            cube,
        );
        let centroid = body
            .centroid()
            .as_ref()
            .and_then(Quantity::measured)
            .expect("centroid")
            .value()
            .value
            .get();
        let expected = [center[0], center[1] + side * 3.0 * radius / 8.0, center[2]];
        for (computed, expected) in [centroid.x, centroid.y, centroid.z]
            .into_iter()
            .zip(expected)
        {
            assert!(
                (computed - expected).abs() <= TEST_ABSOLUTE_FLOOR * radius,
                "{side}: {computed} against {expected}"
            );
        }
        let report = measure(&ir, &faces());
        let cap = report
            .faces()
            .iter()
            .find(|face| face.carrier().kind == SurfaceKind::Sphere)
            .expect("sphere face");
        assert_measured(
            cap.measures().area(),
            2.0 * PI * radius * radius,
            radius * radius,
        );
    }
}

/// The cone with apex `(0, 0, 0)`, axis `+z`, stored origin `(0, 0, 10)`,
/// radius 5 at the origin and `tan α = 1/2`.
fn cone(builder: &mut Builder) -> cadmpeg_ir::ids::SurfaceId {
    builder.cone(Frame::z_up([0.0, 0.0, 10.0]), 5.0, 0.5_f64.atan())
}

/// A sector of the cone between the apex and the circle of radius 10 at
/// `z = 20`, of angle `φ` from `+x`. One polyline edge runs up from the rim
/// at `φ` to the apex and down to the rim at angle 0, with its kink at the
/// apex inside its interval; the rim arc closes the loop. The lateral area
/// is `φ·ρ·L/2` with `ρ = 10` and slant length `L = 10√5`.
#[test]
fn a_cone_sector_whose_edge_turns_at_the_apex() {
    let slant = 10.0 * 5.0_f64.sqrt();
    for angle in [0.5 * PI, 1.5 * PI] {
        let mut builder = Builder::new();
        let start = builder.vertex([10.0, 0.0, 20.0]);
        let corner = [10.0 * angle.cos(), 10.0 * angle.sin(), 20.0];
        let end = builder.vertex(corner);
        let generators = builder.polyline(&[corner, [0.0; 3], [10.0, 0.0, 20.0]], &end, &start);
        let rim = builder.arc(
            Circle {
                frame: Frame::z_up([0.0, 0.0, 20.0]),
                radius: 10.0,
            },
            [0.0, angle],
            &start,
            &end,
        );
        let surface = cone(&mut builder);
        builder.face(
            &surface,
            Sense::Reversed,
            &[vec![(generators, Sense::Forward), (rim, Sense::Forward)]],
            true,
        );
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(&ir, &faces());
        let measures = only_face(&report).measures();
        assert_measured(measures.area(), angle * 10.0 * slant / 2.0, 400.0);
        assert_measured(measures.boundary_length(), 2.0 * slant + 10.0 * angle, 20.0);
    }
}

/// A line of the cone through the apex from the rim point `(10, 0, 20)` to
/// the point `(−10, 0, −20)` of the opposite nappe, and back.
#[test]
fn a_line_through_the_cone_apex_onto_the_opposite_nappe_is_refused() {
    let mut builder = Builder::new();
    let upper = builder.vertex([10.0, 0.0, 20.0]);
    let lower = builder.vertex([-10.0, 0.0, -20.0]);
    let down = builder.line(&upper, &lower);
    let up = builder.line(&lower, &upper);
    let surface = cone(&mut builder);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![(down, Sense::Forward), (up, Sense::Forward)]],
        true,
    );
    let ir: CadIr = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let area = only_face(&report)
        .measures()
        .area()
        .unavailable()
        .expect("unavailable area");
    assert_eq!(area.reason(), UnavailableReason::BoundaryInversionFailed);
    assert!(
        area.message().contains("opposite nappe"),
        "{}",
        area.message()
    );
}
