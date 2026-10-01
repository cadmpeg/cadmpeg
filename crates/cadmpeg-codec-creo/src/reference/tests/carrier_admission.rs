// SPDX-License-Identifier: Apache-2.0
use crate::reference::{
    ReferenceCircle, ReferenceCircleCenter, ReferenceEllipse, ReferenceLine, ReferenceLineKind,
};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::{PositiveLength, PositiveReal};
use cadmpeg_ir::units::UnitVector3;

fn point(value: [f64; 3]) -> FinitePoint3 {
    FinitePoint3::new(value.into()).expect("finite point")
}
fn radius(value: f64) -> PositiveLength {
    PositiveLength::new(value).expect("positive radius")
}

#[test]
fn reference_line_constructor_rejects_stored_length_disagreement() {
    let endpoints = [point([0.0; 3]), point([1.0, 0.0, 0.0])];
    let kind = |length| ReferenceLineKind::Line3d {
        entity_id: 1,
        original_length: PositiveReal::new(length).expect("positive length"),
    };
    assert!(ReferenceLine::try_new(kind(2.0), endpoints[0], endpoints[1], 0).is_none());
    let line =
        ReferenceLine::try_new(kind(1.0), endpoints[0], endpoints[1], 0).expect("matching length");
    assert_eq!(line.start(), endpoints[0]);
    assert_eq!(line.end(), endpoints[1]);
    assert_eq!(line.kind(), &kind(1.0));
}

#[test]
fn reference_circle_constructor_rejects_off_circle_endpoints() {
    assert!(ReferenceCircle::try_new(
        1,
        ReferenceCircleCenter::Stored(point([0.0; 3])),
        radius(1.0),
        UnitVector3::Z_AXIS,
        [point([2.0, 0.0, 0.0]), point([0.0, 2.0, 0.0])],
        0
    )
    .is_none());
}

#[test]
fn reference_circle_constructor_rejects_endpoints_outside_its_plane() {
    assert!(ReferenceCircle::try_new(
        1,
        ReferenceCircleCenter::Stored(point([0.0; 3])),
        radius(1.0),
        UnitVector3::Z_AXIS,
        [point([1.0, 0.0, 0.0]), point([0.0, 0.0, 1.0])],
        0
    )
    .is_none());
}

#[test]
fn reference_circle_diameter_admission_derives_the_endpoint_midpoint() {
    let endpoints = [point([3.0, 2.0, 4.0]), point([-1.0, 2.0, 4.0])];
    let circle = ReferenceCircle::try_new(
        1,
        ReferenceCircleCenter::Diameter,
        radius(2.0),
        UnitVector3::Z_AXIS,
        endpoints,
        0,
    )
    .expect("diameter circle");
    assert_eq!(circle.center(), point([1.0, 2.0, 4.0]));
    assert!(!circle.center_stored());
    assert_eq!([circle.start(), circle.end()], endpoints);
    assert!(ReferenceCircle::try_new(
        1,
        ReferenceCircleCenter::Diameter,
        radius(1.0),
        UnitVector3::Z_AXIS,
        endpoints,
        0
    )
    .is_none());
}

#[test]
fn reference_ellipse_constructor_rejects_parallel_frame() {
    assert!(ReferenceEllipse::try_new(
        1,
        point([0.0; 3]),
        UnitVector3::Z_AXIS,
        UnitVector3::Z_AXIS,
        [radius(2.0), radius(1.0)],
        0
    )
    .is_none());
}

#[test]
fn reference_ellipse_constructor_rejects_reversed_radius_order() {
    assert!(ReferenceEllipse::try_new(
        1,
        point([0.0; 3]),
        UnitVector3::Z_AXIS,
        UnitVector3::X_AXIS,
        [radius(1.0), radius(2.0)],
        0
    )
    .is_none());
}
