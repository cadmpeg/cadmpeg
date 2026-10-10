// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use crate::brep::graph::test_support::test_nurbs_curve;
use crate::brep::graph::test_support::test_nurbs_surface;
const EPS_EXPECTED_INTERSECTION_PARAMETER: f64 = 1.0e-10;
const EPS_EXPECTED_PARAMETER: f64 = 1.0e-12;
const EPS_INTERSECTION_FIT_MM: f64 = 1.0e-9;

fn intersection_support_pcurve(
    support_data: &crate::brep::intersection::IntersectionSupportData,
    chart: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    surface_attr: u16,
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    edge_endpoints: [cadmpeg_ir::math::Point3; 2],
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<
    Option<(
        cadmpeg_ir::geometry::pcurve::PcurveGeometry,
        [f64; 2],
        crate::brep::graph::pcurves::IntersectionPcurveSource,
    )>,
    cadmpeg_core::CodecError,
> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("empty test root fits service policy");
    crate::brep::graph::pcurves::intersection_support_pcurve(
        &ctx,
        support_data,
        chart,
        surface_attr,
        surface,
        edge_endpoints,
        &mut crate::lane_refusal::scoped::RefusalSink::Retained(refusal),
    )
}
#[test]
fn intersection_uv_converts_length_parameters_and_exact_endpoints() {
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    let endpoints = [
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            0.0,
            3.0,
        )
        .expect("cylinder start")
        .get(),
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            0.5,
            2.0,
        )
        .expect("cylinder end")
        .get(),
    ];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
    let support_data = crate::brep::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: 0.2,
        support_uv: Some([
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0029),
                cadmpeg_ir::math::Point2::new(0.5, 0.0018),
            ],
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ],
        ]),
    };
    let (geometry, range, source) = intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .expect("support parameterization");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected solved UV NURBS");
    };
    assert_eq!(
        nurbs.control_points()[0],
        cadmpeg_ir::math::Point2::new(0.0, 3.0)
    );
    assert_eq!(
        nurbs.control_points()[1],
        cadmpeg_ir::math::Point2::new(0.5, 2.0)
    );
    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(source, crate::brep::graph::pcurves::IntersectionPcurveSource::StoredCache);

    let ambiguous = crate::brep::intersection::IntersectionSupportData {
        supports: [10, 10],
        ..support_data.clone()
    };
    assert!(intersection_support_pcurve(
        &ambiguous,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .expect("resource allocation did not fail")
    .is_none());

    let malformed = crate::brep::intersection::IntersectionSupportData {
        support_uv: Some([Vec::new(), Vec::new()]),
        ..support_data
    };
    assert!(intersection_support_pcurve(
        &malformed,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .expect("resource allocation did not fail")
    .is_none());
}

#[test]
fn intersection_support_pcurve_refuses_collection_limit() {
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid cylinder"),
    ));
    let endpoints = [
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            0.0,
            3.0,
        )
        .expect("start")
        .get(),
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            0.5,
            2.0,
        )
        .expect("end")
        .get(),
    ];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
    let support_data = crate::brep::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: 0.2,
        support_uv: Some([
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0029),
                cadmpeg_ir::math::Point2::new(0.5, 0.0018),
            ],
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ],
        ]),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits policy");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = crate::brep::graph::pcurves::intersection_support_pcurve(
        &ctx,
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::scoped::RefusalSink::Retained(&mut crate::lane_refusal::LaneRefusals::new()),
    ) else {
        panic!("two UV controls exceed one collection item")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert!(intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service policy")
    .is_some());
}

#[test]
fn analytic_intersection_chart_derives_continuous_uv_without_cache() {
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    let model_points = [(3.0, 1.0), (3.2, 2.0), (3.4, 3.0)]
        .map(|(u, v)| {
            cadmpeg_ir::eval::decode::surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &surface,
                u,
                v,
            )
            .expect("cylinder point")
            .get()
        })
        .to_vec();
    let endpoints = [model_points[0], model_points[2]];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
    let support_data = crate::brep::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: 0.011,
        support_uv: None,
    };

    let (geometry, _, source) = intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .expect("analytic support inversion");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected solved UV NURBS");
    };
    for (point, expected) in nurbs
        .control_points()
        .iter()
        .zip([(3.0, 1.0), (3.2, 2.0), (3.4, 3.0)])
    {
        assert!((point.u - expected.0).abs() < EPS_EXPECTED_PARAMETER);
        assert!((point.v - expected.1).abs() < EPS_EXPECTED_PARAMETER);
    }
    assert_eq!(source, crate::brep::graph::pcurves::IntersectionPcurveSource::AnalyticInverse);

    let under_toleranced = crate::brep::intersection::IntersectionSupportData {
        fit_tolerance_mm: 0.009,
        ..support_data
    };
    assert!(intersection_support_pcurve(
        &under_toleranced,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .expect("resource allocation did not fail")
    .is_none());
}

