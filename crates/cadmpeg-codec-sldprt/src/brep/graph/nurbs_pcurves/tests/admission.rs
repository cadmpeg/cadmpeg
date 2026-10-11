// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeArena;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::DecodePolicy;

#[test]
fn nurbs_isocurve_size_mismatch_needs_no_knot_comparison_work() {
    use cadmpeg_ir::geometry::nurbs::{
        NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
    };
    use cadmpeg_ir::math::Point3;
    let service = cadmpeg_test_support::service_decode_context();
    let surface = NurbsSurface::from_lanes(
        &service,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .unwrap()
    .unwrap();
    let curve = NurbsCurve::from_lanes(
        &service,
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.5, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap()
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::brep::graph::nurbs_pcurves::nurbs_strict_isocurve_pcurve(&ctx, &surface, &curve)
            .unwrap(),
        crate::brep::graph::inverse::InverseResolution::NoMatch
    ));
    ctx.finish_session().unwrap();
}

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;

fn projection_source() -> (
    cadmpeg_ir::geometry::nurbs::NurbsSurface,
    cadmpeg_ir::geometry::nurbs::NurbsCurve,
) {
    use cadmpeg_ir::math::Point3;
    let surface = crate::brep::graph::test_support::test_nurbs_surface(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        3,
        2,
        &[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    let curve = crate::brep::graph::test_support::test_nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        None,
    );
    (surface, curve)
}

#[test]
fn boundary_comparison_releases_each_isocurve_and_keeps_no_retained_geometry() {
    use cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis;
    let (surface, curve) = projection_source();
    // Degree2 basis minimum4 f64 slots, two exact raw Point3 slots,
    // four knot slots, and two exact converted FinitePoint3 slots coexist.
    let peak = u64::try_from(
        8 * std::mem::size_of::<f64>()
            + 2 * std::mem::size_of::<cadmpeg_ir::math::Point3>()
            + 2 * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>(),
    )
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let super::super::InverseResolution::Unique(geometry) =
            super::super::nurbs_boundary_pcurve(&ctx, &surface, &curve, SurfaceParameterAxis::U)
                .unwrap()
        else {
            panic!("the original boundary remains unique");
        };
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line) = geometry else {
            panic!("boundary comparison produces the fixed line");
        };
        assert_eq!(
            line.origin().as_raw(),
            &cadmpeg_ir::math::Point2::new(0.0, 0.0)
        );
        assert_eq!(
            line.direction().as_raw(),
            &cadmpeg_ir::math::Point2::new(0.0, 1.0)
        );
        drop(
            ctx.reserve_scoped(peak, "reuse released boundary comparison")
                .unwrap(),
        );
    }
    ctx.finish_session().unwrap();
}

#[test]
fn boundary_comparison_refuses_exact_converted_pole_peak_and_keeps_fuse() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis;
    let (surface, curve) = projection_source();
    let prior = u64::try_from(
        8 * std::mem::size_of::<f64>() + 2 * std::mem::size_of::<cadmpeg_ir::math::Point3>(),
    )
    .unwrap();
    let added =
        u64::try_from(2 * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = prior + added - 1;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        super::super::nurbs_boundary_pcurve(&ctx, &surface, &curve, SurfaceParameterAxis::U)
            .err()
            .expect("comparison peak refusal")
    else {
        panic!("one byte below actual comparison peak refuses");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "IR NURBS admitted poles");
    assert_eq!((limit.used, limit.additional), (prior, added));
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn cache_controls_keep_only_their_live_backing_and_release_it_after_fit() {
    let (surface, curve) = projection_source();
    let raw = u64::try_from(4 * std::mem::size_of::<cadmpeg_ir::math::Point2>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let (controls, fit) =
            super::super::nurbs_degree_one_cache_lanes(&ctx, &surface, &curve, [0.0, 1.0])
                .unwrap()
                .unwrap();
        assert_eq!(
            controls.values(),
            [
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(0.0, 1.0)
            ]
        );
        assert_eq!(fit, 0.0);
        drop(
            ctx.reserve_scoped(SCRATCH_BYTES - raw, "probe live raw cache controls")
                .unwrap(),
        );
        drop(controls);
        drop(
            ctx.reserve_scoped(SCRATCH_BYTES, "reuse consumed cache controls")
                .unwrap(),
        );
    }
    ctx.finish_session().unwrap();
}

#[test]
fn cache_control_reservation_refuses_exact_backing_before_projection() {
    use cadmpeg_core::decode::ResourceDimension;
    let (surface, curve) = projection_source();
    let raw = u64::try_from(4 * std::mem::size_of::<cadmpeg_ir::math::Point2>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = raw - 1;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        super::super::nurbs_degree_one_cache_lanes(&ctx, &surface, &curve, [0.0, 1.0])
    });
    let Err(super::super::NurbsPcurveFailure::Resource(cadmpeg_core::CodecError::ResourceLimit(
        limit,
    ))) = result
    else {
        panic!("one byte below actual raw cache backing refuses");
    };
    assert_eq!(allocations, 0);
    assert_eq!(limit.operation, "collect NURBS cache pcurve controls");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional), (0, raw));
    drop(result);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(replay)) if replay == limit)
    );
}

