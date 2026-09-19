// SPDX-License-Identifier: Apache-2.0
use crate::{
    geometry::{
        analytic::{
            CircleCurve, ConeSurface, CylinderSurface, PlaneSurface, SphereSurface, TorusSurface,
        },
        CurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    },
    math::{Point3, Vector3},
};

#[test]
fn analytic_circle_numeric_admission_is_shared_by_constructor_and_serde() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    for radius in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        assert!(CircleCurve::try_new(center, axis, reference, radius).is_err());
    }
    assert!(CircleCurve::try_new(Point3::new(f64::NAN, 0.0, 0.0), axis, reference, 1.0).is_err());
    assert!(CircleCurve::try_new(center, Vector3::new(0.0, 0.0, 2.0), reference, 1.0).is_err());
    assert!(CircleCurve::try_new(center, axis, axis, 1.0).is_err());
    let wire = serde_json::json!({
        "kind": "circle",
        "center": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis": {"x": 0.0, "y": 0.0, "z": 1.0},
        "ref_direction": {"x": 1.0, "y": 0.0, "z": 0.0},
        "radius": 1.0
    });
    let curve: CurveGeometry = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(curve).unwrap(), wire);
    let mut invalid = wire.clone();
    invalid["radius"] = serde_json::json!(-1.0);
    let error = serde_json::from_value::<CurveGeometry>(invalid)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("radius") || error.contains("did not match"),
        "{error}"
    );
    let mut invalid = wire;
    invalid["ref_direction"] = serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0});
    let error = serde_json::from_value::<CurveGeometry>(invalid)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("frame") || error.contains("did not match"),
        "{error}"
    );
}

#[test]
fn analytic_surface_admission_preserves_signed_and_zero_radius_contracts() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let tiny = 1e-200;
    for radius in [tiny, -tiny] {
        let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(center, axis, reference, radius).unwrap(),
        ));
        let wire = serde_json::to_value(&sphere).unwrap();
        assert_eq!(
            serde_json::from_value::<SurfaceGeometry>(wire).unwrap(),
            sphere
        );
    }
    assert!(SphereSurface::try_new(center, axis, reference, 0.0).is_err());
    assert!(TorusSurface::try_new(center, axis, reference, tiny, -tiny).is_ok());
    assert!(TorusSurface::try_new(center, axis, reference, -tiny, tiny).is_err());
    assert!(ConeSurface::try_new(center, axis, reference, 0.0, 1.0, -0.5).is_ok());
    assert!(ConeSurface::try_new(center, axis, reference, -tiny, 1.0, 0.5).is_err());
    assert!(CylinderSurface::try_new(center, axis, reference, 0.0).is_err());
    assert!(PlaneSurface::try_new(center, Vector3::new(0.0, 0.0, 0.0), reference).is_err());
}
