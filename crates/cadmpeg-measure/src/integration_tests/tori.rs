// SPDX-License-Identifier: Apache-2.0
//! Ring torus faces: whole tori, a band between two parallels and an elbow
//! between two meridians, with expectations from geometry.

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::report::{FaceMeasures, MeasureReport, RegionRule};
use crate::select::Selection;
use crate::test_support::{
    assert_measured, assert_tight_box, measure, Builder, Frame, TEST_ABSOLUTE_FLOOR,
};
use crate::value::{Quantity, Reading};

const MAJOR: f64 = 10.0;
const MINOR: f64 = 2.0;

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

/// The area of the torus between the parallels `v0` and `v1`.
fn band_area(v0: f64, v1: f64) -> f64 {
    TAU * MINOR * (MAJOR * (v1 - v0) + MINOR * (v1.sin() - v0.sin()))
}

#[test]
fn a_torus_without_loops_is_whole_and_closed() {
    let mut builder = Builder::new();
    let torus = builder.torus(
        Frame {
            origin: [0.0; 3],
            axis: [0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2],
            reference: [1.0, 0.0, 0.0],
        },
        MAJOR,
        MINOR,
    );
    builder.face(&torus, Sense::Forward, &[], false);
    let ir = builder.finish(BodyKind::Solid);
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    assert_eq!(*measures.region(), Some(RegionRule::WholeSurface));
    assert_measured(measures.area(), TAU * TAU * MAJOR * MINOR, MAJOR * MAJOR);
    let tilted = MAJOR * FRAC_1_SQRT_2 + MINOR;
    let outer = MAJOR + MINOR;
    assert_tight_box(
        measures.bbox(),
        [-outer, -tilted, -tilted],
        [outer, tilted, tilted],
        outer,
    );
    let report = measure(&ir, &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert_measured(
        body.volume().as_ref().expect("volume"),
        2.0 * PI * PI * MAJOR * MINOR * MINOR,
        MAJOR.powi(3),
    );
}

#[test]
fn seam_circles_bound_the_whole_torus() {
    let mut builder = Builder::new();
    let vertex = builder.vertex([MAJOR + MINOR, 0.0, 0.0]);
    let parallel = builder.circle(
        [0.0; 3],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        MAJOR + MINOR,
        &vertex,
    );
    let meridian = builder.circle(
        [MAJOR, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
        MINOR,
        &vertex,
    );
    let torus = builder.torus(Frame::z_up([0.0; 3]), MAJOR, MINOR);
    builder.face(
        &torus,
        Sense::Forward,
        &[vec![
            (parallel.clone(), Sense::Forward),
            (meridian.clone(), Sense::Forward),
            (parallel, Sense::Reversed),
            (meridian, Sense::Reversed),
        ]],
        false,
    );
    let ir = builder.finish(BodyKind::Solid);
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    assert_eq!(*measures.region(), Some(RegionRule::WholeSurface));
    assert_measured(measures.area(), TAU * TAU * MAJOR * MINOR, MAJOR * MAJOR);
    assert_tight_box(
        measures.bbox(),
        [-12.0, -12.0, -MINOR],
        [12.0, 12.0, MINOR],
        12.0,
    );
    let report = measure(&ir, &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert!(*body.shells()[0].closed());
    assert_measured(
        body.volume().as_ref().expect("volume"),
        2.0 * PI * PI * MAJOR * MINOR * MINOR,
        MAJOR.powi(3),
    );
}

#[test]
fn a_band_between_two_parallels_and_its_complement() {
    let mut builder = Builder::new();
    let low = builder.vertex([MAJOR, 0.0, -MINOR]);
    let high = builder.vertex([MAJOR, 0.0, MINOR]);
    let lower = builder.circle(
        [0.0, 0.0, -MINOR],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        MAJOR,
        &low,
    );
    let upper = builder.circle(
        [0.0, 0.0, MINOR],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        MAJOR,
        &high,
    );
    let torus = builder.torus(Frame::z_up([0.0; 3]), MAJOR, MINOR);
    builder.face(
        &torus,
        Sense::Forward,
        &[
            vec![(lower, Sense::Forward)],
            vec![(upper, Sense::Reversed)],
        ],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    assert_eq!(*measures.region(), Some(RegionRule::DeclaredOrientation));
    let outer = band_area(-FRAC_PI_2, FRAC_PI_2);
    assert_measured(measures.area(), outer, MAJOR * MAJOR);
    let area = measures.area().measured().expect("area");
    assert_eq!(area.reading(), Some(Reading::Declared));
    let [candidate] = area.candidates() else {
        panic!("one candidate");
    };
    let inner = band_area(FRAC_PI_2, 3.0 * FRAC_PI_2);
    assert!((candidate.value().value.get() - inner).abs() <= TEST_ABSOLUTE_FLOOR * outer);
    let bbox = measures.bbox().measured().expect("box");
    let (low, high) = (bbox.value().min.get(), bbox.value().max.get());
    for (computed, expected) in [
        (low.x, -12.0),
        (low.y, -12.0),
        (low.z, -MINOR),
        (high.x, 12.0),
        (high.y, 12.0),
        (high.z, MINOR),
    ] {
        assert!((computed - expected).abs() <= TEST_ABSOLUTE_FLOOR * 12.0);
    }
    let [box_candidate] = bbox.candidates() else {
        panic!("one box candidate");
    };
    let high = box_candidate.value().max.get();
    assert!((high.x - MAJOR).abs() <= TEST_ABSOLUTE_FLOOR * 12.0);
}

/// The elbow `u ∈ [0, 1]` of the torus, bounded by its meridians at `u = 0`
/// and `u = 1`, whose closed edges start at `v = starts[0]` and
/// `v = starts[1]`.
fn elbow(starts: [f64; 2]) -> CadIr {
    let mut builder = Builder::new();
    let mut rings = Vec::new();
    for (u, start, sense) in [
        (0.0_f64, starts[0], Sense::Reversed),
        (1.0_f64, starts[1], Sense::Forward),
    ] {
        let (sine, cosine) = u.sin_cos();
        let radial = [cosine, sine, 0.0];
        let (v_sine, v_cosine) = start.sin_cos();
        let reference = [radial[0] * v_cosine, radial[1] * v_cosine, v_sine];
        let center = [MAJOR * cosine, MAJOR * sine, 0.0];
        let vertex = builder.vertex([
            center[0] + MINOR * reference[0],
            center[1] + MINOR * reference[1],
            MINOR * reference[2],
        ]);
        let meridian = builder.circle(center, [sine, -cosine, 0.0], reference, MINOR, &vertex);
        rings.push(vec![(meridian, sense)]);
    }
    let torus = builder.torus(Frame::z_up([0.0; 3]), MAJOR, MINOR);
    builder.face(&torus, Sense::Forward, &rings, false);
    builder.finish(BodyKind::Sheet)
}

#[test]
fn an_elbow_area_does_not_depend_on_its_start_points() {
    let elbow_area = TAU * MINOR * MAJOR;
    let whole = TAU * TAU * MAJOR * MINOR;
    for starts in [
        [0.0, 0.0],
        [0.0, FRAC_PI_2],
        [FRAC_PI_2, 0.0],
        [1.0, 2.5],
        [3.0, -3.0],
    ] {
        let report = measure(&elbow(starts), &faces());
        let measures = only_face(&report).measures();
        assert_eq!(*measures.region(), Some(RegionRule::DeclaredOrientation));
        assert_measured(measures.area(), elbow_area, MAJOR * MAJOR);
        let Quantity::Measured(area) = measures.area() else {
            panic!("a measured area");
        };
        let [candidate] = area.candidates() else {
            panic!("one candidate");
        };
        assert!(
            (candidate.value().value.get() - (whole - elbow_area)).abs()
                <= TEST_ABSOLUTE_FLOOR * whole,
            "{starts:?}"
        );
    }
}
