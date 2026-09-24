// SPDX-License-Identifier: Apache-2.0
//! Unit scaling of solved carriers tests only what scaling can break.

use super::ScaleRefusal;
use crate::geometry::analytic::{ConeSurface, EllipseCurve, PlaneSurface};
use crate::geometry::sampled::{
    GeometryLayoutError, PolygonalSurface, PolylineCurve, PolylineSamples, PolylineVertex,
};
use crate::geometry::{
    PlacedSurface, SolvedCurveGeometry, SolvedSurfaceGeometry, MAX_GEOMETRY_NESTING,
};
use crate::math::{Point3, Vector3};
use crate::scalar::PositiveReal;
use crate::transform::Transform;

fn scale(value: f64) -> PositiveReal {
    PositiveReal::new(value).expect("a positive scale fixture")
}

fn ellipse(major: f64, minor: f64) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            major,
            minor,
        )
        .expect("an ordered ellipse fixture"),
    )
}

fn scaled_radii(geometry: &SolvedCurveGeometry) -> [f64; 2] {
    let SolvedCurveGeometry::Ellipse(ellipse) = geometry else {
        panic!("a scaled ellipse stays an ellipse");
    };
    [ellipse.major_radius().get(), ellipse.minor_radius().get()]
}

/// The major radius is the successor of the minor one, and both round to one
/// millimetre value at the inch scale. The order is not strict, so the
/// ellipse with equal radii is admitted; rounding is monotone, so scaled
/// radii never reverse.
#[test]
fn radii_that_round_to_one_value_keep_the_ellipse_order() {
    let minor = 1.9_f64;
    let major = f64::from_bits(minor.to_bits() + 1);
    let scaled = ellipse(major, minor)
        .scaled(scale(25.4))
        .expect("the scaled radii keep their order");
    assert_eq!(scaled_radii(&scaled), [48.26, 48.26]);

    for scale_value in [25.4, 1000.0, 0.1, 0.0254, 3.0e-7] {
        let mut minor = 1.0e-3_f64;
        for _ in 0..4096 {
            let major = f64::from_bits(minor.to_bits() + 1);
            let [scaled_major, scaled_minor] = scaled_radii(
                &ellipse(major, minor)
                    .scaled(scale(scale_value))
                    .expect("a positive scale keeps the order of two admitted radii"),
            );
            assert!(scaled_minor <= scaled_major);
            minor = major;
        }
    }
}

#[test]
fn a_scaled_ellipse_refuses_each_field_with_its_own_text_in_order() {
    let center = SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(
            Point3::new(f64::MAX, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            f64::MAX,
            1.0,
        )
        .expect("an ordered ellipse fixture"),
    );
    let cases = [
        (center, 2.0, "EllipseCurve.center must be finite"),
        (
            ellipse(f64::MAX, 1.0),
            2.0,
            "EllipseCurve.major_radius must be positive and finite",
        ),
        (
            ellipse(1.0e-320, 1.0e-320),
            1.0e-10,
            "EllipseCurve.major_radius must be positive and finite",
        ),
        (
            ellipse(1.0, 1.0e-320),
            1.0e-10,
            "EllipseCurve.minor_radius must be positive and finite",
        ),
    ];
    for (geometry, scale_value, text) in cases {
        assert_eq!(
            geometry.scaled(scale(scale_value)),
            Err(ScaleRefusal::Field(text))
        );
    }
}

fn plane() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("a unit-axis plane"),
    )
}

/// A chain at the admitted depth stays at it: scaling keeps every placement,
/// so one more placement over the scaled chain is refused as over the
/// original chain.
#[test]
fn a_scaled_placement_chain_keeps_its_nesting_depth() {
    let translation = Transform::affine([
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 2.0],
        [0.0, 0.0, 1.0, 3.0],
    ])
    .expect("a translation fixture");
    let mut chain = plane();
    for _ in 0..MAX_GEOMETRY_NESTING {
        chain = SolvedSurfaceGeometry::Transformed(
            PlacedSurface::try_new(Box::new(chain), translation)
                .expect("the chain is within the bound"),
        );
    }
    let scaled = chain.scaled(scale(25.4)).expect("finite scaled chain");
    assert_eq!(scaled.nesting_depth(), MAX_GEOMETRY_NESTING);
    assert!(PlacedSurface::try_new(Box::new(scaled.clone()), translation).is_err());

    let SolvedSurfaceGeometry::Transformed(outer) = &scaled else {
        panic!("a scaled placement stays a placement");
    };
    assert_eq!(
        outer.transform().affine_rows().map(|row| row[3]),
        [25.4, 2.0 * 25.4, 3.0 * 25.4]
    );
    let mut leaf = &scaled;
    while let SolvedSurfaceGeometry::Transformed(placed) = leaf {
        leaf = placed.basis();
    }
    let SolvedSurfaceGeometry::Plane(plane) = leaf else {
        panic!("the leaf stays a plane");
    };
    assert_eq!(
        plane.origin().get(),
        Point3::new(25.4, 2.0 * 25.4, 3.0 * 25.4)
    );
}

