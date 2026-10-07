// SPDX-License-Identifier: Apache-2.0
//! Unit scaling of solved carriers tests only what scaling can break.

use super::ScaleRefusal;
use crate::geometry::analytic::{ConeSurface, EllipseCurve, LineCurve, PlaneSurface};
use crate::geometry::sampled::{
    GeometryLayoutError, PolygonalSurface, PolylineCurve, PolylineSamples, PolylineVertex,
};
use crate::geometry::{
    PlacedCurve, PlacedSurface, SolvedCurveGeometry, SolvedSurfaceGeometry, MAX_GEOMETRY_NESTING,
};
use crate::math::{Point3, Vector3};
use crate::scalar::PositiveReal;
use crate::transform::Transform;

const SCALE_TO_TINY_LENGTH: f64 = 1.0e-10;

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
        .scaled(&cadmpeg_test_support::service_decode_context(), scale(25.4))
        .expect("fixture scaling admission")
        .expect("the scaled radii keep their order");
    assert_eq!(scaled_radii(&scaled), [48.26, 48.26]);

    for scale_value in [25.4, 1000.0, 0.1, 0.0254, 3.0e-7] {
        let mut minor = 1.0e-3_f64;
        for _ in 0..4096 {
            let major = f64::from_bits(minor.to_bits() + 1);
            let [scaled_major, scaled_minor] = scaled_radii(
                &ellipse(major, minor)
                    .scaled(
                        &cadmpeg_test_support::service_decode_context(),
                        scale(scale_value),
                    )
                    .expect("fixture scaling admission")
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
            geometry
                .scaled(
                    &cadmpeg_test_support::service_decode_context(),
                    scale(scale_value)
                )
                .expect("fixture scaling admission"),
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
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
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
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_recursion_depth =
                u64::try_from(MAX_GEOMETRY_NESTING).expect("fixture depth");
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            let scaled = chain
                .scaled(&ctx, scale(25.4))
                .expect("fixture scaling admission")
                .expect("finite scaled chain");
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
        })
        .expect("start a thread with a one MiB stack")
        .join()
        .expect("scale the full placement chain on a one MiB stack");
}