#[test]
fn analytic_torus_chart_unwraps_both_periodic_parameters() {
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            5.0,
            1.0,
        )
        .unwrap(),
    ));
    let expected = [(3.0, 3.0), (3.2, 3.2), (3.4, 3.4)];
    let model_points = expected
        .map(|(u, v)| {
            cadmpeg_ir::eval::decode::surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &surface,
                u,
                v,
            )
            .expect("torus point")
            .get()
        })
        .to_vec();
    let endpoints = [model_points[0], model_points[2]];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
    let support_data = crate::brep::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: 0.08,
        support_uv: None,
    };

    let (geometry, _, _) = intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .expect("torus support inversion");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected solved UV NURBS");
    };
    for (point, expected) in nurbs.control_points().iter().zip(expected) {
        assert!((point.u - expected.0).abs() < EPS_EXPECTED_PARAMETER);
        assert!((point.v - expected.1).abs() < EPS_EXPECTED_PARAMETER);
    }
}

#[test]
fn nurbs_intersection_chart_inverts_with_continuation_seeds() {
    let nurbs = test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let surface =
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs.clone()));
    let expected = [(0.2, 0.1), (0.5, 0.4), (0.8, 0.7)];
    let model_points = expected
        .map(|(u, v)| {
            cadmpeg_ir::eval::decode::nurbs_surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &nurbs,
                u,
                v,
            )
            .expect("surface point")
            .get()
        })
        .to_vec();
    let endpoints = [model_points[0], model_points[2]];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], model_points, None);
    let support_data = crate::brep::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm: EPS_INTERSECTION_FIT_MM,
        support_uv: None,
    };

    let (geometry, _, source) = intersection_support_pcurve(
        &support_data,
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .expect("NURBS support inversion");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("expected solved UV NURBS");
    };
    for (point, expected) in nurbs.control_points().iter().zip(expected) {
        assert!((point.u - expected.0).abs() < EPS_EXPECTED_INTERSECTION_PARAMETER);
        assert!((point.v - expected.1).abs() < EPS_EXPECTED_INTERSECTION_PARAMETER);
    }
    assert_eq!(source, crate::brep::graph::pcurves::IntersectionPcurveSource::NurbsInverse);
}

#[test]
fn nurbs_intersection_chart_requires_a_complete_chord_certificate() {
    let nurbs = test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
        ],
        None,
    );
    let surface =
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs));
    let endpoints = [
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
    ];
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
    let support_data = |fit_tolerance_mm| crate::brep::intersection::IntersectionSupportData {
        supports: [10, 11],
        fit_tolerance_mm,
        support_uv: None,
    };

    assert!(intersection_support_pcurve(
        &support_data(0.3),
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .is_none());
    assert!(intersection_support_pcurve(
        &support_data(0.34),
        &chart,
        10,
        &surface,
        endpoints,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("resource allocation did not fail")
    .is_some());
}


fn planar_intersection_fixture() -> (
    cadmpeg_ir::geometry::SurfaceGeometry,
    cadmpeg_ir::geometry::nurbs::NurbsCurve,
    crate::brep::intersection::IntersectionSupportData,
    [cadmpeg_ir::math::Point3; 2],
) {
    let endpoints = [cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0)];
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            endpoints[0], cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ).unwrap(),
    ));
    // The source carrier is constructed in a separate fixture session.
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 1.0, 1.0], endpoints.to_vec(), None);
    let support = crate::brep::intersection::IntersectionSupportData {
        supports: [10, 11], fit_tolerance_mm: EPS_INTERSECTION_FIT_MM,
        support_uv: Some([vec![cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(0.001, 0.001)], Vec::new()]),
    };
    (surface, chart, support, endpoints)
}

