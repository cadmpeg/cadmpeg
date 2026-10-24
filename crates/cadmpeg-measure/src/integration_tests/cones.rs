// SPDX-License-Identifier: Apache-2.0
//! Cone faces: a tip bounded by one circle, a frustum band, a seam-cut full
//! cone and a sector that meets the apex, with expectations from geometry.
//!
//! Every face lies on the cone with apex `(0, 0, 0)`, axis `+z`, stored
//! origin `(0, 0, 10)`, radius 5 at the origin and `tan α = 1/2`, so the
//! cross-section radius at height `z` is `|z| / 2` and the slant length from
//! the apex to a circle of radius `ρ` is `ρ·√5`.

use std::f64::consts::PI;

use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::report::{FaceMeasures, LoopRole, MeasureReport, RegionRule};
use crate::select::Selection;
use crate::test_support::builders::{
    assert_measured, assert_range, assert_tight_box, measure, Builder, Circle, Frame,
};
use crate::value::Quantity;

/// Rounding of exact angle arithmetic.
const ANGLE_ROUNDING: f64 = 1.0e-12;

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

/// The circle of the cone at height `z`.
fn section(z: f64) -> Circle {
    Circle {
        frame: Frame::z_up([0.0, 0.0, z]),
        radius: z.abs() / 2.0,
    }
}

fn cone(builder: &mut Builder) -> cadmpeg_ir::ids::SurfaceId {
    builder.cone(Frame::z_up([0.0, 0.0, 10.0]), 5.0, 0.5_f64.atan())
}

/// The lateral area between the apex and the circle of radius `rho`.
fn tip_area(rho: f64) -> f64 {
    PI * rho * rho * 5.0_f64.sqrt()
}

fn closed_section(builder: &mut Builder, z: f64) -> cadmpeg_ir::ids::EdgeId {
    let rho = z.abs() / 2.0;
    let vertex = builder.vertex([rho, 0.0, z]);
    builder.circle(
        [0.0, 0.0, z],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        rho,
        &vertex,
    )
}

fn single_face(
    rings: impl FnOnce(&mut Builder) -> Vec<Vec<(cadmpeg_ir::ids::EdgeId, Sense)>>,
    sense: Sense,
) -> CadIr {
    let mut builder = Builder::new();
    let rings = rings(&mut builder);
    let surface = cone(&mut builder);
    builder.face(&surface, sense, &rings, true);
    builder.finish(BodyKind::Sheet)
}

#[test]
fn a_tip_bounded_by_one_circle_reaches_the_apex() {
    let ir = single_face(
        |builder| vec![vec![(closed_section(builder, 20.0), Sense::Reversed)]],
        Sense::Forward,
    );
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    assert_measured(measures.area(), tip_area(10.0), 400.0);
    assert_eq!(*measures.region(), Some(RegionRule::Nesting));
    assert_eq!(*measures.loops()[0].role(), Some(LoopRole::Outer));
    assert_eq!(*face.orientation().consistent(), Some(true));
    assert_range(measures.axial_range().as_ref(), [-10.0, 10.0], 20.0);
    assert_range(measures.radius_range().as_ref(), [0.0, 10.0], 20.0);
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("sweep");
    assert!(sweep.value().full);
    assert_tight_box(
        measures.bbox(),
        [-10.0, -10.0, 0.0],
        [10.0, 10.0, 20.0],
        20.0,
    );
}

#[test]
fn a_tip_on_the_lower_nappe_has_the_apex_above() {
    let ir = single_face(
        |builder| vec![vec![(closed_section(builder, -20.0), Sense::Forward)]],
        Sense::Forward,
    );
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    assert_measured(measures.area(), tip_area(10.0), 400.0);
    assert_eq!(*measures.loops()[0].role(), Some(LoopRole::Outer));
    assert_range(measures.axial_range().as_ref(), [-30.0, -10.0], 30.0);
    assert_range(measures.radius_range().as_ref(), [0.0, 10.0], 20.0);
    assert_tight_box(
        measures.bbox(),
        [-10.0, -10.0, -20.0],
        [10.0, 10.0, 0.0],
        20.0,
    );
}

