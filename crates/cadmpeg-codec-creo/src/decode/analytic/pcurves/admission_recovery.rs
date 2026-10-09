// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn with_zero_limits(run: impl FnOnce(&DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(&ctx);
}

fn assert_free_original<T: PartialEq + std::fmt::Debug>(
    ctx: &DecodeContext<'_>,
    expected: T,
    mut run: impl FnMut() -> Result<T, CodecError>,
) {
    assert_eq!(run().expect("fixed recovery"), expected);
    assert!(ctx.resource_refusal().is_none());
    let original = ctx.charge_work_limit(1, "before fixed pcurve recovery")
        .expect_err("zero work cap");
    for _ in 0..2 {
        assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

fn plane() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).expect("plane"),
    ))
}

fn cylinder() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0), 2.0,
        ).expect("cylinder"),
    ))
}

fn cone(ratio: f64) -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0), 2.0, ratio, 0.25,
        ).expect("cone"),
    ))
}

fn sphere() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0), 2.0,
        ).expect("sphere"),
    ))
}

fn torus() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0), 3.0, 1.0,
        ).expect("torus"),
    ))
}

fn empty_two_chart() -> crate::curve::TwoChartPcurveSamples {
    crate::curve::TwoChartPcurveSamples { curve_id: 7, faces: [1, 2], samples: Vec::new(), offset: 0 }
}

#[test]
fn modern_pcurve_surface_filter_is_free_and_keeps_original_refusal() {
    with_zero_limits(|ctx| assert_free_original(ctx, HashSet::new(), ||
        topology_ignored_surface_ids(ctx, &crate::container::Layout::Nd, &[])));
}

#[test]
fn absent_pcurve_faces_are_free_and_keep_original_refusal() {
    let endpoints = [[[0.0, 0.0], [1.0, 0.0]], [[2.0, 0.0], [3.0, 0.0]]];
    let scan = crate::test_support::empty_container_scan();
    with_zero_limits(|ctx| assert_free_original(ctx, endpoints, ||
        canonicalized_pcurve_endpoints(ctx, &scan, [None; 2], endpoints[0], endpoints[1])));
}

#[test]
fn empty_two_chart_mapping_is_free_and_keeps_original_refusal() {
    let scan = crate::test_support::empty_container_scan();
    let ir = CadIr::empty();
    let pcurve = empty_two_chart();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    with_zero_limits(|ctx| {
        let index = SurfaceIndex::new(ctx, &ir.model.surfaces).expect("empty index");
        assert_free_original(ctx, true, || map_two_chart_endpoint_sets(
            ctx, &scan, &ir, &pcurve, &carriers, &index,
        ).map(|result| matches!(result, TwoChartMapping::NoSamples)));
    });
}

#[test]
fn empty_two_chart_endpoints_are_free_and_keep_original_refusal() {
    let scan = crate::test_support::empty_container_scan();
    let ir = CadIr::empty();
    let pcurve = empty_two_chart();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    with_zero_limits(|ctx| assert_free_original(ctx, None, ||
        mapped_two_chart_endpoint_sets(ctx, &scan, &ir, &pcurve, &carriers)));
}

#[test]
fn unsupported_pcurve_plane_pair_is_free_and_keeps_original_refusal() {
    let plane_carrier = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0; 3], normal: [0.0, 0.0, 1.0],
    });
    let cone_carrier = CarrierEquation::Cone(super::super::equations::ConeEquation::new(
        [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 2.0, 1.0, 0.25,
    ).expect("cone carrier"));
    with_zero_limits(|ctx| assert_free_original(ctx,
        PcurveCarrierStatus::Unknown(PcurveCarrierUnknownReason::UnsupportedPair), ||
        pcurve_plane_carrier_status(ctx, &plane(), plane_carrier, cone_carrier,
            [[0.0, 0.0], [1.0, 0.0]])));
}

