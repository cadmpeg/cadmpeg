// SPDX-License-Identifier: Apache-2.0
use crate::{
    geometry::{
        analytic::{
            CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve,
            HyperbolaCurve, LineCurve, ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
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

#[test]
fn analytic_surfaces_rebuild_from_their_checked_getters() {
    let point = Point3::new(1.0, 2.0, 3.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);

    let plane = PlaneSurface::try_new(point, axis, reference).unwrap();
    assert_eq!(PlaneSurface::new(plane.origin(), plane.frame()), plane);

    let cylinder = CylinderSurface::try_new(point, axis, reference, 2.0).unwrap();
    assert_eq!(
        CylinderSurface::new(cylinder.origin(), cylinder.frame(), cylinder.radius()),
        cylinder
    );

    let cone = ConeSurface::try_new(point, axis, reference, 3.0, 0.5, -0.25).unwrap();
    assert_eq!(
        ConeSurface::new(
            cone.origin(),
            cone.frame(),
            cone.radius(),
            cone.ratio(),
            cone.half_angle(),
        ),
        cone
    );

    let sphere = SphereSurface::try_new(point, axis, reference, -4.0).unwrap();
    assert_eq!(
        SphereSurface::new(sphere.center(), sphere.frame(), sphere.radius()),
        sphere
    );

    let torus = TorusSurface::try_new(point, axis, reference, 5.0, -1.0).unwrap();
    assert_eq!(
        TorusSurface::new(
            torus.center(),
            torus.frame(),
            torus.major_radius(),
            torus.minor_radius(),
        ),
        torus
    );

    for (rebuilt, original) in [
        (
            serde_json::to_value(PlaneSurface::new(plane.origin(), plane.frame())).unwrap(),
            serde_json::to_value(plane).unwrap(),
        ),
        (
            serde_json::to_value(TorusSurface::new(
                torus.center(),
                torus.frame(),
                torus.major_radius(),
                torus.minor_radius(),
            ))
            .unwrap(),
            serde_json::to_value(torus).unwrap(),
        ),
    ] {
        assert_eq!(rebuilt, original);
    }
}

#[test]
fn a_surface_frame_hands_back_its_admitted_directions() {
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let plane = PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0), axis, reference).unwrap();
    let frame = plane.frame();
    assert_eq!(frame.unit_axis().as_raw(), plane.normal());
    assert_eq!(frame.unit_reference().as_raw(), plane.u_axis());
    assert_eq!(
        frame.unit_axis(),
        crate::units::UnitVector3::new(axis).unwrap()
    );
    assert_eq!(
        frame.unit_reference(),
        crate::units::UnitVector3::new(reference).unwrap()
    );
}

#[test]
fn surface_radius_admission_refuses_both_signed_zeros_and_keeps_a_negative_cone_zero() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    for zero in [0.0, -0.0] {
        assert!(SphereSurface::try_new(center, axis, reference, zero).is_err());
        assert!(TorusSurface::try_new(center, axis, reference, 1.0, zero).is_err());
        assert!(CylinderSurface::try_new(center, axis, reference, zero).is_err());
    }
    let cone = ConeSurface::try_new(center, axis, reference, -0.0, 1.0, 0.0).unwrap();
    assert_eq!(cone.radius().get().to_bits(), (-0.0_f64).to_bits());
    let wire = serde_json::to_value(cone).unwrap();
    assert_eq!(
        serde_json::from_value::<ConeSurface>(wire)
            .unwrap()
            .radius()
            .get()
            .to_bits(),
        (-0.0_f64).to_bits()
    );
}

