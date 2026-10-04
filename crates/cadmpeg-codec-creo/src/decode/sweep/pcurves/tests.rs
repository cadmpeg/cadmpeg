// SPDX-License-Identifier: Apache-2.0

use super::{
    add_extrusion_pcurve, nurbs_sense_sample, revolution_boundary_pcurve, PcurveAdmission,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::RevolutionAxis;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn extrusion_pcurve_identity_copy_refuses_below_retained_limit() {
    let id = PcurveId::mint("creo:feature:extrusion#7:pcurve:cap").expect("identity grammar");
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
    ));
    let geometry = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).expect("line"),
    );
    let source = crate::decode::source_carriers::SourceUnitCarriers::default();
    // Admit preceding annotation backing nodes before refusing the identity copy.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "creo extrusion pcurve identity copy",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            add_extrusion_pcurve(
                &ctx,
                &mut CadIr::empty(),
                &mut AnnotationBuilder::new(),
                PcurveAdmission::Pending(&source, &surface),
                id.clone(),
                0,
                geometry.clone(),
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo extrusion pcurve identity copy"));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let result = add_extrusion_pcurve(
        &ctx,
        &mut CadIr::empty(),
        &mut AnnotationBuilder::new(),
        PcurveAdmission::Pending(&source, &surface),
        id.clone(),
        0,
        geometry,
    )
    .expect("service pcurve");
    assert_eq!(result, id);
}

#[test]
fn revolution_nurbs_sense_samples_a_wide_finite_parameter_range() {
    let (parameter, epsilon) = nurbs_sense_sample(-f64::MAX, f64::MAX);
    assert_eq!(parameter, 0.0);
    assert!(epsilon.is_finite());
    assert!((epsilon / f64::MAX - 2.0e-6).abs() <= 4.0 * f64::EPSILON);
}

#[test]
fn spindle_torus_boundary_pcurve_retains_the_signed_ring_branch() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            5.0,
        )
        .expect("valid TorusSurface fixture"),
    ));
    let axis = RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite point fixture"),
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
            .expect("valid direction fixture"),
        reference: None,
    };
    let pcurve = crate::decode::with_test_decode_ctx(|ctx| {
        revolution_boundary_pcurve(
            ctx,
            &surface,
            [-3.0, 0.0, 0.0],
            &axis,
            &"revolution boundary fixture",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
    })
    .expect("resource admission")
    .expect("spindle boundary");
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &pcurve,
            parameter,
        )
        .expect("pcurve point");
        let point = cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &surface,
            uv.u,
            uv.v,
        )
        .expect("surface point");
        assert!((point.x.hypot(point.y) - 3.0).abs() < 1.0e-12);
        assert!(point.z.abs() < 1.0e-12);
    }
}