#[test]
fn parallel_pcurve_plane_pair_is_free_and_keeps_original_refusal() {
    let carrier = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0; 3], normal: [0.0, 0.0, 1.0],
    });
    with_zero_limits(|ctx| assert_free_original(ctx,
        PcurveCarrierStatus::Unknown(PcurveCarrierUnknownReason::ParallelPlanePair), ||
        pcurve_plane_carrier_status(ctx, &plane(), carrier, carrier,
            [[0.0, 0.0], [1.0, 0.0]])));
}

#[test]
fn nonplane_pcurve_carrier_path_is_free_and_keeps_original_refusal() {
    let carrier = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0; 3], normal: [0.0, 0.0, 1.0],
    });
    let crossing = CarrierEquation::Plane(PlaneEquation {
        origin: [0.0; 3], normal: [1.0, 0.0, 0.0],
    });
    with_zero_limits(|ctx| assert_free_original(ctx,
        PcurveCarrierStatus::Unknown(PcurveCarrierUnknownReason::UnsupportedPath), ||
        pcurve_plane_carrier_status(ctx, &cylinder(), carrier, crossing,
            [[0.0, 0.0], [1.0, 0.0]])));
}

#[test]
fn missing_pcurve_path_surface_is_free_and_keeps_original_refusal() {
    let ir = CadIr::empty();
    let carriers = BTreeMap::new();
    let source = crate::decode::source_carriers::SourceUnitCarriers::default();
    with_zero_limits(|ctx| {
        let index = SurfaceIndex::new(ctx, &ir.model.surfaces).expect("empty index");
        assert_free_original(ctx,
            PcurveCarrierStatus::Unknown(PcurveCarrierUnknownReason::MissingSurface), ||
            pcurve_path_carrier_status(ctx, &ir, &carriers, ([NonZeroU32::new(1), None], 0),
                [[0.0, 0.0], [1.0, 0.0]], &source, &index));
    });
}

#[test]
fn missing_pcurve_endpoint_surface_is_free_and_keeps_original_refusal() {
    let ir = CadIr::empty();
    let carriers = BTreeMap::new();
    let source = crate::decode::source_carriers::SourceUnitCarriers::default();
    with_zero_limits(|ctx| {
        let index = SurfaceIndex::new(ctx, &ir.model.surfaces).expect("empty index");
        assert_free_original(ctx,
            PcurveCarrierStatus::Unknown(PcurveCarrierUnknownReason::MissingSurface), ||
            pcurve_endpoint_carrier_status(ctx, &ir, &carriers, ([NonZeroU32::new(1), None], 0),
                [[0.0, 0.0], [1.0, 0.0]], &source, &index));
    });
}

#[test]
fn absent_support_cone_face_is_free_and_keeps_original_refusal() {
    with_zero_limits(|ctx| {
        let mut witnesses = BTreeMap::new();
        assert_free_original(ctx, (), || collect_support_cone_plane_witness(
            ctx, &mut witnesses, &BTreeMap::new(), [NonZeroU32::new(1), None], [None; 2]));
        assert!(witnesses.is_empty());
    });
}

#[test]
fn absent_support_cone_endpoints_are_free_and_keep_original_refusal() {
    with_zero_limits(|ctx| {
        let mut witnesses = BTreeMap::new();
        assert_free_original(ctx, (), || collect_support_cone_plane_witness(
            ctx, &mut witnesses, &BTreeMap::new(), [NonZeroU32::new(1), NonZeroU32::new(2)], [None; 2]));
        assert!(witnesses.is_empty());
    });
}

#[test]
fn empty_mapped_pcurve_evidence_is_free_and_keeps_original_refusal() {
    with_zero_limits(|ctx| assert_free_original(ctx, true, ||
        pcurve_endpoint_evidence_from_mapped(ctx, &[], false).map(|result| result.is_none())));
}

#[test]
fn empty_pcurve_mismatch_is_free_and_keeps_original_refusal() {
    with_zero_limits(|ctx| assert_free_original(ctx, true, ||
        pcurve_mismatch_detail(ctx, 7, &[]).map(|result| result.is_none())));
}