#[test]
fn cache_conversion_retains_only_knots_and_finite_poles() {
    let (surface, curve) = projection_source();
    let output = u64::try_from(
        4 * std::mem::size_of::<f64>() + 2 * std::mem::size_of::<cadmpeg_ir::units::FinitePoint2>(),
    )
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = output;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (geometry, fit) =
        super::super::nurbs_degree_one_cache_pcurve(&ctx, &surface, &curve, [0.0, 1.0])
            .unwrap()
            .unwrap();
    assert_eq!(fit, 0.0);
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &geometry else {
        panic!("the original degree-one cache produces the pcurve");
    };
    assert_eq!(nurbs.knots().as_slice(), curve.knots().as_slice());
    assert_eq!(nurbs.pole_rows().count(), 2);
    drop(
        ctx.reserve_scoped(
            SCRATCH_BYTES,
            "reuse cache conversion workspace with output alive",
        )
        .unwrap(),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx
        .copy_retained_text("x", "probe final cache backing")
        .unwrap_err()
    else {
        panic!("actual final cache backing remains retained");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert_eq!((limit.used, limit.additional), (output, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn strict_isocurve_first_knot_mismatch_needs_no_tail_work() {
    use crate::brep::graph::inverse::InverseResolution;
    use crate::brep::graph::nurbs_pcurves::nurbs_strict_isocurve_pcurve;
    use cadmpeg_ir::math::Point3;
    let (surface, _) = projection_source();
    let curve = crate::brep::graph::test_support::test_nurbs_curve(
        1,
        vec![-1.0, -1.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        None,
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        nurbs_strict_isocurve_pcurve(&ctx, &surface, &curve).unwrap(),
        InverseResolution::NoMatch
    ));
    ctx.finish_session().unwrap();
}

#[test]
fn boundary_isocurve_knot_comparison_refuses_one_visited_pair() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis;
    let (surface, curve) = projection_source();
    let error =
        crate::test_support::work_refusal_at("compare Parasolid boundary curve knots", |ctx| {
            super::super::nurbs_boundary_pcurve(ctx, &surface, &curve, SurfaceParameterAxis::U)
        });
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("knot comparison refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.additional, 1);
}

#[test]
fn extended_isocurve_comparison_releases_every_clamped_curve() {
    use cadmpeg_ir::math::Point3;
    let surface = crate::brep::graph::test_support::test_nurbs_surface(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        &[
            Point3::new(0., 0., 0.),
            Point3::new(0., 1., 0.),
            Point3::new(1., 0., 0.),
            Point3::new(1., 1., 0.),
        ],
        None,
    );
    let curve = crate::brep::graph::test_support::test_nurbs_curve(
        1,
        vec![-1.0, -1.0, 2.0, 2.0],
        vec![Point3::new(0.5, -1., 0.), Point3::new(0.5, 2., 0.)],
        None,
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..4 {
        let result = super::super::extended_nurbs_isocurve_pcurve(&ctx, &surface, &curve).unwrap();
        let super::super::InverseResolution::Unique(
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line),
        ) = result
        else {
            panic!("exact clamped isocurve line");
        };
        assert_eq!(
            line.origin().as_raw(),
            &cadmpeg_ir::math::Point2::new(0.5, 0.0)
        );
        assert_eq!(
            line.direction().as_raw(),
            &cadmpeg_ir::math::Point2::new(0.0, 1.0)
        );
        drop(
            ctx.reserve_scoped(SCRATCH_BYTES, "test released extended clamp")
                .unwrap(),
        );
    }
    ctx.finish_session().unwrap();
}

#[test]
fn missing_start_inverse_skips_end_inverse_search() {
    use crate::brep::graph::inverse::{nurbs_parameter_at_point, InverseResolution};
    use crate::brep::graph::nurbs_pcurves::nurbs_edge_parameter_range;
    use cadmpeg_ir::ids::{EdgeId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};
    let curve = crate::brep::graph::test_support::test_nurbs_curve(
        1,
        vec![0., 0., 1., 2., 3., 3.],
        (0..4).map(|index| Point3::new(f64::from(index), 0., 0.)).collect(),
        None,
    );
    let start = Point3::new(0., 1., 0.);
    let end = Point3::new(3., 0., 0.);
    let boundary = crate::test_support::work_refusal_at("test completed first endpoint inverse", |ctx| {
        assert!(matches!(nurbs_parameter_at_point(ctx, &curve, start)?, InverseResolution::NoMatch));
        ctx.charge_work(1, "test completed first endpoint inverse")
    });
    let cadmpeg_core::CodecError::ResourceLimit(boundary) = boundary else {
        panic!("completed first inverse boundary");
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = boundary.used;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let edge = Edge {
        id: EdgeId::mint("test:model:edge#inverse").unwrap(),
        carrier: EdgeCarrier::new(None, None).unwrap(),
        start: VertexId::mint("test:model:vertex#start").unwrap(),
        end: VertexId::mint("test:model:vertex#end").unwrap(),
        tolerance: None,
    };
    assert_eq!(nurbs_edge_parameter_range(&ctx, &edge, &curve, || Ok(Some([start, end]))).unwrap(), None);
    ctx.finish_session().unwrap();
}
