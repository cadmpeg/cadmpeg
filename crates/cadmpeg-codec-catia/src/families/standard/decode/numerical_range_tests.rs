// SPDX-License-Identifier: Apache-2.0

use super::*;

const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;

#[test]
fn bezier_parameter_search_refuses_before_nodes_queue_and_parameters() {
    use cadmpeg_core::CodecError;

    let point = Point3::new(0.0, 0.0, 0.0);
    for (limit, operation) in [
        (0, "catia_bezier_search_nodes"),
        (1, "catia_bezier_search_queue"),
        (2, "catia_bezier_parameters"),
    ] {
        let result = crate::test_support::with_collection_limit(limit, |ctx| {
            let mut parameters = Vec::new();
            collect_bezier_point_parameters(
                ctx,
                [point; 6],
                [0.0, 1.0],
                point,
                0.01,
                1.0,
                &mut parameters,
            )
        });
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(error)) if error.operation == operation)
        );
    }
    crate::test_support::with_service_context(|ctx| {
        let mut parameters = Vec::new();
        collect_bezier_point_parameters(
            ctx,
            [point; 6],
            [0.0, 1.0],
            point,
            0.01,
            1.0,
            &mut parameters,
        )
        .expect("service budget");
        assert!(!parameters.is_empty());
    });
}

#[test]
fn numerical_0922_quintic_keeps_both_branches() {
    for d in [1., SMALL_PARAMETER_DOMAIN] {
        let mut poles = (0..6)
            .map(|i| Point3::new(-1. + 2. * f64::from(i) / 5., 0., 0.))
            .collect::<Vec<_>>();
        poles.extend((0..6).map(|i| Point3::new(1. - 2. * f64::from(i) / 5., 0., 0.)));
        let mut knots = vec![0.; 6];
        knots.extend(vec![0.5 * d; 6]);
        knots.extend(vec![d; 6]);
        let n = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            5,
            knots,
            poles,
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid quintic curve");
        let result = crate::test_support::with_service_context(|ctx| {
            standard_limit_curve_point_parameter(ctx, &n, Point3::new(0., 0., 0.), 2e-3)
        })
        .expect("service budget");
        println!("CATIA folded quintic locus d{d:e}: {result:?}");
        assert_eq!(result, None);
    }
}
#[test]
fn numerical_0922_finite_bezier_midpoint() {
    for x in [0., 1e308] {
        let poles = (0..6)
            .map(|i| Point3::new(x, f64::from(i) / 5., 0.))
            .collect::<Vec<_>>();
        let mut knots = vec![0.; 6];
        knots.extend(vec![1.; 6]);
        let n = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            5,
            knots,
            poles,
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid quintic curve");
        let result = crate::test_support::with_service_context(|ctx| {
            standard_limit_curve_point_parameter(ctx, &n, Point3::new(x, 0.5, 0.), 2e-3)
        })
        .expect("service budget");
        println!("CATIA straight quintic at x{x:e} midpoint: {result:?}");
        assert_eq!(result, Some(0.5));
    }
}

#[test]
fn standard_limit_curve_finds_interior_point_on_wide_finite_domain() {
    let poles = (0..6)
        .map(|index| Point3::new(0.0, f64::from(index) / 5.0, 0.0))
        .collect::<Vec<_>>();
    let mut knots = vec![-f64::MAX; 6];
    knots.extend(vec![f64::MAX; 6]);
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        5,
        knots,
        poles,
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("wide finite quintic domain");
    let parameter = crate::test_support::with_service_context(|ctx| {
        standard_limit_curve_point_parameter(ctx, &curve, Point3::new(0.0, 0.2, 0.0), 2e-3)
    })
    .expect("service budget")
    .expect("interior point parameter");
    assert!((parameter / f64::MAX + 0.6).abs() <= 0.01);
}