#[test]
fn intersection_pcurve_retains_only_output_and_releases_all_workspace() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (surface, chart, support, endpoints) = planar_intersection_fixture();
    let output_bytes = u64::try_from(4 * std::mem::size_of::<f64>()
        + 2 * std::mem::size_of::<cadmpeg_ir::units::FinitePoint2>()).unwrap();
    // Core amortized growth reserves four Point3 slots for two lifted points.
    // Two raw UVs and two errors coexist with that backing. After it dies,
    // raw UVs coexist with four knots and two admitted poles.
    let peak = u64::try_from(2 * std::mem::size_of::<cadmpeg_ir::math::Point2>()
        + 4 * std::mem::size_of::<cadmpeg_ir::math::Point3>()
        + 2 * std::mem::size_of::<f64>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = output_bytes;
    policy.limits.max_materialized_bytes = peak;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut refusals = crate::lane_refusal::scoped::ScopedLaneRefusals::new(&ctx).unwrap();
    let (geometry, range, source) = crate::brep::graph::pcurves::intersection_support_pcurve(
        &ctx, &support, &chart, 10, &surface, endpoints,
        &mut crate::lane_refusal::scoped::RefusalSink::Scoped(&mut refusals),
    ).unwrap().unwrap();
    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(source, crate::brep::graph::pcurves::IntersectionPcurveSource::StoredCache);
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &geometry else {
        panic!("the valid planar cache produces the original NURBS pcurve");
    };
    assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(nurbs.pole_rows().point_at(0).unwrap().get(), cadmpeg_ir::math::Point2::new(0.0, 0.0));
    assert_eq!(nurbs.pole_rows().point_at(1).unwrap().get(), cadmpeg_ir::math::Point2::new(1.0, 1.0));
    let reuse = ctx.reserve_scoped(peak, "reuse intersection pcurve workspace with output live").unwrap();
    drop(reuse);
    let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx.copy_retained_text(
        "x", "intersection final backing remains retained").unwrap_err() else {
        panic!("the actual final geometry consumes its exact output allowance");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, output_bytes);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn intersection_pcurve_refuses_exact_workspace_and_final_backing_boundaries() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (surface, chart, support, endpoints) = planar_intersection_fixture();
    let output_bytes = u64::try_from(4 * std::mem::size_of::<f64>()
        + 2 * std::mem::size_of::<cadmpeg_ir::units::FinitePoint2>()).unwrap();
    let raw_bytes = u64::try_from(2 * std::mem::size_of::<cadmpeg_ir::math::Point2>()).unwrap();
    let lifted_bytes = u64::try_from(4 * std::mem::size_of::<cadmpeg_ir::math::Point3>()).unwrap();
    let error_bytes = u64::try_from(2 * std::mem::size_of::<f64>()).unwrap();
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::RetainedBytes] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = output_bytes;
        policy.limits.max_materialized_bytes = raw_bytes + lifted_bytes + error_bytes;
        if dimension == ResourceDimension::MaterializedBytes {
            policy.limits.max_materialized_bytes -= 1;
        } else {
            policy.limits.max_retained_bytes -= 1;
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut refusals = crate::lane_refusal::scoped::ScopedLaneRefusals::new(&ctx).unwrap();
        let error = crate::brep::graph::pcurves::intersection_support_pcurve(
            &ctx, &support, &chart, 10, &surface, endpoints,
            &mut crate::lane_refusal::scoped::RefusalSink::Scoped(&mut refusals),
        ).unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("one byte below actual workspace or output backing refuses");
        };
        assert_eq!(limit.dimension, dimension);
        if dimension == ResourceDimension::MaterializedBytes {
            assert_eq!(limit.operation, "check intersection support control errors");
            assert_eq!(limit.used, raw_bytes + lifted_bytes);
            assert_eq!(limit.additional, error_bytes);
        } else {
            assert_eq!(limit.operation, "hold intersection support pcurve construction");
            assert_eq!(limit.used, 0);
            assert_eq!(limit.additional, output_bytes);
        }
        assert_eq!(ctx.resource_refusal(), Some(limit));
        let cadmpeg_core::CodecError::ResourceLimit(replay) = ctx.reserve_scoped(
            0, "replay original intersection pcurve refusal").unwrap_err() else {
            panic!("original refusal remains sticky after all candidate values die");
        };
        assert_eq!(replay, limit);
    }
}

#[test]
fn rejected_intersection_uv_length_releases_raw_controls_without_retained_output() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (surface, _, support, endpoints) = planar_intersection_fixture();
    let chart = test_nurbs_curve(1, vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![endpoints[0], cadmpeg_ir::math::Point3::new(0.5, 0.5, 0.0), endpoints[1]], None);
    let raw_bytes = u64::try_from(2 * std::mem::size_of::<cadmpeg_ir::math::Point2>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = raw_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let mut refusals = crate::lane_refusal::scoped::ScopedLaneRefusals::new(&ctx).unwrap();
        assert!(crate::brep::graph::pcurves::intersection_support_pcurve(
            &ctx, &support, &chart, 10, &surface, endpoints,
            &mut crate::lane_refusal::scoped::RefusalSink::Scoped(&mut refusals),
        ).unwrap().is_none());
        let reuse = ctx.reserve_scoped(raw_bytes, "reuse rejected intersection UV controls").unwrap();
        drop(reuse);
    }
    assert_eq!(ctx.resource_refusal(), None);
}