#[test]
fn a_frustum_band_lies_between_its_circles() {
    let ir = single_face(
        |builder| {
            vec![
                vec![(closed_section(builder, 10.0), Sense::Forward)],
                vec![(closed_section(builder, 20.0), Sense::Reversed)],
            ]
        },
        Sense::Forward,
    );
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    assert_measured(measures.area(), tip_area(10.0) - tip_area(5.0), 400.0);
    let roles: Vec<Option<LoopRole>> = measures.loops().iter().map(|entry| *entry.role()).collect();
    assert_eq!(
        roles,
        [Some(LoopRole::BandLower), Some(LoopRole::BandUpper)]
    );
    assert_eq!(*face.orientation().consistent(), Some(true));
    assert_range(measures.axial_range().as_ref(), [0.0, 10.0], 20.0);
    assert_range(measures.radius_range().as_ref(), [5.0, 10.0], 20.0);
    assert_tight_box(
        measures.bbox(),
        [-10.0, -10.0, 10.0],
        [10.0, 10.0, 20.0],
        20.0,
    );
}

#[test]
fn a_seam_cut_cone_runs_its_seam_to_the_apex() {
    let mut builder = Builder::new();
    let rim = builder.vertex([10.0, 0.0, 20.0]);
    let apex = builder.vertex([0.0, 0.0, 0.0]);
    let circle = builder.circle(
        [0.0, 0.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        10.0,
        &rim,
    );
    let seam = builder.line(&rim, &apex);
    let surface = cone(&mut builder);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![
            (circle, Sense::Reversed),
            (seam.clone(), Sense::Forward),
            (seam, Sense::Reversed),
        ]],
        true,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    let slant = 10.0 * 5.0_f64.sqrt();
    assert_measured(measures.area(), tip_area(10.0), 400.0);
    assert_measured(measures.boundary_length(), 20.0 * PI, 20.0);
    assert_measured(measures.seam_length(), slant, slant);
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("sweep");
    assert!(sweep.value().full);
    assert_range(measures.radius_range().as_ref(), [0.0, 10.0], 20.0);
}

#[test]
fn a_sector_through_the_apex_sweeps_its_angle() {
    let mut builder = Builder::new();
    let apex = builder.vertex([0.0, 0.0, 0.0]);
    let start = builder.vertex([10.0, 0.0, 20.0]);
    let end = builder.vertex([0.0, -10.0, 20.0]);
    let rising = builder.line(&apex, &start);
    let rim = builder.arc(section(20.0), [0.0, 1.5 * PI], &start, &end);
    let falling = builder.line(&apex, &end);
    let surface = cone(&mut builder);
    builder.face(
        &surface,
        Sense::Reversed,
        &[vec![
            (rising, Sense::Forward),
            (rim, Sense::Forward),
            (falling, Sense::Reversed),
        ]],
        true,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    let slant = 10.0 * 5.0_f64.sqrt();
    assert_measured(measures.area(), 0.75 * tip_area(10.0), 400.0);
    assert_measured(measures.boundary_length(), 2.0 * slant + 15.0 * PI, 20.0);
    assert_eq!(*face.orientation().consistent(), Some(true));
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("sweep");
    assert!(!sweep.value().full);
    assert!((sweep.value().value.get() - 1.5 * PI).abs() <= ANGLE_ROUNDING);
    let [[low, high]] = sweep.value().intervals.as_slice() else {
        panic!("one interval");
    };
    assert!(low.get().abs() <= ANGLE_ROUNDING);
    assert!((high.get() - 1.5 * PI).abs() <= ANGLE_ROUNDING);
    assert_range(measures.radius_range().as_ref(), [0.0, 10.0], 20.0);
    assert_tight_box(
        measures.bbox(),
        [-10.0, -10.0, 0.0],
        [10.0, 10.0, 20.0],
        20.0,
    );
}
