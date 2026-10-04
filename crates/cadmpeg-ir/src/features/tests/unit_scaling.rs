// SPDX-License-Identifier: Apache-2.0
//! Feature scaling routes test only what a positive scale can break.

use crate::features::{
    FeatureEllipticArc, FinitePoint3, PrimitiveSolid, PrimitiveSolidKind, PrimitiveSolidScaleError,
};
use crate::geometry::DirectedParameterRange;
use crate::math::{Point3, Vector3};
use crate::scalar::{Angle, Length, PositiveLength, PositiveReal};

fn scale(value: f64) -> PositiveReal {
    PositiveReal::new(value).expect("a positive scale")
}

#[test]
fn a_scaled_point_is_refused_only_when_a_coordinate_overflows() {
    let point = FinitePoint3::new(Point3::new(1.0, -2.0, 0.5)).expect("a finite point");
    assert_eq!(
        point.scaled(scale(4.0)).map(FinitePoint3::get),
        Some(Point3::new(4.0, -8.0, 2.0))
    );
    let far = FinitePoint3::new(Point3::new(0.0, 0.0, -f64::MAX)).expect("a finite point");
    assert!(far.scaled(scale(2.0)).is_none());
}

fn arc(radii: [f64; 2]) -> FeatureEllipticArc {
    FeatureEllipticArc::new(
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        radii.map(|radius| PositiveLength::new(radius).expect("a positive radius")),
        DirectedParameterRange::new([0.0, 1.0]).expect("a nonzero interval"),
    )
    .expect("an ordered arc fixture")
}

/// Radii that round to one value keep the non-strict order; a major radius
/// that overflows or rounds to zero and a minor radius that rounds to zero
/// are refused. The center and the interval are kept.
#[test]
fn an_elliptic_arc_scales_its_radii_in_order() {
    let minor = 1.9_f64;
    let major = f64::from_bits(minor.to_bits() + 1);
    let scaled = arc([major, minor])
        .with_scaled_radii(scale(25.4))
        .expect("the radii keep their order");
    assert_eq!(scaled.radii().map(PositiveLength::get), [48.26, 48.26]);
    assert_eq!(scaled.center(), arc([major, minor]).center());
    assert_eq!(scaled.angles(), arc([major, minor]).angles());

    assert!(arc([f64::MAX, 1.0]).with_scaled_radii(scale(2.0)).is_none());
    assert!(arc([1.0e-320, 1.0e-320])
        .with_scaled_radii(scale(1.0e-10))
        .is_none());
    assert!(arc([1.0, 1.0e-320])
        .with_scaled_radii(scale(1.0e-10))
        .is_none());

    let moved = FinitePoint3::new(Point3::new(-5.0, 0.0, 7.5)).expect("a finite point");
    let recentered = arc([4.0, 2.0]).with_center(moved);
    assert_eq!(recentered.center(), moved);
    assert_eq!(recentered.radii(), arc([4.0, 2.0]).radii());
}

fn length(value: f64) -> Length {
    Length::new(value).expect("a finite length")
}

fn wedge(bounds: [f64; 10]) -> PrimitiveSolid {
    let [xmin, ymin, zmin, x2min, z2min, xmax, ymax, zmax, x2max, z2max] = bounds.map(length);
    PrimitiveSolid::new(PrimitiveSolidKind::Wedge {
        xmin,
        ymin,
        zmin,
        x2min,
        z2min,
        xmax,
        ymax,
        zmax,
        x2max,
        z2max,
    })
    .expect("a wedge fixture")
}

/// Every length is scaled and the angles are kept; a cone with one zero
/// radius and a wedge whose non-strict bounds are equal stay admitted.
#[test]
fn a_scaled_primitive_keeps_signs_non_strict_orders_and_angles() {
    let quarter = Angle::new(std::f64::consts::FRAC_PI_2).expect("a finite angle");
    let cone = PrimitiveSolid::new(PrimitiveSolidKind::Cone {
        radius1: length(0.0),
        radius2: length(2.0),
        height: length(3.0),
        angle: quarter,
    })
    .expect("a cone fixture");
    assert_eq!(
        cone.scaled(scale(25.4)),
        Ok(PrimitiveSolid::new(PrimitiveSolidKind::Cone {
            radius1: length(0.0),
            radius2: length(2.0 * 25.4),
            height: length(3.0 * 25.4),
            angle: quarter,
        })
        .expect("scaled cone fixture"))
    );

    let sphere = PrimitiveSolid::new(PrimitiveSolidKind::Sphere {
        radius: length(2.0),
        latitude1: Angle::new(-1.0).expect("a finite angle"),
        latitude2: Angle::new(1.0).expect("a finite angle"),
        longitude: quarter,
    })
    .expect("a sphere fixture");
    let Ok(scaled) = sphere.scaled(scale(0.5)) else {
        panic!("a scaled sphere stays admitted");
    };
    let PrimitiveSolidKind::Sphere {
        radius,
        latitude1,
        latitude2,
        longitude,
    } = scaled.kind()
    else {
        panic!("a scaled sphere stays a sphere");
    };
    assert_eq!(radius.get(), 1.0);
    assert_eq!([latitude1.get(), latitude2.get()], [-1.0, 1.0]);
    assert_eq!(longitude.get(), quarter.get());

    let equal_upper = wedge([0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0]);
    assert!(equal_upper.scaled(scale(3.0e-7)).is_ok());
}

/// A strict wedge extent whose bounds round to one value is refused with the
/// admission's own text, and a length that overflows is refused before the
/// dimensions are tested.
#[test]
fn a_scaled_primitive_refuses_a_collapsed_extent_and_an_overflowing_length() {
    let low = 1.9_f64;
    let high = f64::from_bits(low.to_bits() + 1);
    let thin = wedge([low, 0.0, 0.0, 0.0, 0.0, high, 1.0, 1.0, 0.0, 0.0]);
    assert_eq!(
        thin.scaled(scale(25.4)),
        Err(PrimitiveSolidScaleError::Admission(
            "primitive dimensions are invalid"
        ))
    );

    let tiny = PrimitiveSolid::new(PrimitiveSolidKind::Box {
        length: length(1.0e-320),
        width: length(1.0),
        height: length(1.0),
    })
    .expect("a box fixture");
    assert_eq!(
        tiny.scaled(scale(1.0e-10)),
        Err(PrimitiveSolidScaleError::Admission(
            "primitive dimensions are invalid"
        ))
    );

    let collapsed_and_huge = wedge([low, 0.0, 0.0, 0.0, 0.0, high, f64::MAX, 1.0, 0.0, 0.0]);
    assert_eq!(
        collapsed_and_huge.scaled(scale(25.4)),
        Err(PrimitiveSolidScaleError::NonFinite)
    );
}
