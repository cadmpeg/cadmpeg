// SPDX-License-Identifier: Apache-2.0
//! Ring torus faces: whole tori, a band between two parallels and an elbow
//! between two meridians, with expectations from geometry.

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::UnavailableReason;
use crate::report::{FaceMeasures, MeasureReport, RegionRule};
use crate::select::Selection;
use crate::session::{findings, MeasureOptions};
use crate::test_support::builders::{
    assert_measured, assert_tight_box, measure, service_context, Builder, Frame,
    TEST_ABSOLUTE_FLOOR,
};
use crate::value::{Quantity, Reading, Unavailable};

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
    let (angle, full, _) = sweep(measures);
    assert!(full && angle == TAU, "{angle}");
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

/// The strip `u ∈ [low, low + span]` by `v ∈ [−½, ½]` of the torus,
/// bounded by four edges with no 3D curve whose line pcurves run along two
/// parallels and two meridians.
fn strip(low: f64, span: f64) -> CadIr {
    let (v0, v1) = (-0.5, 0.5);
    let high = low + span;
    let position = |u: f64, v: f64| {
        let ring = MAJOR + MINOR * v.cos();
        [ring * u.cos(), ring * u.sin(), MINOR * v.sin()]
    };
    let mut builder = Builder::new();
    let corners = [
        builder.vertex(position(low, v0)),
        builder.vertex(position(high, v0)),
        builder.vertex(position(high, v1)),
        builder.vertex(position(low, v1)),
    ];
    let sides: Vec<_> = (0..4)
        .map(|index| {
            let edge = builder.bare_edge(&corners[index], &corners[(index + 1) % 4]);
            (edge, Sense::Forward)
        })
        .collect();
    let torus = builder.torus(Frame::z_up([0.0; 3]), MAJOR, MINOR);
    let face = builder.face(&torus, Sense::Forward, &[sides], false);
    builder.line_pcurve(&face, [0, 0], [low, v0], [1.0, 0.0], [0.0, span]);
    builder.line_pcurve(&face, [0, 1], [high, v0], [0.0, 1.0], [0.0, v1 - v0]);
    builder.line_pcurve(&face, [0, 2], [high, v1], [-1.0, 0.0], [0.0, span]);
    builder.line_pcurve(&face, [0, 3], [low, v1], [0.0, -1.0], [0.0, v1 - v0]);
    builder.finish(BodyKind::Sheet)
}

#[test]
fn a_strip_shorter_than_a_period_has_its_area() {
    let report = measure(&strip(0.0, 1.0), &faces());
    let measures = only_face(&report).measures();
    assert_eq!(*measures.region(), Some(RegionRule::DeclaredOrientation));
    assert_measured(measures.area(), band_area(-0.5, 0.5) / TAU, MAJOR * MAJOR);
}

/// A strip whose pcurves cover more whole periods than one span may hold,
/// or lie where a shift by one period is lost, has no region indicator.
/// Its bounded quantities are unavailable within the pass allowance, and
/// the check pass gives no finding: the ring is listed in traversal order,
/// the face has no orientation reading, the sheet has no closed shell, and
/// no slice runs out.
#[test]
fn a_strip_beyond_the_lift_bounds_is_unavailable_and_checks_without_findings() {
    for (low, span, reason) in [
        (0.0, 1.0e12, UnavailableReason::EvaluationWorkExhausted),
        (
            2.0_f64.powi(57),
            1024.0,
            UnavailableReason::BoundaryPeriodUnresolved,
        ),
    ] {
        let ir = strip(low, span);
        let report = measure(&ir, &faces());
        let row = only_face(&report);
        assert_eq!(
            [
                row.measures().area().unavailable().map(Unavailable::reason),
                row.measures().bbox().unavailable().map(Unavailable::reason),
            ],
            [Some(reason); 2],
            "{}",
            reason.code()
        );
        let Quantity::Unavailable(area) = row.measures().area() else {
            panic!("an unavailable area");
        };
        assert_eq!(area.entity(), Some(row.face().as_str()));
        assert!(!report.document().work().exhausted());
        let ctx = service_context();
        let found = findings(&ctx, &ir, MeasureOptions::default()).expect("findings");
        ctx.finish_session().expect("session stays unrefused");
        assert_eq!(found, Vec::new(), "{}", reason.code());
    }
}

/// A face's measured angular sweep about the axis: its angle and whether
/// it is full, with its candidates' angles.
fn sweep(measures: &crate::report::FaceQuantities) -> (f64, bool, Vec<f64>) {
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("sweep");
    (
        sweep.value().value.get(),
        sweep.value().full,
        sweep
            .candidates()
            .iter()
            .map(|candidate| candidate.value().value.get())
            .collect(),
    )
}

/// The sweep about the axis of a ring torus face is the angle its `u`
/// covers: the elbow between the meridians `u = 0` and `u = 1` covers 1 and
/// its complement `2π − 1`; the strip `u ∈ [0, 1]` covers 1; a torus without
/// loops covers the whole circle.
#[test]
fn a_torus_face_sweeps_the_angle_its_meridians_bound() {
    let report = measure(&elbow([0.0, FRAC_PI_2]), &faces());
    let (angle, full, candidates) = sweep(only_face(&report).measures());
    assert!((angle - 1.0).abs() <= 1.0e-12 && !full, "{angle}");
    let [complement] = candidates.as_slice() else {
        panic!("the complement candidate");
    };
    assert!((complement - (TAU - 1.0)).abs() <= 1.0e-12, "{complement}");

    let report = measure(&strip(0.0, 1.0), &faces());
    let (angle, full, _) = sweep(only_face(&report).measures());
    assert!((angle - 1.0).abs() <= 1.0e-12 && !full, "{angle}");

    let mut builder = Builder::new();
    let torus = builder.torus(Frame::z_up([0.0; 3]), MAJOR, MINOR);
    builder.face(&torus, Sense::Forward, &[], false);
    let report = measure(&builder.finish(BodyKind::Solid), &faces());
    let (angle, full, _) = sweep(only_face(&report).measures());
    assert!(full && angle == TAU, "{angle}");
}
