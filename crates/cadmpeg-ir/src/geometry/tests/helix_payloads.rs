use super::super::{
    HelixCircleProfile, HelixCurveConstruction, HelixLineProfile, HelixPathConstruction,
    HelixSurfaceConstruction, HelixSurfaceProfile, ProceduralCurveDefinition,
};
use crate::math::{Point3, Vector3};
use serde_json::json;

fn curve(radius: f64) -> HelixCurveConstruction {
    HelixCurveConstruction::try_new(
        [0.0, 1.0],
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(radius, 0.0, 0.0),
        Vector3::new(0.0, radius, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        0.0,
        Vector3::new(0.0, 0.0, 1.0),
    )
    .unwrap()
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
        assert!(
            HelixCurveConstruction::try_new(range, center, major, minor, pitch, apex, axis)
                .is_err()
        );
    }
    assert!(
        HelixCurveConstruction::try_new([0.0, 0.0], center, major, minor, pitch, apex, axis)
            .is_ok()
    );
    for vector in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::EPSILON, 0.0, 0.0),
    ] {
        assert!(
            HelixCurveConstruction::try_new(range, center, vector, minor, pitch, apex, axis)
                .is_err()
        );
        assert!(
            HelixCurveConstruction::try_new(range, center, major, minor, pitch, apex, vector)
                .is_err()
        );
    }
    assert!(HelixCurveConstruction::try_new(
        range,
        center,
        major,
        Vector3::new(0.0, 2.0, 0.0),
        pitch,
        apex,
        axis
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
    value.reverse_parameterization();
    assert_eq!(*value.angle_range(), [-1.0, 0.0]);
    value.reverse_parameterization();
    assert_eq!(value, old);
    value.try_scale_lengths(2.0).unwrap();
    assert_eq!(*value.major(), Vector3::new(2.0, 0.0, 0.0));
    let mut huge = curve(1.0e200);
    let old = huge;
    assert!(huge.try_scale_lengths(1.0e200).is_err());
    assert_eq!(huge, old);
}

#[test]
fn helix_surface_and_curve_keep_distinct_radius_tolerances() {
    const RADIUS_DIFFERENCE: f64 = 5.0e-7;
    let center = Point3::new(0.0, 0.0, 0.0);
    let major = Vector3::new(1000.0, 0.0, 0.0);
    let minor = Vector3::new(0.0, 1000.0 + RADIUS_DIFFERENCE, 0.0);
    let pitch = Vector3::new(0.0, 0.0, 1.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    assert!(
        HelixPathConstruction::try_new([0.0, 1.0], center, major, minor, pitch, 0.0, axis).is_ok()
    );
    assert!(
        HelixCurveConstruction::try_new([0.0, 1.0], center, major, minor, pitch, 0.0, axis)
            .is_err()
    );
    assert!(HelixPathConstruction::try_new(
        [0.0, 1.0],
        center,
        major,
        Vector3::new(0.0, 1001.0, 0.0),
        pitch,
        0.0,
        axis
    )
    .is_err());
}

#[test]
fn helix_surface_preserves_finite_directed_ranges_and_signed_profiles() {
    let path = HelixPathConstruction::try_new(
        [1.0, -1.0],
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(2.0, 0.0, 0.0),
        Vector3::new(0.0, 2.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        0.0,
        Vector3::new(0.0, 0.0, 0.0),
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