#[test]
fn single_path_pcurve_mismatch_is_free_and_keeps_original_refusal() {
    let mapped = [MappedPcurvePath { face_id: 1, endpoints: [[0.0; 3], [1.0, 0.0, 0.0]] }];
    with_zero_limits(|ctx| assert_free_original(ctx, true, ||
        pcurve_mismatch_detail(ctx, 7, &mapped).map(|result| result.is_none())));
}

#[test]
fn equal_linear_pcurve_endpoints_are_free_and_keep_original_refusal() {
    with_zero_limits(|ctx| assert_free_original(ctx, None, ||
        linear_pcurve_carrier(ctx, &plane(), [[0.0; 2]; 2])));
}

#[test]
fn unsupported_linear_pcurve_path_is_free_and_keeps_original_refusal() {
    with_zero_limits(|ctx| assert_free_original(ctx, None, ||
        linear_pcurve_carrier(ctx, &cylinder(), [[0.0, 0.0], [1.0, 1.0]])));
}

fn assert_fixed_carrier(surface: SurfaceGeometry, endpoints: [[f64; 2]; 2], expected: &str) {
    with_zero_limits(|ctx| assert_free_original(ctx, true, ||
        linear_pcurve_carrier(ctx, &surface, endpoints).map(|geometry| matches!((expected, geometry),
            ("line", Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(_))))
            | ("circle", Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))))
            | ("ellipse", Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(_))))
        ))));
}

#[test]
fn fixed_cylinder_generator_is_free_and_keeps_original_refusal() {
    assert_fixed_carrier(cylinder(), [[0.0, 0.0], [0.0, 1.0]], "line");
}

#[test]
fn fixed_cylinder_parallel_is_free_and_keeps_original_refusal() {
    assert_fixed_carrier(cylinder(), [[0.0, 0.0], [1.0, 0.0]], "circle");
}

#[test]
fn fixed_cone_circle_is_free_and_keeps_original_refusal() {
    assert_fixed_carrier(cone(1.0), [[0.0, 0.0], [1.0, 0.0]], "circle");
}

#[test]
fn fixed_cone_ellipse_is_free_and_keeps_original_refusal() {
    assert_fixed_carrier(cone(0.5), [[0.0, 0.0], [1.0, 0.0]], "ellipse");
}

#[test]
fn fixed_sphere_meridian_is_free_and_keeps_original_refusal() {
    assert_fixed_carrier(sphere(), [[0.0, 0.0], [0.0, 1.0]], "circle");
}

#[test]
fn fixed_sphere_parallel_is_free_and_keeps_original_refusal() {
    assert_fixed_carrier(sphere(), [[0.0, 0.0], [1.0, 0.0]], "circle");
}

#[test]
fn fixed_torus_meridian_is_free_and_keeps_original_refusal() {
    assert_fixed_carrier(torus(), [[0.0, 0.0], [0.0, 1.0]], "circle");
}

#[test]
fn fixed_torus_parallel_is_free_and_keeps_original_refusal() {
    assert_fixed_carrier(torus(), [[0.0, 0.0], [1.0, 0.0]], "circle");
}

#[test]
fn primitive_planar_projection_is_free_and_keeps_original_refusal() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0),
        ).expect("line"),
    ));
    with_zero_limits(|ctx| assert_free_original(ctx, true, || planar_curve_pcurve(
        ctx, &plane(), &line, &"line", &mut crate::lane_refusal::LaneRefusals::new(),
    ).map(|result| matches!(result, Some(PcurveGeometry::Line(_))))));
}

#[test]
fn nonplane_primitive_projection_is_free_and_keeps_original_refusal() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0),
        ).expect("line"),
    ));
    with_zero_limits(|ctx| assert_free_original(ctx, None, || planar_curve_pcurve(
        ctx, &cylinder(), &line, &"line", &mut crate::lane_refusal::LaneRefusals::new(),
    )));
}
