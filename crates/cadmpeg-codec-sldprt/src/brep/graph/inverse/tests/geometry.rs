// SPDX-License-Identifier: Apache-2.0

use crate::brep::graph::test_support::test_nurbs_curve;
use crate::brep::graph::test_support::test_nurbs_surface;
use crate::brep::graph::test_support::with_test_context;
const EPS_EXPECTED_BILINEAR_PARAMETER: f64 = 1.0e-8;
const EPS_EXPECTED_PARAMETER: f64 = 1.0e-12;

#[test]
fn interior_ruled_surface_line_has_affine_isoparametric_inverse() {
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
            cadmpeg_ir::math::Point3::new(2.0, 1.0, 0.0),
        ],
        None,
    );
    let geometry = match with_test_context(|ctx| {
        crate::brep::graph::inverse::ruled_surface_line_pcurve(
            ctx,
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
            cadmpeg_ir::math::Point3::new(0.0, 0.5, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
    })
    .expect("ruling evaluation")
    {
        crate::brep::graph::inverse::InverseResolution::Unique(geometry) => geometry,
        crate::brep::graph::inverse::InverseResolution::NoMatch => panic!("interior ruling did not match"),
        crate::brep::graph::inverse::InverseResolution::Ambiguous => panic!("interior ruling was ambiguous"),
    };
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected affine line pcurve");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!(origin.u.abs() < EPS_EXPECTED_PARAMETER);
    assert!((origin.v - 0.5).abs() < EPS_EXPECTED_PARAMETER);
    assert!((direction.u - 2.0 / 3.0).abs() < EPS_EXPECTED_PARAMETER);
    assert!(direction.v.abs() < EPS_EXPECTED_PARAMETER);
}

#[test]
fn v_linear_surface_line_has_axis_symmetric_inverse() {
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
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.5, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let geometry = match with_test_context(|ctx| {
        crate::brep::graph::inverse::ruled_surface_line_pcurve(
            ctx,
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
            cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        )
    })
    .expect("ruling evaluation")
    {
        crate::brep::graph::inverse::InverseResolution::Unique(geometry) => geometry,
        crate::brep::graph::inverse::InverseResolution::NoMatch => panic!("transposed ruling did not match"),
        crate::brep::graph::inverse::InverseResolution::Ambiguous => panic!("transposed ruling was ambiguous"),
    };
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line_pcurve) = geometry else {
        panic!("expected affine line pcurve");
    };
    let origin = line_pcurve.origin().as_raw();
    let direction = line_pcurve.direction().as_raw();
    assert!((origin.u - 0.5).abs() < EPS_EXPECTED_BILINEAR_PARAMETER);
    assert!(origin.v.abs() < EPS_EXPECTED_PARAMETER);
    assert!(direction.u.abs() < EPS_EXPECTED_PARAMETER);
    assert!((direction.v - 1.0).abs() < EPS_EXPECTED_PARAMETER);
}

#[test]
fn repeated_ruled_surface_line_candidates_are_ambiguous() {
    let surface = test_nurbs_surface(
        1,
        2,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        2,
        3,
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
    );
    assert!(matches!(
        with_test_context(|ctx| crate::brep::graph::inverse::ruled_surface_line_pcurve(
            ctx,
            &surface,
            cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        ))
        .expect("ruling evaluation"),
        crate::brep::graph::inverse::InverseResolution::Ambiguous
    ));
}

#[test]
fn repeated_nurbs_endpoint_candidates_are_ambiguous() {
    let curve = test_nurbs_curve(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        ],
        None,
    );
    assert!(matches!(
        with_test_context(|ctx| crate::brep::graph::inverse::nurbs_parameter_at_point(
            ctx,
            &curve,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
        ))
        .expect("inverse evaluation"),
        crate::brep::graph::inverse::InverseResolution::Ambiguous
    ));
}

