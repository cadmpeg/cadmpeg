// SPDX-License-Identifier: Apache-2.0
//! Cylinder faces: a seam-cut full side, a band between two circles, and
//! trimmed patches, with expectations from geometry.

use std::f64::consts::PI;

use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::report::{FaceMeasures, LoopRole, MeasureReport, RegionRule};
use crate::select::Selection;
use crate::test_support::builders::{
    assert_measured, assert_range, assert_tight_box, measure, Builder, Circle, Frame,
};

const RADIUS: f64 = 10.0;
const HEIGHT: f64 = 30.0;

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

fn circle_at(z: f64) -> Circle {
    Circle {
        frame: Frame::z_up([0.0, 0.0, z]),
        radius: RADIUS,
    }
}

/// The side of the cylinder `r = 10`, `z ∈ [0, 30]`, as one ring with a
/// seam: bottom circle, seam up, top circle reversed, seam down.
fn seam_cut_side(sense: Sense) -> CadIr {
    let mut builder = Builder::new();
    let low = builder.vertex([RADIUS, 0.0, 0.0]);
    let high = builder.vertex([RADIUS, 0.0, HEIGHT]);
    let bottom = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], RADIUS, &low);
    let top = builder.circle(
        [0.0, 0.0, HEIGHT],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        RADIUS,
        &high,
    );
    let seam = builder.line(&low, &high);
    let surface = builder.cylinder(Frame::z_up([0.0; 3]), RADIUS);
    builder.face(
        &surface,
        sense,
        &[vec![
            (bottom, Sense::Forward),
            (seam.clone(), Sense::Forward),
            (top, Sense::Reversed),
            (seam, Sense::Reversed),
        ]],
        true,
    );
    builder.finish(BodyKind::Sheet)
}

#[test]
fn a_seam_cut_side_face_measures_the_full_cylinder() {
    let ir = seam_cut_side(Sense::Forward);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    let scale = 2.0 * RADIUS;
    assert_measured(measures.area(), 2.0 * PI * RADIUS * HEIGHT, scale * HEIGHT);
    assert_measured(measures.boundary_length(), 4.0 * PI * RADIUS, scale);
    assert_measured(measures.seam_length(), HEIGHT, HEIGHT);
    assert_eq!(*measures.region(), Some(RegionRule::Nesting));
    assert_range(measures.axial_range().as_ref(), [0.0, HEIGHT], HEIGHT);
    assert_measured(
        measures.axial_extent().as_ref().expect("extent"),
        HEIGHT,
        HEIGHT,
    );
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(crate::value::Quantity::measured)
        .expect("sweep");
    assert!(sweep.value().full);
    assert_eq!(sweep.value().value.get(), 2.0 * PI);
    assert_tight_box(
        measures.bbox(),
        [-RADIUS, -RADIUS, 0.0],
        [RADIUS, RADIUS, HEIGHT],
        HEIGHT,
    );
    assert_eq!(*face.orientation().consistent(), Some(true));

    let reversed = seam_cut_side(Sense::Reversed);
    let report = measure(&reversed, &faces());
    let face = only_face(&report);
    assert_measured(
        face.measures().area(),
        2.0 * PI * RADIUS * HEIGHT,
        scale * HEIGHT,
    );
    assert_eq!(*face.orientation().consistent(), Some(false));
}

#[test]
fn a_band_between_two_circles_is_bounded_by_both() {
    let mut builder = Builder::new();
    let low = builder.vertex([RADIUS, 0.0, 0.0]);
    let high = builder.vertex([RADIUS, 0.0, HEIGHT]);
    let bottom = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], RADIUS, &low);
    let top = builder.circle(
        [0.0, 0.0, HEIGHT],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        RADIUS,
        &high,
    );
    let surface = builder.cylinder(Frame::z_up([0.0; 3]), RADIUS);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![(top, Sense::Reversed)], vec![(bottom, Sense::Forward)]],
        true,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    assert_measured(
        measures.area(),
        2.0 * PI * RADIUS * HEIGHT,
        2.0 * RADIUS * HEIGHT,
    );
    let roles: Vec<Option<LoopRole>> = measures.loops().iter().map(|entry| *entry.role()).collect();
    assert_eq!(
        roles,
        [Some(LoopRole::BandUpper), Some(LoopRole::BandLower)]
    );
    assert_eq!(*face.orientation().consistent(), Some(true));
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(crate::value::Quantity::measured)
        .expect("sweep");
    assert!(sweep.value().full);
    assert_range(measures.axial_range().as_ref(), [0.0, HEIGHT], HEIGHT);
}