#[test]
fn a_scaled_curve_chain_fits_a_small_stack_and_keeps_resource_limits() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let translation = Transform::affine([
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 3.0],
            ])
            .expect("translation");
            let mut chain = SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(1.0, 2.0, 3.0), Vector3::new(1.0, 0.0, 0.0))
                    .expect("line"),
            );
            for _ in 0..MAX_GEOMETRY_NESTING {
                chain = SolvedCurveGeometry::Transformed(
                    PlacedCurve::try_new(Box::new(chain), translation).expect("admitted depth"),
                );
            }
            let depth = u64::try_from(MAX_GEOMETRY_NESTING).expect("depth");
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_recursion_depth = depth;
            policy.limits.max_work_units = 2 * depth;
            policy.limits.max_collection_items = depth;
            policy.limits.max_retained_bytes = depth
                * u64::try_from(std::mem::size_of::<SolvedCurveGeometry>()).expect("carrier size");
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            let borrowed = chain
                .scaled(&ctx, scale(25.4))
                .expect("admitted carrier copy")
                .expect("finite scaled chain");
            ctx.finish_session()
                .expect("exact copy and scaling budgets");
            for (work, nesting, operation) in [
                (depth - 1, depth, "IR geometry unit scaling work"),
                (depth, depth - 1, "IR geometry unit scaling nesting"),
            ] {
                let refused = with_scaling_limits(work, 0, nesting, |ctx| {
                    chain.clone().scaled_owned(ctx, scale(25.4))
                });
                assert!(
                    matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.operation == operation)
                );
            }
            let scaled = with_scaling_limits(depth, 0, depth, |ctx| {
                chain.scaled_owned(ctx, scale(25.4))
            })
            .expect("exact work and depth budgets")
            .expect("finite scaled chain");
            assert_eq!(borrowed, scaled);
            assert_eq!(scaled.nesting_depth(), MAX_GEOMETRY_NESTING);
            let mut leaf = &scaled;
            while let SolvedCurveGeometry::Transformed(placed) = leaf {
                assert_eq!(
                    placed.transform().affine_rows().map(|row| row[3]),
                    [25.4, 2.0 * 25.4, 3.0 * 25.4]
                );
                leaf = placed.basis();
            }
            let SolvedCurveGeometry::Line(line) = leaf else {
                panic!("the leaf stays a line");
            };
            assert_eq!(
                line.origin().get(),
                Point3::new(25.4, 2.0 * 25.4, 3.0 * 25.4)
            );
        })
        .expect("start a thread with a one MiB stack")
        .join()
        .expect("scale the full curve chain on a one MiB stack");
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
        translation_only
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission"),
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
        both.scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission"),
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
        .scaled(
            &cadmpeg_test_support::service_decode_context(),
            scale(SCALE_TO_TINY_LENGTH),
        )
        .expect("fixture scaling admission")
        .expect("a zero radius stays admitted")
    else {
        panic!("a scaled cone stays a cone");
    };
    assert_eq!(scaled.radius().get(), 0.0);
    assert_eq!(
        cone(f64::MAX)
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission"),
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
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("polyline construction admission")
            .expect("a polyline fixture"),
        )
    };
    let SolvedCurveGeometry::Polyline(scaled) = polyline(1.0, 0.0)
        .scaled(&cadmpeg_test_support::service_decode_context(), scale(25.4))
        .expect("fixture scaling admission")
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
        polyline(f64::MAX, f64::MAX)
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission"),
        refusal("points must be finite")
    );
    assert_eq!(
        polyline(1.0, f64::MAX)
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission"),
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
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("polygonal construction admission")
            .expect("a polygonal fixture"),
        )
    };
    assert_eq!(
        surface(1.0, 0.0)
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(25.4))
            .expect("fixture scaling admission"),
        Ok(SolvedSurfaceGeometry::Polygonal(
            PolygonalSurface::new(
                vec![
                    Point3::new(25.4, 0.0, 0.0),
                    Point3::new(0.0, 25.4, 0.0),
                    Point3::new(0.0, 0.0, 25.4),
                ],
                vec![[0, 1, 2]],
                0.0,
                &cadmpeg_test_support::service_decode_context()
            )
            .expect("polygonal construction admission")
            .expect("a polygonal fixture"),
        ))
    );
    let refusal = |message: &str| {
        Err(ScaleRefusal::Samples(GeometryLayoutError::Layout(
            message.to_string(),
        )))
    };
    assert_eq!(
        surface(f64::MAX, f64::MAX)
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission"),
        refusal("vertices must be finite")
    );
    assert_eq!(
        surface(1.0, f64::MAX)
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission"),
        refusal("chordal_deflection must be finite and non-negative")
    );
}

fn with_scaling_limits<T>(
    work: u64,
    retained: u64,
    depth: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_retained_bytes = retained;
    policy.limits.max_recursion_depth = depth;
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(&ctx)
}

fn scaling_curve(x: f64, rational: bool) -> SolvedCurveGeometry {
    SolvedCurveGeometry::Nurbs(
        crate::geometry::nurbs::NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(x, 0.0, 0.0); 2],
            rational.then(|| vec![1.0, 2.0]),
            false,
        )
        .expect("fixture constructor admission")
        .expect("curve"),
    )
}

fn scaling_surface(x: f64, rational: bool) -> SolvedSurfaceGeometry {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![vec![Point3::new(x, 0.0, 0.0); 2]; 2],
                rational.then(|| vec![vec![1.0, 2.0]; 2]),
            ),
            false,
        )
        .expect("fixture constructor admission")
        .expect("surface"),
    )
}

