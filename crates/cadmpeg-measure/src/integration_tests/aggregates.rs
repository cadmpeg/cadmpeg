// SPDX-License-Identifier: Apache-2.0
//! Body and document aggregates over faces whose regions rest on readings:
//! area, box and area centroid, with expectations from geometry.

use std::f64::consts::{PI, SQRT_2};

use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::{Category, UnavailableReason};
use crate::report::{BodyMeasures, MeasureReport};
use crate::select::Selection;
use crate::test_support::{
    assert_measured, measure, plate_with_hole, Builder, Frame, TEST_ABSOLUTE_FLOOR,
};
use crate::value::{BoxValue, Measured, Point, Quantity, Reading};

fn only_body(report: &MeasureReport) -> &BodyMeasures {
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    body
}

fn assert_point(found: Point3, expected: [f64; 3], scale: f64) {
    for (value, wanted) in [found.x, found.y, found.z].into_iter().zip(expected) {
        assert!(
            (value - wanted).abs() <= TEST_ABSOLUTE_FLOOR * scale,
            "{found:?} against {expected:?}"
        );
    }
}

fn assert_corners(value: &BoxValue, min: [f64; 3], max: [f64; 3], scale: f64) {
    assert_point(value.min.get(), min, scale);
    assert_point(value.max.get(), max, scale);
}

/// The value of the candidate under `reading`.
fn candidate<T: Copy>(measured: &Measured<T>, reading: Reading) -> T {
    *measured
        .candidates()
        .iter()
        .find(|candidate| candidate.reading() == reading)
        .expect("candidate")
        .value()
}

/// The cylinder of radius 5 from `z = 0` to `z = 10` and the hemisphere of
/// radius 5 on its top rim, tangent to it. The open shell has no bottom.
/// `dome` is the sense of the hemisphere face: reversed, its declared
/// region is the lower hemisphere `5 ≤ z ≤ 10` and the tangent rim
/// contradicts it.
fn open_capsule(dome: Sense) -> CadIr {
    let mut builder = Builder::new();
    let low = builder.vertex([5.0, 0.0, 0.0]);
    let high = builder.vertex([5.0, 0.0, 10.0]);
    let bottom = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 5.0, &low);
    let rim = builder.circle(
        [0.0, 0.0, 10.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        5.0,
        &high,
    );
    let cylinder = builder.cylinder(Frame::z_up([0.0; 3]), 5.0);
    builder.face(
        &cylinder,
        Sense::Forward,
        &[
            vec![(bottom, Sense::Forward)],
            vec![(rim.clone(), Sense::Reversed)],
        ],
        false,
    );
    let upper = builder.sphere(Frame::z_up([0.0, 0.0, 10.0]), 5.0);
    builder.face(&upper, dome, &[vec![(rim, Sense::Forward)]], true);
    builder.finish(BodyKind::Sheet)
}

/// The side, area `100π` with its centroid at `z = 5`, and the upper
/// hemisphere, area `50π` with its centroid at `z = 12.5`: `z = 7.5`. The
/// lower hemisphere has its centroid at `z = 7.5`, which gives `z = 35/6`.
#[test]
fn a_contradicted_dome_leaves_the_area_centroid_to_its_candidates() {
    let report = measure(&open_capsule(Sense::Forward), &Selection::default());
    let centroid = only_body(&report)
        .area_centroid()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("area centroid");
    assert_eq!(centroid.reading(), None);
    assert_point(centroid.value().value.get(), [0.0, 0.0, 7.5], 10.0);

    let report = measure(&open_capsule(Sense::Reversed), &Selection::default());
    let body = only_body(&report);
    let Some(Quantity::Unavailable(centroid)) = body.area_centroid() else {
        panic!("the area centroid is unavailable");
    };
    assert_eq!(
        centroid.reason(),
        UnavailableReason::RegionOrientationConflict
    );
    assert_eq!(centroid.category(), Category::Input);
    let reading = |reading: Reading| {
        centroid
            .candidates()
            .iter()
            .find(|candidate| candidate.reading() == reading)
            .map(|candidate| candidate.value().value.get())
            .expect("candidate")
    };
    assert_point(reading(Reading::FaceSense), [0.0, 0.0, 35.0 / 6.0], 10.0);
    assert_point(reading(Reading::LoopWinding), [0.0, 0.0, 7.5], 10.0);
}