/// A patch `u ∈ [start, start + span]`, `z ∈ [0, 30]` of the cylinder,
/// with its ring listed in `order` (bottom, right, top, left).
fn patch(start: f64, span: f64, order: [usize; 4]) -> CadIr {
    let mut builder = Builder::new();
    let point = |u: f64, z: f64| [RADIUS * u.cos(), RADIUS * u.sin(), z];
    let a = builder.vertex(point(start, 0.0));
    let b = builder.vertex(point(start + span, 0.0));
    let c = builder.vertex(point(start + span, HEIGHT));
    let d = builder.vertex(point(start, HEIGHT));
    let range = [start, start + span];
    let bottom = builder.arc(circle_at(0.0), range, &a, &b);
    let right = builder.line(&b, &c);
    let top = builder.arc(circle_at(HEIGHT), range, &d, &c);
    let left = builder.line(&a, &d);
    let surface = builder.cylinder(Frame::z_up([0.0; 3]), RADIUS);
    let ring = [
        (bottom, Sense::Forward),
        (right, Sense::Forward),
        (top, Sense::Reversed),
        (left, Sense::Reversed),
    ];
    builder.face(
        &surface,
        Sense::Forward,
        &[order.iter().map(|index| ring[*index].clone()).collect()],
        true,
    );
    builder.finish(BodyKind::Sheet)
}

const LISTED: [usize; 4] = [0, 1, 2, 3];

/// Rounding of exact angle arithmetic.
const ANGLE_ROUNDING: f64 = 1.0e-12;

#[test]
fn a_quarter_patch_measures_its_sweep_extent_and_area() {
    let ir = patch(0.0, PI / 2.0, LISTED);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    assert_measured(measures.area(), 150.0 * PI, RADIUS * HEIGHT);
    assert_measured(measures.boundary_length(), 10.0 * PI + 60.0, HEIGHT);
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(crate::value::Quantity::measured)
        .expect("sweep");
    assert!(!sweep.value().full);
    assert!((sweep.value().value.get() - PI / 2.0).abs() <= ANGLE_ROUNDING);
    let [[start, end]] = sweep.value().intervals.as_slice() else {
        panic!("one interval");
    };
    assert!(start.get().abs() <= ANGLE_ROUNDING);
    assert!((end.get() - PI / 2.0).abs() <= ANGLE_ROUNDING);
    assert_tight_box(measures.bbox(), [0.0; 3], [RADIUS, RADIUS, HEIGHT], HEIGHT);
    assert_eq!(*face.orientation().consistent(), Some(true));
}

#[test]
fn a_patch_across_the_reference_direction_sweeps_one_interval() {
    let ir = patch(-PI / 4.0, PI / 2.0, LISTED);
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    assert_measured(measures.area(), 150.0 * PI, RADIUS * HEIGHT);
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(crate::value::Quantity::measured)
        .expect("sweep");
    let [[start, end]] = sweep.value().intervals.as_slice() else {
        panic!("one interval");
    };
    assert!((start.get() - 7.0 * PI / 4.0).abs() <= ANGLE_ROUNDING);
    assert!((end.get() - 9.0 * PI / 4.0).abs() <= ANGLE_ROUNDING);
    let half = RADIUS * (PI / 4.0).cos();
    assert_tight_box(
        measures.bbox(),
        [half, -half, 0.0],
        [RADIUS, half, HEIGHT],
        HEIGHT,
    );
}

#[test]
fn a_patch_listed_out_of_order_keeps_its_area() {
    let ir = patch(0.0, PI / 2.0, [0, 2, 1, 3]);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    assert_measured(face.measures().area(), 150.0 * PI, RADIUS * HEIGHT);
    assert_eq!(face.caveats().len(), 1);
}