#[test]
fn owned_nurbs_scaling_refuses_each_pole_work_and_reuses_lanes() {
    for rational in [false, true] {
        for cap in [1, 2] {
            let result = with_scaling_limits(cap, u64::MAX, u64::MAX, |ctx| {
                scaling_curve(1.0, rational).scaled_owned(ctx, scale(2.0))
            });
            assert!(
                matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(resource))
                if resource.operation == "IR NURBS unit scaling work")
            );
        }
        assert_eq!(
            with_scaling_limits(3, 0, u64::MAX, |ctx| scaling_curve(1.0, rational)
                .scaled_owned(ctx, scale(2.0)))
            .expect("admitted"),
            scaling_curve(1.0, rational)
                .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
                .expect("fixture scaling admission")
        );
        for operation in ["IR NURBS unit scaling rows", "IR NURBS unit scaling work"] {
            cadmpeg_test_support::refusal::resource_limit_at(cadmpeg_core::decode::ResourceDimension::WorkUnits, operation,
                |cap| with_scaling_limits(cap, u64::MAX, u64::MAX, |ctx| {
                    scaling_surface(1.0, rational).scaled_owned(ctx, scale(2.0))
                }));
        }
        assert_eq!(
            with_scaling_limits(9, 0, u64::MAX, |ctx| scaling_surface(1.0, rational)
                .scaled_owned(ctx, scale(2.0)))
            .expect("admitted"),
            scaling_surface(1.0, rational)
                .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
                .expect("fixture scaling admission")
        );
        for retained in [0, 40] {
            assert!(matches!(with_scaling_limits(u64::MAX, retained, u64::MAX,
                |ctx| scaling_curve(f64::MAX, rational).scaled_owned(ctx, scale(2.0))),
                Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "IR NURBS refusal text"));
        }
        assert_eq!(
            with_scaling_limits(u64::MAX, u64::MAX, u64::MAX, |ctx| scaling_surface(
                f64::MAX,
                rational
            )
            .scaled_owned(ctx, scale(2.0)))
            .expect("admitted refusal"),
            scaling_surface(f64::MAX, rational)
                .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
                .expect("fixture scaling admission")
        );
    }
}