#[test]
fn surface_admission_names_the_refused_component() {
    let finite = Point3::new(0.0, 0.0, 0.0);
    let nonfinite = Point3::new(f64::NAN, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);

    assert_eq!(
        SphereSurface::try_new(finite, axis, reference, 0.0).unwrap_err(),
        "SphereSurface.radius must be finite and nonzero"
    );
    assert_eq!(
        SphereSurface::try_new(finite, axis, reference, f64::INFINITY).unwrap_err(),
        "SphereSurface.radius must be finite and nonzero"
    );
    assert_eq!(
        SphereSurface::try_new(nonfinite, axis, reference, 0.0).unwrap_err(),
        "SphereSurface.center must be finite"
    );
    assert_eq!(
        SphereSurface::try_new(finite, axis, axis, 0.0).unwrap_err(),
        "SphereSurface.axis/ref_direction must form an orthonormal frame"
    );

    assert_eq!(
        TorusSurface::try_new(finite, axis, reference, 1.0, 0.0).unwrap_err(),
        "TorusSurface.minor_radius must be finite and nonzero"
    );
    assert_eq!(
        TorusSurface::try_new(finite, axis, reference, 0.0, 0.0).unwrap_err(),
        "TorusSurface.major_radius must be positive and finite"
    );
    assert_eq!(
        TorusSurface::try_new(nonfinite, axis, reference, 1.0, 0.0).unwrap_err(),
        "TorusSurface.center must be finite"
    );

    assert_eq!(
        CylinderSurface::try_new(finite, axis, reference, 0.0).unwrap_err(),
        "CylinderSurface.radius must be positive and finite"
    );
    assert_eq!(
        ConeSurface::try_new(finite, axis, reference, -1.0, 1.0, 0.0).unwrap_err(),
        "ConeSurface.radius must be nonnegative and finite"
    );
    assert_eq!(
        PlaneSurface::try_new(nonfinite, axis, reference).unwrap_err(),
        "PlaneSurface.origin must be finite"
    );
}

#[test]
fn analytic_curves_rebuild_from_their_checked_getters() {
    let point = Point3::new(1.0, 2.0, 3.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);

    let line = LineCurve::try_new(point, axis).unwrap();
    assert_eq!(LineCurve::new(line.origin(), line.direction()), line);

    let circle = CircleCurve::try_new(point, axis, reference, 2.0).unwrap();
    assert_eq!(
        CircleCurve::new(circle.center(), circle.frame(), circle.radius()),
        circle
    );

    let ellipse = EllipseCurve::try_new(point, axis, reference, 4.0, 2.0).unwrap();
    assert_eq!(
        EllipseCurve::try_from_parts(
            ellipse.center(),
            ellipse.frame(),
            ellipse.major_radius(),
            ellipse.minor_radius(),
        )
        .unwrap(),
        ellipse
    );

    let parabola = ParabolaCurve::try_new(point, axis, reference, 0.5).unwrap();
    assert_eq!(
        ParabolaCurve::new(
            parabola.vertex(),
            parabola.frame(),
            parabola.focal_distance(),
        ),
        parabola
    );

    let hyperbola = HyperbolaCurve::try_new(point, axis, reference, 2.0, 5.0).unwrap();
    assert_eq!(
        HyperbolaCurve::new(
            hyperbola.center(),
            hyperbola.frame(),
            hyperbola.major_radius(),
            hyperbola.minor_radius(),
        ),
        hyperbola
    );

    let degenerate = DegenerateCurve::try_new(point).unwrap();
    assert_eq!(DegenerateCurve::new(degenerate.point()), degenerate);

    for (rebuilt, original) in [
        (
            serde_json::to_value(CircleCurve::new(
                circle.center(),
                circle.frame(),
                circle.radius(),
            ))
            .unwrap(),
            serde_json::to_value(circle).unwrap(),
        ),
        (
            serde_json::to_value(
                EllipseCurve::try_from_parts(
                    ellipse.center(),
                    ellipse.frame(),
                    ellipse.major_radius(),
                    ellipse.minor_radius(),
                )
                .unwrap(),
            )
            .unwrap(),
            serde_json::to_value(ellipse).unwrap(),
        ),
    ] {
        assert_eq!(rebuilt, original);
    }
}

#[test]
fn a_curve_frame_direction_builds_a_line_without_readmission() {
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let circle = CircleCurve::try_new(Point3::new(1.0, 2.0, 3.0), axis, reference, 2.0).unwrap();
    let generatrix = LineCurve::new(circle.center(), circle.frame().unit_axis());
    assert_eq!(generatrix.origin(), circle.center());
    assert_eq!(generatrix.direction().as_raw(), circle.axis());
    assert_eq!(
        generatrix.direction(),
        crate::units::UnitVector3::new(axis).unwrap()
    );
    assert_eq!(
        DegenerateCurve::new(generatrix.origin()).point(),
        circle.center()
    );
}

