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
        crate::brep::graph::nurbs_pcurves::nurbs_strict_isocurve_pcurve(&ctx, &surface, &curve).unwrap(),
        crate::brep::graph::inverse::InverseResolution::NoMatch
    ));
    ctx.finish_session().unwrap();
}