#[test]
fn a_scaled_placement_refuses_its_basis_before_its_translation() {
    let overflowing = Transform::affine([
        [1.0, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("a finite translation fixture");
    let translation_only = SolvedSurfaceGeometry::Transformed(
        PlacedSurface::try_new(Box::new(plane()), overflowing).expect("a placement fixture"),
    );
    assert_eq!(
        translation_only.scaled(scale(2.0)),
        Err(ScaleRefusal::Translation)
    );

    let far_plane = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(f64::MAX, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("a finite plane fixture"),
    );
    let both = SolvedSurfaceGeometry::Transformed(
        PlacedSurface::try_new(Box::new(far_plane), overflowing).expect("a placement fixture"),
    );
    assert_eq!(
        both.scaled(scale(2.0)),
        Err(ScaleRefusal::Field("PlaneSurface.origin must be finite"))
    );
}

/// A cone radius is nonnegative, and a positive scale keeps a zero radius at
/// zero; only a radius that overflows is refused.
#[test]
fn a_scaled_cone_keeps_a_zero_radius_and_refuses_only_overflow() {
    let cone = |radius: f64| {
        SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
                1.0,
                0.5,
            )
            .expect("a cone fixture"),
        )
    };
    let SolvedSurfaceGeometry::Cone(scaled) = cone(0.0)
        .scaled(scale(1.0e-10))
        .expect("a zero radius stays admitted")
    else {
        panic!("a scaled cone stays a cone");
    };
    assert_eq!(scaled.radius().get(), 0.0);
    assert_eq!(
        cone(f64::MAX).scaled(scale(2.0)),
        Err(ScaleRefusal::Field(
            "ConeSurface.radius must be nonnegative and finite"
        ))
    );
}

/// A scaled polyline keeps its source parameters and refuses a point before
/// a chordal deviation, each only when it overflows.
#[test]
fn a_scaled_polyline_keeps_its_parameters_and_refuses_only_overflow() {
    let polyline = |point: f64, deflection: f64| {
        SolvedCurveGeometry::Polyline(
            PolylineCurve::new(
                PolylineSamples::Parameterized {
                    vertices: crate::features::NonEmptyMembers::try_from(vec![
                        PolylineVertex {
                            parameter: 3.0,
                            point: Point3::new(point, 0.0, 0.0),
                        },
                        PolylineVertex {
                            parameter: 1.0,
                            point: Point3::new(0.0, 1.0, 0.0),
                        },
                    ])
                    .expect("two samples"),
                },
                deflection,
            )
            .expect("a polyline fixture"),
        )
    };
    let SolvedCurveGeometry::Polyline(scaled) = polyline(1.0, 0.0)
        .scaled(scale(25.4))
        .expect("finite scaled samples")
    else {
        panic!("a scaled polyline stays a polyline");
    };
    assert_eq!(
        scaled.parameters().map(|parameters| parameters
            .map(crate::scalar::FiniteReal::get)
            .collect::<Vec<_>>()),
        Some(vec![3.0, 1.0])
    );
    assert_eq!(
        scaled
            .points()
            .map(crate::features::FinitePoint3::get)
            .collect::<Vec<_>>(),
        vec![Point3::new(25.4, 0.0, 0.0), Point3::new(0.0, 25.4, 0.0)]
    );
    assert_eq!(scaled.chordal_deflection().get(), 0.0);

    let refusal = |message: &str| {
        Err(ScaleRefusal::Samples(GeometryLayoutError::Layout(
            message.to_string(),
        )))
    };
    assert_eq!(
        polyline(f64::MAX, f64::MAX).scaled(scale(2.0)),
        refusal("points must be finite")
    );
    assert_eq!(
        polyline(1.0, f64::MAX).scaled(scale(2.0)),
        refusal("chordal_deflection must be finite and non-negative")
    );
}

/// A scaled polygonal surface keeps its triangles and refuses a vertex
/// before a chordal deviation, each only when it overflows.
#[test]
fn a_scaled_polygonal_surface_keeps_its_triangles_and_refuses_only_overflow() {
    let surface = |x: f64, deflection: f64| {
        SolvedSurfaceGeometry::Polygonal(
            PolygonalSurface::new(
                vec![
                    Point3::new(x, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(0.0, 0.0, 1.0),
                ],
                vec![[0, 1, 2]],
                deflection,
            )
            .expect("a polygonal fixture"),
        )
    };
    assert_eq!(
        surface(1.0, 0.0).scaled(scale(25.4)),
        Ok(SolvedSurfaceGeometry::Polygonal(
            PolygonalSurface::new(
                vec![
                    Point3::new(25.4, 0.0, 0.0),
                    Point3::new(0.0, 25.4, 0.0),
                    Point3::new(0.0, 0.0, 25.4),
                ],
                vec![[0, 1, 2]],
                0.0,
            )
            .expect("a polygonal fixture"),
        ))
    );
    let refusal = |message: &str| {
        Err(ScaleRefusal::Samples(GeometryLayoutError::Layout(
            message.to_string(),
        )))
    };
    assert_eq!(
        surface(f64::MAX, f64::MAX).scaled(scale(2.0)),
        refusal("vertices must be finite")
    );
    assert_eq!(
        surface(1.0, f64::MAX).scaled(scale(2.0)),
        refusal("chordal_deflection must be finite and non-negative")
    );
}