#[test]
fn ellipse_ordering_is_enforced_through_raw_and_checked_construction() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let ordered = "EllipseCurve.major_radius must be at least minor_radius";

    assert_eq!(
        EllipseCurve::try_new(center, axis, reference, 2.0, 4.0).unwrap_err(),
        ordered
    );
    let equal = EllipseCurve::try_new(center, axis, reference, 3.0, 3.0).unwrap();
    assert_eq!(equal.major_radius().get(), equal.minor_radius().get());

    let ellipse = EllipseCurve::try_new(center, axis, reference, 4.0, 2.0).unwrap();
    assert_eq!(
        EllipseCurve::try_from_parts(
            ellipse.center(),
            ellipse.frame(),
            ellipse.minor_radius(),
            ellipse.major_radius(),
        )
        .unwrap_err(),
        ordered
    );
    assert!(HyperbolaCurve::try_new(center, axis, reference, 2.0, 4.0).is_ok());
}

#[test]
fn curve_admission_names_the_refused_component() {
    let finite = Point3::new(0.0, 0.0, 0.0);
    let nonfinite = Point3::new(f64::NAN, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);

    assert_eq!(
        LineCurve::try_new(nonfinite, axis).unwrap_err(),
        "LineCurve.origin must be finite"
    );
    assert_eq!(
        LineCurve::try_new(finite, Vector3::new(0.0, 0.0, 2.0)).unwrap_err(),
        "LineCurve.direction must have unit length"
    );

    assert_eq!(
        CircleCurve::try_new(finite, axis, axis, 1.0).unwrap_err(),
        "CircleCurve.axis/ref_direction must form an orthonormal frame"
    );
    assert_eq!(
        CircleCurve::try_new(nonfinite, axis, reference, 0.0).unwrap_err(),
        "CircleCurve.center must be finite"
    );
    for radius in [0.0, -0.0, -1.0, f64::INFINITY] {
        assert_eq!(
            CircleCurve::try_new(finite, axis, reference, radius).unwrap_err(),
            "CircleCurve.radius must be positive and finite"
        );
    }

    assert_eq!(
        EllipseCurve::try_new(finite, axis, axis, 2.0, 4.0).unwrap_err(),
        "EllipseCurve.axis/major_direction must form an orthonormal frame"
    );
    assert_eq!(
        EllipseCurve::try_new(nonfinite, axis, reference, 2.0, 4.0).unwrap_err(),
        "EllipseCurve.center must be finite"
    );
    assert_eq!(
        EllipseCurve::try_new(finite, axis, reference, -1.0, 2.0).unwrap_err(),
        "EllipseCurve.major_radius must be positive and finite"
    );
    assert_eq!(
        EllipseCurve::try_new(finite, axis, reference, 2.0, 0.0).unwrap_err(),
        "EllipseCurve.minor_radius must be positive and finite"
    );

    assert_eq!(
        ParabolaCurve::try_new(nonfinite, axis, reference, 1.0).unwrap_err(),
        "ParabolaCurve.vertex must be finite"
    );
    assert_eq!(
        ParabolaCurve::try_new(finite, axis, reference, 0.0).unwrap_err(),
        "ParabolaCurve.focal_distance must be positive and finite"
    );

    assert_eq!(
        HyperbolaCurve::try_new(nonfinite, axis, reference, 1.0, 1.0).unwrap_err(),
        "HyperbolaCurve.center must be finite"
    );
    assert_eq!(
        HyperbolaCurve::try_new(finite, axis, reference, 0.0, 1.0).unwrap_err(),
        "HyperbolaCurve.major_radius must be positive and finite"
    );
    assert_eq!(
        HyperbolaCurve::try_new(finite, axis, reference, 1.0, -1.0).unwrap_err(),
        "HyperbolaCurve.minor_radius must be positive and finite"
    );

    assert_eq!(
        DegenerateCurve::try_new(nonfinite).unwrap_err(),
        "DegenerateCurve.point must be finite"
    );
}