/// The cap `z ≥ 6` of the sphere of radius 10: area `80π`, box
/// `(−8, −8, 6)..(8, 8, 10)` and area centroid `z = 8` under the declared
/// reading; the rest of the sphere, `z ≤ 6`: area `320π`, box
/// `(−10, −10, −10)..(10, 10, 6)` and area centroid `z = −2` under the
/// complement. The area of a sphere zone is
/// uniform in `z`, so each centroid is the middle of its `z` range.
#[test]
fn body_and_document_totals_rest_on_an_unconfirmed_cap() {
    let mut builder = Builder::new();
    let vertex = builder.vertex([8.0, 0.0, 6.0]);
    let circle = builder.circle(
        [0.0, 0.0, 6.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        8.0,
        &vertex,
    );
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), 10.0);
    builder.face(
        &sphere,
        Sense::Forward,
        &[vec![(circle, Sense::Forward)]],
        true,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &Selection::default());
    let body = only_body(&report);

    let bbox = body.bbox().measured().expect("body box");
    assert_eq!(bbox.reading(), Some(Reading::Declared));
    assert_corners(bbox.value(), [-8.0, -8.0, 6.0], [8.0, 8.0, 10.0], 10.0);
    assert_corners(
        &candidate(bbox, Reading::Complement),
        [-10.0; 3],
        [10.0, 10.0, 6.0],
        10.0,
    );

    let centroid = body
        .area_centroid()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("area centroid");
    assert_eq!(centroid.reading(), Some(Reading::Declared));
    assert_point(centroid.value().value.get(), [0.0, 0.0, 8.0], 10.0);
    let other: Point = candidate(centroid, Reading::Complement);
    assert_point(other.value.get(), [0.0, 0.0, -2.0], 10.0);

    let document = report.document();
    let area = document
        .area()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("document area");
    assert_eq!(area.reading(), Some(Reading::Declared));
    assert_measured(document.area().as_ref().expect("area"), 80.0 * PI, 400.0);
    let complement = candidate(area, Reading::Complement).value.get();
    assert!((complement - 320.0 * PI).abs() <= TEST_ABSOLUTE_FLOOR * 400.0 * PI);
    let bbox = document
        .bbox()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("document box");
    assert_eq!(bbox.reading(), Some(Reading::Declared));
    assert_corners(
        &candidate(bbox, Reading::Complement),
        [-10.0; 3],
        [10.0, 10.0, 6.0],
        10.0,
    );
}

/// The plate `[0, 40]²` with a hole of diameter 20 at `(20, 20)`: area
/// `1600 − 100π`, boundary length `160 + 20π`, box diagonal `40√2`. Its
/// area centroid `(20, 20, 0)` moves by up to boundary × `τ` × diagonal /
/// area when the boundary moves by the linear tolerance `τ`.
#[test]
fn the_area_centroid_states_its_model_uncertainty() {
    let ir = plate_with_hole(true);
    let tau = ir.tolerances.linear.get();
    let report = measure(&ir, &Selection::default());
    let centroid = only_body(&report)
        .area_centroid()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("area centroid");
    assert_point(centroid.value().value.get(), [20.0, 20.0, 0.0], 40.0);
    let area = 1600.0 - 100.0 * PI;
    let expected = (160.0 + 20.0 * PI) * tau * 40.0 * SQRT_2 / area;
    let model = centroid.model_uncertainty().get();
    assert!(expected > 0.0);
    assert!(
        (model - expected).abs() <= TEST_ABSOLUTE_FLOOR * expected,
        "{model} against {expected}"
    );
}
