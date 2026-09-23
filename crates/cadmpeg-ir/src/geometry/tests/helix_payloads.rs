use super::super::{
    HelixCircleProfile, HelixCurveConstruction, HelixFrame, HelixLineProfile,
    HelixPathConstruction, HelixSurfaceConstruction, HelixSurfaceProfile,
    ProceduralCurveDefinition,
};
use crate::math::{Point3, Vector3};
use serde_json::json;

fn curve(radius: f64) -> HelixCurveConstruction {
    HelixCurveConstruction::try_new(
        [0.0, 1.0],
        HelixFrame {
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(radius, 0.0, 0.0),
            minor: Vector3::new(0.0, radius, 0.0),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        },
        0.0,
        None,
    )
    .unwrap()
}

#[test]
fn numerical_audit_helix_curve_rejects_overflowing_unequal_radii() {
    let maximum = f64::MAX;
    assert!(HelixCurveConstruction::try_new(
        [0.0, 1.0],
        HelixFrame {
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(maximum, maximum, 0.0),
            minor: Vector3::new(maximum, maximum, maximum),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        },
        0.0,
        None,
    )
    .is_err());
}

#[test]
fn numerical_audit_helix_line_admits_finite_nonzero_small_directions() {
    for magnitude in [1e-200, f64::from_bits(1)] {
        assert!(HelixLineProfile::try_new(Vector3::new(magnitude, 0.0, 0.0)).is_ok());
    }
}

#[test]
fn helix_curve_admission_rejects_the_validator_numeric_states() {
    let valid = curve(1.0);
    let range = *valid.angle_range();
    let center = *valid.center();
    let major = *valid.major();
    let minor = *valid.minor();
    let pitch = *valid.pitch();
    let apex = valid.apex_factor();
    let axis = *valid.axis();
    for range in [[1.0, 0.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(HelixCurveConstruction::try_new(
            range,
            HelixFrame {
                center,
                major,
                minor,
                pitch,
                axis
            },
            apex,
            None
        )
        .is_err());
    }
    assert!(HelixCurveConstruction::try_new(
        [0.0, 0.0],
        HelixFrame {
            center,
            major,
            minor,
            pitch,
            axis
        },
        apex,
        None
    )
    .is_ok());
    for vector in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::EPSILON, 0.0, 0.0),
    ] {
        assert!(HelixCurveConstruction::try_new(
            range,
            HelixFrame {
                center,
                major: vector,
                minor,
                pitch,
                axis
            },
            apex,
            None
        )
        .is_err());
        assert!(HelixCurveConstruction::try_new(
            range,
            HelixFrame {
                center,
                major,
                minor,
                pitch,
                axis: vector
            },
            apex,
            None
        )
        .is_err());
    }
    assert!(HelixCurveConstruction::try_new(
        range,
        HelixFrame {
            center,
            major,
            minor: Vector3::new(0.0, 2.0, 0.0),
            pitch,
            axis
        },
        apex,
        None
    )
    .is_err());
    let wire = serde_json::to_value(ProceduralCurveDefinition::Helix(valid)).unwrap();
    assert_eq!(wire["kind"], "helix");
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        ProceduralCurveDefinition::Helix(valid)
    );
    for (field, invalid) in [
        ("angle_range", json!([1.0, 0.0])),
        ("axis", json!({"x": 0.0, "y": 0.0, "z": 0.0})),
    ] {
        let mut bad = wire.clone();
        bad[field] = invalid;
        assert!(serde_json::from_value::<ProceduralCurveDefinition>(bad).is_err());
    }
}

#[test]
fn helix_curve_scaling_is_atomic_and_reversal_preserves_admission() {
    let mut value = curve(1.0);
    let old = value;
    for scale in [0.0, f64::INFINITY, f64::NAN] {
        assert!(value.try_scale_lengths(scale).is_err());
        assert_eq!(value, old);
    }
    value.try_reverse_parameterization().unwrap();
    assert_eq!(*value.angle_range(), [-1.0, 0.0]);
    value.try_reverse_parameterization().unwrap();
    assert_eq!(value, old);
    value.try_scale_lengths(2.0).unwrap();
    assert_eq!(*value.major(), Vector3::new(2.0, 0.0, 0.0));
    let mut huge = curve(1.0e200);
    let old = huge;
    assert!(huge.try_scale_lengths(1.0e200).is_err());
    assert_eq!(huge, old);
}

