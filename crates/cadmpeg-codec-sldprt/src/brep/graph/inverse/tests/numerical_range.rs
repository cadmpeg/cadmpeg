// SPDX-License-Identifier: Apache-2.0

use crate::brep::graph::inverse::InverseResolution;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;
use crate::brep::graph::inverse::nurbs_parameter_at_point;
use crate::brep::graph::inverse::unique_inverse_parameter;
const INVERSE_FIT_TOLERANCE: f64 = 1e-6;

#[test]
fn numerical_0922_wide_domain_keeps_distinct_roots() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let r = unique_inverse_parameter(
        &ctx,
        vec![(-5e307, 0.), (5e307, 0.)],
        INVERSE_FIT_TOLERANCE,
        [-1e308, 1e308],
    )
    .unwrap();
    assert!(matches!(r, InverseResolution::Ambiguous));
}
#[test]
fn numerical_0922b_wide_curve_inverse() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    for d in [[0., 1.], [-1e308, 1e308], [1e308, 1.1e308]] {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![d[0], d[0], d[1], d[1]],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .unwrap();
        let r = nurbs_parameter_at_point(&ctx, &curve, Point3::new(0.3, 0., 0.)).unwrap();
        let result = match r {
            InverseResolution::Unique(p) => Some(p),
            InverseResolution::NoMatch => None,
            InverseResolution::Ambiguous => panic!("ambiguous"),
        };
        println!("SW chart{d:?}: inverse{result:?}");
        let hit = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &curve,
            result.unwrap(),
        )
        .unwrap();
        assert!((hit.x - 0.3).abs() < INVERSE_FIT_TOLERANCE);
    }
}
