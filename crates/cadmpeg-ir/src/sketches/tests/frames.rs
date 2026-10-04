// SPDX-License-Identifier: Apache-2.0
use crate::math::{Point3, Vector3};

#[test]
fn a_sketch_frame_holds_its_admitted_origin_axes_and_pattern_direction() {
    use crate::features::{FinitePoint3, FiniteVector3};
    use crate::scalar::Length;
    use crate::sketches::{SketchPatternDirection, SketchPlacement};
    use crate::units::UnitVector2;

    let origin = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    let u_axis = Vector3::new(3.0, 0.0, 0.0);
    let placement = SketchPlacement::try_resolved(origin, normal, u_axis).unwrap();
    assert_eq!(
        placement.resolved(),
        Some((
            FinitePoint3::new(origin).unwrap(),
            FiniteVector3::new(normal).unwrap(),
            FiniteVector3::new(u_axis).unwrap(),
        ))
    );
    let moved = FinitePoint3::new(Point3::new(-4.0, 5.0, 6.0)).unwrap();
    assert_eq!(
        placement.with_origin(moved),
        SketchPlacement::try_resolved(moved.get(), normal, u_axis).unwrap()
    );
    assert_eq!(
        SketchPlacement::Unresolved {}.with_origin(moved),
        SketchPlacement::Unresolved {}
    );
    let direction =
        SketchPatternDirection::new([0.0, -1.0], Length::new(2.0).unwrap(), None, None).unwrap();
    assert_eq!(
        direction.direction(),
        UnitVector2::new([0.0, -1.0]).unwrap()
    );
}