#[test]
fn helix_reversal_refuses_a_zero_radius_at_the_new_start_atomically() {
    let mut value = HelixCurveConstruction::try_new(
        [0.0, std::f64::consts::TAU],
        HelixFrame {
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(1.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 1.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        },
        -1.0,
        None,
    )
    .unwrap();
    let original = value;
    assert!(value.try_reverse_parameterization().is_err());
    assert_eq!(value, original);
}

#[test]
fn helix_surface_and_curve_keep_distinct_radius_tolerances() {
    const RADIUS_DIFFERENCE: f64 = 5.0e-7;
    let center = Point3::new(0.0, 0.0, 0.0);
    let major = Vector3::new(1000.0, 0.0, 0.0);
    let minor = Vector3::new(0.0, 1000.0 + RADIUS_DIFFERENCE, 0.0);
    let pitch = Vector3::new(0.0, 0.0, 1.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    assert!(HelixPathConstruction::try_new(
        [0.0, 1.0],
        HelixFrame {
            center,
            major,
            minor,
            pitch,
            axis
        },
        0.0
    )
    .is_ok());
    assert!(HelixCurveConstruction::try_new(
        [0.0, 1.0],
        HelixFrame {
            center,
            major,
            minor,
            pitch,
            axis
        },
        0.0,
        None
    )
    .is_err());
    assert!(HelixPathConstruction::try_new(
        [0.0, 1.0],
        HelixFrame {
            center,
            major,
            minor: Vector3::new(0.0, 1001.0, 0.0),
            pitch,
            axis
        },
        0.0
    )
    .is_err());
}

#[test]
fn helix_surface_preserves_finite_directed_ranges_and_signed_profiles() {
    let path = HelixPathConstruction::try_new(
        [1.0, -1.0],
        HelixFrame {
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(2.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 2.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            axis: Vector3::new(0.0, 0.0, 0.0),
        },
        0.0,
    )
    .unwrap();
    let profile = HelixSurfaceProfile::Circle(HelixCircleProfile::try_new(-1.0, -2.0).unwrap());
    let surface =
        HelixSurfaceConstruction::try_new([1.0, -1.0], [2.0, -2.0], path, profile).unwrap();
    let wire = serde_json::to_value(surface).unwrap();
    assert_eq!(
        wire["profile"],
        json!({"kind": "circle", "length": -1.0, "radius": -2.0})
    );
    assert_eq!(
        serde_json::from_value::<HelixSurfaceConstruction>(wire.clone()).unwrap(),
        surface
    );
    let mut bad = wire;
    bad["profile"]["radius"] = json!(0.0);
    assert!(serde_json::from_value::<HelixSurfaceConstruction>(bad).is_err());
    assert!(HelixCircleProfile::try_new(1.0, 0.0).is_err());
    assert!(HelixCircleProfile::try_new(f64::INFINITY, 1.0).is_err());
    assert!(HelixLineProfile::try_new(Vector3::new(0.0, 0.0, 0.0)).is_err());
    assert!(HelixLineProfile::try_new(Vector3::new(2.0, 0.0, 0.0)).is_ok());
}

#[test]
fn numerical_followup_circular_helix_admits_finite_radial_scales() {
    use crate::geometry::{HelixFrame, HelixPathConstruction};
    use crate::math::{Point3, Vector3};
    for radius in [1.0, 1e200, 1e-200] {
        let frame = HelixFrame {
            center: Point3::new(0., 0., 0.),
            major: Vector3::new(radius, 0., 0.),
            minor: Vector3::new(0., radius, 0.),
            pitch: Vector3::new(0., 0., 1.),
            axis: Vector3::new(0., 0., 1.),
        };
        assert!(HelixPathConstruction::try_new([0., 1.], frame, 0.).is_ok());
    }
}

#[test]
fn numerical_followup_helix_path_requires_two_equal_nonzero_radii() {
    for radius in [1e-200, 1e-10, 1.0, 1e200] {
        for ratio in [0.0, 0.5, 1.0] {
            let result = HelixPathConstruction::try_new(
                [0., 1.],
                HelixFrame {
                    center: Point3::new(0., 0., 0.),
                    major: Vector3::new(radius, 0., 0.),
                    minor: Vector3::new(0., ratio * radius, 0.),
                    pitch: Vector3::new(0., 0., 1.),
                    axis: Vector3::new(0., 0., 1.),
                },
                0.,
            );
            assert_eq!(result.is_ok(), ratio == 1.);
        }
    }
}

#[test]
fn helix_curve_finite_center_returns_the_admitted_center() {
    use crate::features::FinitePoint3;

    for center in [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(-0.0, 1.5, -2.0e300),
        Point3::new(f64::MAX, 5.0e-324, -f64::MAX),
    ] {
        let helix = HelixCurveConstruction::try_new(
            [0.0, 1.0],
            HelixFrame {
                center,
                major: Vector3::new(1.0, 0.0, 0.0),
                minor: Vector3::new(0.0, 1.0, 0.0),
                pitch: Vector3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            0.0,
            None,
        )
        .unwrap();
        let finite = helix.finite_center();
        assert_eq!(
            [finite.x, finite.y, finite.z].map(f64::to_bits),
            [center.x, center.y, center.z].map(f64::to_bits)
        );
        assert_eq!(finite.as_raw(), helix.center());
        assert_eq!(FinitePoint3::new(center), Some(finite));
    }
}