#[test]
fn owned_sample_scaling_refuses_work_and_retained_text_without_row_copies() {
    for parameterized in [false, true] {
        let curve = |x, deflection| {
            SolvedCurveGeometry::Polyline(
                PolylineCurve::new(
                    if parameterized {
                        PolylineSamples::Parameterized {
                            vertices: vec![
                                PolylineVertex {
                                    parameter: 0.0,
                                    point: Point3::new(x, 0.0, 0.0),
                                },
                                PolylineVertex {
                                    parameter: 1.0,
                                    point: Point3::new(0.0, 1.0, 0.0),
                                },
                            ]
                            .try_into()
                            .expect("samples"),
                        }
                    } else {
                        PolylineSamples::Unparameterized {
                            points: vec![Point3::new(x, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)]
                                .try_into()
                                .expect("samples"),
                        }
                    },
                    deflection,
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("polyline construction admission")
                .expect("polyline"),
            )
        };
        for cap in [1, 2] {
            assert!(matches!(with_scaling_limits(cap, 0, u64::MAX,
                |ctx| curve(1.0, 0.0).scaled_owned(ctx, scale(2.0))),
                Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "IR sampled unit scaling work"));
        }
        assert_eq!(
            with_scaling_limits(3, 0, u64::MAX, |ctx| curve(1.0, 0.0)
                .scaled_owned(ctx, scale(2.0)))
            .expect("admitted"),
            curve(1.0, 0.0)
                .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
                .expect("fixture scaling admission")
        );
        for (x, deflection) in [(f64::MAX, f64::MAX), (1.0, f64::MAX)] {
            assert!(matches!(with_scaling_limits(u64::MAX, 0, u64::MAX,
                |ctx| curve(x, deflection).scaled_owned(ctx, scale(2.0))),
                Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "IR sampled refusal text"));
            assert_eq!(
                with_scaling_limits(u64::MAX, u64::MAX, u64::MAX, |ctx| curve(x, deflection)
                    .scaled_owned(ctx, scale(2.0)))
                .expect("admitted refusal"),
                curve(x, deflection)
                    .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
                    .expect("fixture scaling admission")
            );
        }
    }
    let surface = |x, deflection| {
        SolvedSurfaceGeometry::Polygonal(
            PolygonalSurface::new(
                vec![
                    Point3::new(x, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(0.0, 0.0, 1.0),
                ],
                vec![[0, 1, 2]],
                deflection,
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("polygonal construction admission")
            .expect("polygonal"),
        )
    };
    for cap in 1..4 {
        assert!(matches!(with_scaling_limits(cap, 0, u64::MAX,
            |ctx| surface(1.0, 0.0).scaled_owned(ctx, scale(2.0))),
            Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "IR sampled unit scaling work"));
    }
    assert_eq!(
        with_scaling_limits(4, 0, u64::MAX, |ctx| surface(1.0, 0.0)
            .scaled_owned(ctx, scale(2.0)))
        .expect("admitted"),
        surface(1.0, 0.0)
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission")
    );
    for (x, deflection) in [(f64::MAX, f64::MAX), (1.0, f64::MAX)] {
        assert!(matches!(with_scaling_limits(u64::MAX, 0, u64::MAX,
            |ctx| surface(x, deflection).scaled_owned(ctx, scale(2.0))),
            Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "IR sampled refusal text"));
        assert_eq!(
            with_scaling_limits(u64::MAX, u64::MAX, u64::MAX, |ctx| surface(x, deflection)
                .scaled_owned(ctx, scale(2.0)))
            .expect("admitted refusal"),
            surface(x, deflection)
                .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
                .expect("fixture scaling admission")
        );
    }
}

#[test]
fn owned_placement_scaling_refuses_nesting_and_geometry_work() {
    let surface = || {
        SolvedSurfaceGeometry::Transformed(
            PlacedSurface::try_new(Box::new(plane()), Transform::identity()).expect("placement"),
        )
    };
    assert!(matches!(with_scaling_limits(u64::MAX, 0, 0,
        |ctx| surface().scaled_owned(ctx, scale(2.0))),
        Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "IR geometry unit scaling nesting"));
    assert!(matches!(with_scaling_limits(0, 0, 1,
        |ctx| surface().scaled_owned(ctx, scale(2.0))),
        Err(cadmpeg_core::CodecError::ResourceLimit(resource)) if resource.operation == "IR geometry unit scaling work"));
    assert_eq!(
        with_scaling_limits(2, 0, 1, |ctx| surface().scaled_owned(ctx, scale(2.0)))
            .expect("admitted"),
        surface()
            .scaled(&cadmpeg_test_support::service_decode_context(), scale(2.0))
            .expect("fixture scaling admission")
    );
}

#[test]
fn borrowed_scaling_preserves_caller_refusals_and_source_carriers() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::MaterializedBytes,
    ] {
        for surface in [false, true] {
            let curve = scaling_curve(1.0, true);
            let patch = scaling_surface(1.0, true);
            let original_curve = curve.clone();
            let original_patch = patch.clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                _ => panic!("fixture dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut storage = ctx
                .reserve_scoped(0, "borrowed scaling scope")
                .expect("empty scope");
            let run = || {
                if surface {
                    patch.scaled(&ctx, scale(2.0)).map(|_| ())
                } else {
                    curve.scaled(&ctx, scale(2.0)).map(|_| ())
                }
            };
            let result = if dimension == ResourceDimension::MaterializedBytes {
                storage.with_storage(run)
            } else {
                run()
            };
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("caller limit must refuse the copy");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.operation, "IR scaled carrier copy");
            assert_eq!(curve, original_curve);
            assert_eq!(patch, original_patch);
            drop(storage);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
    }

}

#[test]
fn borrowed_scaling_admits_nested_copies_and_matches_owned_scaling() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = SolvedSurfaceGeometry::Transformed(
        PlacedSurface::try_new(Box::new(plane()), Transform::identity()).expect("placement"),
    );
    let original = source.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(limit)) = source.scaled(&ctx, scale(2.0)) else {
        panic!("the borrowed placement enters the caller depth account");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.operation, "IR scaled carrier copy");
    assert_eq!(source, original);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        source.scaled(&ctx, scale(2.0)).expect("copy"),
        source
            .clone()
            .scaled_owned(&ctx, scale(2.0))
            .expect("owned")
    );
    for rational in [false, true] {
        let curve = scaling_curve(1.0, rational);
        let surface = scaling_surface(1.0, rational);
        assert_eq!(
            curve.scaled(&ctx, scale(25.4)).expect("curve copy"),
            curve
                .clone()
                .scaled_owned(&ctx, scale(25.4))
                .expect("owned curve")
        );
        assert_eq!(
            surface.scaled(&ctx, scale(25.4)).expect("surface copy"),
            surface
                .clone()
                .scaled_owned(&ctx, scale(25.4))
                .expect("owned surface")
        );
    }
    ctx.finish_session().expect("shared scaling session");
}