use cadmpeg_ir::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};
fn audit_plane(d: [f64; 2], s: f64) -> NurbsSurface {
    NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![d[0], d[0], d[1], d[1]], false),
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0., 0., 0.), Point3::new(0., s, 0.)],
                vec![Point3::new(s, 0., 0.), Point3::new(s, s, 0.)],
            ],
            None,
        ),
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid bilinear surface")
}
#[test]
fn numerical_0922b_surface_membership_wide_chart() {
    for d in [[0., 1.], [-1e308, 1e308]] {
        let r = point_on_nurbs_surface(
            &cadmpeg_test_support::service_decode_context(),
            Point3::new(0.3, 0.7, 0.),
            &audit_plane(d, 1.),
        );
        println!("CATIA plane chart{d:?}: {r:?}");
        assert_eq!(r, Ok(Some(true)));
    }
}
#[test]
fn numerical_0922b_surface_membership_large_plane() {
    for scale in [1., 1e200] {
        let r = point_on_nurbs_surface(
            &cadmpeg_test_support::service_decode_context(),
            Point3::new(0.3 * scale, 0.7 * scale, 0.),
            &audit_plane([0., 1.], scale),
        );
        println!("CATIA plane scale{scale:e}: {r:?}");
        assert_eq!(r, Ok(Some(true)));
    }
}

#[test]
fn limit_curve_binding_keeps_a_finite_midpoint_on_large_same_sign_domains() {
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};

    for domain in [[1e308, 1.5e308], [-1.5e308, -1e308]] {
        let mut ir = CadIr::empty();
        for (index, y) in [0.0, 1.0].into_iter().enumerate() {
            ir.model.points.push(Point::new(
                PointId::mint(format!("catia:test:point#wide-binding-{index}")).expect("id"),
                FinitePoint3::new(Point3::new(0.0, y, 0.0)).expect("point"),
                None,
            ));
        }
        let surface_id = SurfaceId::mint("catia:test:surface#wide-binding").expect("id");
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("plane"),
            )),
            source_object: None,
        });
        let mut knots = vec![domain[0]; 6];
        knots.extend([domain[1]; 6]);
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            5,
            knots,
            (0..6)
                .map(|i| Point3::new(0.0, f64::from(i) / 5.0, 0.0))
                .collect(),
            None,
            false,
        )
        .expect("admission")
        .expect("curve");
        let result = crate::test_support::with_service_context(|ctx| {
            standard_limit_curve_bindings(
                ctx,
                &ir,
                &[(surface_id.clone(), true, 0)],
                &HashMap::from([(surface_id, 0)]),
                &[StandardCurveSupport {
                    pos: 0,
                    tag: 1,
                    faces: [0, 0],
                    geometry: StandardCurveGeometry::Bspline,
                }],
                &[curve],
            )
        })
        .expect("binding search");
        assert_eq!(
            result,
            vec![vec![StandardLimitCurveBinding {
                curve: 0,
                points: [0, 1],
                parameter_range: domain,
            }]]
        );
    }
}

#[test]
fn circle_endpoint_search_keeps_antipodal_seams_at_a_large_origin() {
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};

    let mut ir = CadIr::empty();
    for (index, y) in [-1.0, 1.0].into_iter().enumerate() {
        ir.model.points.push(Point::new(
            PointId::mint(format!("catia:test:point#wide-circle-{index}")).expect("id"),
            FinitePoint3::new(Point3::new(1e308, y, 0.0)).expect("point"),
            None,
        ));
    }
    let result = crate::test_support::with_service_context(|ctx| {
        resolve_standard_endpoint_pairs(
            ctx,
            &ir,
            &[],
            &HashMap::new(),
            &[StandardCurveSupport {
                pos: 0,
                tag: 1,
                faces: [0, 0],
                geometry: StandardCurveGeometry::Circle {
                    center: FinitePoint3::new(Point3::new(1e308, 0.0, 0.0)).expect("center"),
                    radius: PositiveLength::new(1.0).expect("radius"),
                },
            }],
            &[vec![0, 1]],
        )
    })
    .expect("endpoint search")
    .expect("relation");
    assert_eq!(result, vec![vec![[0, 0], [0, 1], [1, 1]]]);
}
