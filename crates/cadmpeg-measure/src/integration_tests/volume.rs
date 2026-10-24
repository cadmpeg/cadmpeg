// SPDX-License-Identifier: Apache-2.0
//! Body volume, centroid, closure and orientation readings, with
//! expectations from geometry.

use cadmpeg_ir::topology::{BodyKind, Sense};

use crate::reason::{Category, UnavailableReason};
use crate::report::BodyMeasures;
use crate::select::Selection;
use crate::test_support::builders::{
    assert_measured, measure, plate_with_hole, Builder, TEST_ABSOLUTE_FLOOR,
};
use crate::value::{Quantity, Reading};

/// Rounding of a closure flux that vanishes exactly.
const CLOSURE_ROUNDING: f64 = 1.0e-9;

fn only_body(report: &crate::report::MeasureReport) -> &BodyMeasures {
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    body
}

fn assert_point(quantity: &Quantity<crate::value::Point>, expected: [f64; 3], scale: f64) {
    let measured = quantity.measured().expect("a measured point");
    let point = measured.value().value.get();
    let numeric = measured.numeric_error().get();
    for (value, expected) in [point.x, point.y, point.z].into_iter().zip(expected) {
        assert!(
            (value - expected).abs() <= numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(scale),
            "{value} vs {expected}"
        );
    }
}

#[test]
fn the_cube_encloses_its_volume_about_its_centre() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let report = measure(&cube, &Selection::default());
    let body = only_body(&report);
    assert_measured(body.volume().as_ref().expect("volume"), 1000.0, 1000.0);
    assert_point(
        body.centroid().as_ref().expect("centroid"),
        [5.0, 5.0, 5.0],
        10.0,
    );
    assert!(body.area_centroid().is_none());
    assert_eq!(*body.orientation().consistent(), Some(true));
    let closure = body.orientation().closure_flux().as_ref().expect("closure");
    let magnitude = closure
        .face_sense()
        .measured()
        .expect("closure")
        .value()
        .value
        .get();
    assert!(magnitude <= CLOSURE_ROUNDING * 100.0, "{magnitude}");
    let shell_volume = body.shells()[0].volume().as_ref().expect("shell volume");
    assert_measured(shell_volume, 1000.0, 1000.0);
    assert_measured(
        report.document().closed_volume().as_ref().expect("total"),
        1000.0,
        1000.0,
    );
}

#[test]
fn a_flipped_cube_face_leaves_the_volume_to_its_readings() {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let flipped = cube.model.faces[0].id.as_str().to_owned();
    cube.model.faces[0].sense = match cube.model.faces[0].sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    };
    let report = measure(&cube, &Selection::default());
    let body = only_body(&report);
    let Quantity::Unavailable(volume) = body.volume().as_ref().expect("volume") else {
        panic!("the volume is unavailable");
    };
    assert_eq!(
        volume.reason(),
        UnavailableReason::VolumeOrientationConflict
    );
    assert_eq!(volume.category(), Category::Input);
    let candidate = |reading: Reading| {
        volume
            .candidates()
            .iter()
            .find(|candidate| candidate.reading() == reading)
            .map(|candidate| candidate.value().value.get())
            .expect("candidate")
    };
    assert!((candidate(Reading::FaceSense) - 2000.0 / 3.0).abs() <= TEST_ABSOLUTE_FLOOR * 1000.0);
    assert!((candidate(Reading::LoopWinding) - 1000.0).abs() <= TEST_ABSOLUTE_FLOOR * 1000.0);
    let closure = body.orientation().closure_flux().as_ref().expect("closure");
    let flux = |quantity: &Quantity<crate::value::Scalar>| {
        quantity.measured().expect("closure").value().value.get()
    };
    assert!((flux(closure.face_sense()) - 200.0).abs() <= TEST_ABSOLUTE_FLOOR * 200.0);
    assert!(flux(closure.loop_winding()) <= CLOSURE_ROUNDING * 100.0);
    assert_eq!(
        body.orientation().inconsistent(),
        std::slice::from_ref(&flipped)
    );
    assert_eq!(*body.orientation().consistent(), Some(false));
    assert_eq!(report.document().orientation().inconsistent(), &[flipped]);
}

#[test]
fn an_open_sheet_has_no_volume_and_an_area_centroid() {
    let plate = plate_with_hole(true);
    let report = measure(&plate, &Selection::default());
    let body = only_body(&report);
    let Quantity::Unavailable(volume) = body.volume().as_ref().expect("volume") else {
        panic!("the volume is unavailable");
    };
    assert_eq!(volume.reason(), UnavailableReason::VolumeShellOpen);
    assert_eq!(volume.category(), Category::Undefined);
    assert!(!*body.shells()[0].closed());
    assert_point(
        body.area_centroid().as_ref().expect("area centroid"),
        [20.0, 20.0, 0.0],
        40.0,
    );
}

#[test]
fn a_face_on_natural_domain_edges_opens_its_shell() {
    for sense in [Sense::Forward, Sense::Reversed] {
        let mut builder = Builder::new();
        let surface = builder.nurbs(
            (1, vec![0.0, 0.0, 1.0, 1.0]),
            (1, vec![0.0, 0.0, 1.0, 1.0]),
            &[
                vec![([0.0, 0.0, 0.0], 1.0), ([0.0, 2.0, 0.0], 1.0)],
                vec![([2.0, 0.0, 0.0], 1.0), ([2.0, 2.0, 0.0], 1.0)],
            ],
        );
        let face = builder.face(&surface, sense, &[], false);
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(&ir, &Selection::default());
        let body = only_body(&report);
        let shell = &body.shells()[0];
        assert!(!*shell.closed());
        assert_eq!(shell.natural_edge_faces(), &[face.as_str().to_owned()]);
        let Quantity::Unavailable(volume) = body.volume().as_ref().expect("volume") else {
            panic!("the volume is unavailable");
        };
        assert_eq!(volume.reason(), UnavailableReason::VolumeShellOpen);
        assert_point(
            body.area_centroid().as_ref().expect("area centroid"),
            [1.0, 1.0, 0.0],
            2.0,
        );
    }
}
