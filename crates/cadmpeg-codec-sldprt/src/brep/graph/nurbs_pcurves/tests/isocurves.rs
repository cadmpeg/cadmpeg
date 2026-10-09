// SPDX-License-Identifier: Apache-2.0

use crate::brep::graph::test_support::test_nurbs_curve;
use crate::brep::graph::test_support::test_nurbs_surface;
const EPS_EXPECTED_PARAMETER: f64 = 1.0e-12;

#[test]
fn interior_linear_axis_rational_nurbs_isocurve_has_exact_pcurve() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![-0.1, -0.1, 0.9, 0.9],
        3,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, -1.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 3.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, -1.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 3.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, -1.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, 3.0),
        ],
        Some(vec![1.0, 1.0, 2.0, 2.0, 1.0, 1.0]),
    );
    let curve = test_nurbs_curve(
        2,
        surface.u_knots().to_vec(),
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 2.0, 1.0]),
    );
    let geometry =
        match crate::brep::graph::nurbs_pcurves::nurbs_isocurve_pcurve(&ctx, &surface, &curve).expect("isocurve lanes pair") {
            crate::brep::graph::inverse::InverseResolution::Unique(geometry) => geometry,
            crate::brep::graph::inverse::InverseResolution::NoMatch => panic!("interior isocurve did not match"),
            crate::brep::graph::inverse::InverseResolution::Ambiguous => panic!("interior isocurve was ambiguous"),
        };
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected isoparametric line pcurve");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!(origin.u.abs() < EPS_EXPECTED_PARAMETER);
    assert!((origin.v - 0.15).abs() < EPS_EXPECTED_PARAMETER);
    assert!((direction.u - 1.0).abs() < EPS_EXPECTED_PARAMETER);
    assert!(direction.v.abs() < EPS_EXPECTED_PARAMETER);
}

#[test]
fn extended_nurbs_isocurve_clamps_the_carrier_before_matching() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
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
    let curve = test_nurbs_curve(
        1,
        vec![-1.0, -1.0, 2.0, 2.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.5, -1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 2.0, 0.0),
        ],
        None,
    );
    let resolution = crate::brep::graph::nurbs_pcurves::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.2, 0.8])
        .expect("isocurve lanes pair");
    let crate::brep::graph::nurbs_pcurves::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
        line_pcurve,
    )) = resolution
    else {
        panic!("extended isocurve was not certified");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!((origin.u - 0.5).abs() < EPS_EXPECTED_PARAMETER);
    assert!(origin.v.abs() < EPS_EXPECTED_PARAMETER);
    assert!(direction.u.abs() < EPS_EXPECTED_PARAMETER);
    assert!((direction.v - 1.0).abs() < EPS_EXPECTED_PARAMETER);
    let clamped = crate::brep::graph::clamp::clamp_nurbs_curve_to_domain(&ctx, &curve, [0.0, 1.0])
        .expect("the clamped lanes are a curve")
        .expect("clamped segment");
    let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
        &ctx,
        &surface,
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        0.5,
    )
    .expect("resource allocation did not fail")
    .expect("surface isocurve");
    assert!(crate::brep::graph::nurbs_pcurves::nurbs_representation_matches(&ctx, &expected, &clamped).unwrap());
}

#[test]
fn extended_quadratic_isocurve_preserves_the_inserted_homogeneous_segment() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        1,
        2,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        2,
        3,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let curve = test_nurbs_curve(
        2,
        vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.5, -1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 0.5, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 2.0, 0.0),
        ],
        None,
    );
    let resolution = crate::brep::graph::nurbs_pcurves::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.1, 0.9])
        .expect("isocurve lanes pair");
    assert!(
        matches!(resolution, crate::brep::graph::nurbs_pcurves::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        line_pcurve,
                    )) if {
                        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
                        (origin.u - 0.5).abs() <= f64::EPSILON * 64.0
                            && origin.v.abs() <= f64::EPSILON * 64.0
                            && direction.u.abs() <= f64::EPSILON * 64.0
                            && (direction.v - 1.0).abs() <= f64::EPSILON * 64.0
                    })
    );
    let clamped = crate::brep::graph::clamp::clamp_nurbs_curve_to_domain(&ctx, &curve, [0.0, 1.0])
        .expect("the clamped lanes are a curve")
        .expect("clamped quadratic segment");
    let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
        &ctx,
        &surface,
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        0.5,
    )
    .expect("resource allocation did not fail")
    .expect("quadratic surface isocurve");
    assert!(crate::brep::graph::nurbs_pcurves::nurbs_representation_matches(&ctx, &expected, &clamped).unwrap());
}

#[test]
fn extended_rational_isocurve_compares_weights_after_homogeneous_clamping() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
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
        Some(vec![1.0, 1.2, 1.0, 1.2]),
    );
    let curve = test_nurbs_curve(
        1,
        vec![-1.0, -1.0, 2.0, 2.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.5, -1.5, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 2.4 / 1.4, 0.0),
        ],
        Some(vec![0.8, 1.4]),
    );
    let resolution = crate::brep::graph::nurbs_pcurves::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.2, 0.8])
        .expect("isocurve lanes pair");
    assert!(
        matches!(resolution, crate::brep::graph::nurbs_pcurves::NurbsPcurveResolution::Exact(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        line_pcurve,
                    )) if {
                        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
                        (origin.u - 0.5).abs() <= f64::EPSILON * 64.0
                            && origin.v.abs() <= f64::EPSILON * 64.0
                            && direction.u.abs() <= f64::EPSILON * 64.0
                            && (direction.v - 1.0).abs() <= f64::EPSILON * 64.0
                    })
    );
    let clamped = crate::brep::graph::clamp::clamp_nurbs_curve_to_domain(&ctx, &curve, [0.0, 1.0])
        .expect("the clamped lanes are a curve")
        .expect("clamped rational segment");
    let expected = cadmpeg_ir::eval::nurbs_surface_isocurve(
        &ctx,
        &surface,
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        0.5,
    )
    .expect("resource allocation did not fail")
    .expect("rational surface isocurve");
    assert!(crate::brep::graph::nurbs_pcurves::nurbs_representation_matches(&ctx, &expected, &clamped).unwrap());
}

#[test]
fn degree_one_nurbs_cache_pcurve_keeps_measured_chordal_error() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        3,
        2,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0),
            cadmpeg_ir::math::Point3::new(0.5, 1.0, 1.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let curve = test_nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
    );
    let resolution = crate::brep::graph::nurbs_pcurves::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.0, 1.0])
        .expect("isocurve lanes pair");
    let crate::brep::graph::nurbs_pcurves::NurbsPcurveResolution::Cache {
        geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs },
        fit_tolerance,
    } = resolution
    else {
        panic!("degree-one cache was not accepted");
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots(), curve.knots());
    assert_eq!(nurbs.control_points().len(), 2);
    assert!(nurbs.weights().is_none());
    assert!(!nurbs.periodic());
    assert!(fit_tolerance > 0.4);
    assert!(fit_tolerance < 0.6);
}

#[test]
fn off_surface_nurbs_edge_is_classified_before_cache_inversion() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let surface = test_nurbs_surface(
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
    let curve = test_nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 10.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 10.0),
        ],
        None,
    );
    assert!(matches!(
        crate::brep::graph::nurbs_pcurves::derive_nurbs_edge_pcurve(&ctx, &surface, &curve, [0.0, 1.0])
            .expect("isocurve lanes pair"),
        crate::brep::graph::nurbs_pcurves::NurbsPcurveResolution::OffSurface
    ));
}

