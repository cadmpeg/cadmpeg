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

/// The volume's model uncertainty of the cuboid `[0, 10]² × [0, 1]` with
/// tolerances on its side `x = 10` and its edge `x = 10, z = 0`, and its
/// origin vertex moved up by `corner`.
fn cuboid_volume_model(side: Option<f64>, edge: Option<f64>, corner: f64) -> f64 {
    let mut ir = crate::test_support::builders::cuboid([0.0; 3], [10.0, 10.0, 1.0]);
    if let Some(point) = ir
        .model
        .points
        .iter_mut()
        .find(|point| point.position().get() == cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
    {
        point.set_position(
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                0.0, 0.0, corner,
            ))
            .expect("finite"),
        );
    }
    let faces = Selection {
        all_faces: true,
        ..Selection::default()
    };
    let report = measure(&ir, &faces);
    let at = |value: &crate::value::BoxValue, x: [f64; 2], z: [f64; 2]| {
        let (min, max) = (value.min.get(), value.max.get());
        [min.x, max.x] == x && [min.z, max.z] == z
    };
    let found = report
        .faces()
        .iter()
        .find(|row| {
            row.measures()
                .bbox()
                .measured()
                .is_some_and(|bbox| at(bbox.value(), [10.0, 10.0], [0.0, 1.0]))
        })
        .expect("the side x = 10")
        .face()
        .clone();
    let face = ir
        .model
        .faces
        .iter_mut()
        .find(|face| face.id.as_str() == found)
        .expect("the side's face");
    face.tolerance = side.and_then(cadmpeg_ir::scalar::PositiveReal::new);
    let edges = Selection {
        all_edges: true,
        ..Selection::default()
    };
    let report = measure(&ir, &edges);
    let found = report
        .edges()
        .iter()
        .find(|row| {
            row.bbox()
                .measured()
                .is_some_and(|bbox| at(bbox.value(), [10.0, 10.0], [0.0, 0.0]))
        })
        .expect("the edge x = 10, z = 0")
        .edge()
        .clone();
    let model_edge = ir
        .model
        .edges
        .iter_mut()
        .find(|candidate| candidate.id.as_str() == found)
        .expect("the edge");
    model_edge.tolerance = edge.and_then(cadmpeg_ir::scalar::PositiveReal::new);
    let report = measure(&ir, &Selection::default());
    let volume = only_body(&report)
        .volume()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("volume");
    volume.model_uncertainty().get()
}

/// The centroid is `(5, 5, 0.5)`. Every edge box's farthest corner from it
/// is a cuboid corner, `ρ = √50.25`. The area is 240 mm² and the edge uses
/// are 168 mm long.
#[test]
fn each_face_and_edge_bounds_the_volume_by_its_own_tolerance() {
    let reach = 50.25_f64.sqrt();
    let linear = crate::test_support::builders::cuboid([0.0; 3], [10.0, 10.0, 1.0])
        .tolerances
        .linear
        .get();
    let model = cuboid_volume_model(None, None, 0.0);
    let base = 240.0 * linear + 168.0 / 3.0 * linear * reach;
    assert!((model - base).abs() <= 1.0e-12, "{model} {base}");
    // The side x = 10, of area 10.
    let model = cuboid_volume_model(Some(1.0e-3), None, 0.0);
    let expected = base + 10.0 * (1.0e-3 - linear);
    assert!((model - expected).abs() <= 1.0e-12, "{model} {expected}");
    // The edge x = 10, z = 0: two uses of length 10.
    let model = cuboid_volume_model(None, Some(1.0e-3), 0.0);
    let expected = base + 2.0 * 10.0 / 3.0 * (1.0e-3 - linear) * reach;
    assert!((model - expected).abs() <= 1.0e-12, "{model} {expected}");
    // The corner 1e-3 above the shared end of three edges of lengths 10, 10
    // and 1, each on two faces.
    let gap = 1.0e-3;
    let model = cuboid_volume_model(None, None, gap);
    let expected = base + 2.0 * (10.0 + 10.0 + 1.0) / 3.0 * (gap - linear) * reach;
    assert!((model - expected).abs() <= 1.0e-12, "{model} {expected}");
}
