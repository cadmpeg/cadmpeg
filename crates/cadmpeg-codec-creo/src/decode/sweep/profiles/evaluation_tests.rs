// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

#[test]
fn profile_sampling_evaluates_a_spline_beyond_the_inline_basis_window() {
    // Degree 16 is the smallest support that exceeds the inline basis window.
    let poles = 17_usize;
    let nurbs = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        16,
        std::iter::repeat_n(0.0, poles)
            .chain(std::iter::repeat_n(1.0, poles))
            .collect::<Vec<f64>>(),
        (0..poles)
            .map(|index| {
                Point3::new(
                    f64::from(u32::try_from(index).expect("pole")) / 16.0,
                    0.0,
                    0.0,
                )
            })
            .collect(),
        None,
        false,
    )
    .expect("fixture constructor admission")
    .expect("line spline");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut evaluator = cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(&nurbs);
    assert_eq!(
        super::nurbs_profile_point(&ctx, &mut evaluator, &nurbs, 0.5).expect("service"),
        Some([0.5, 0.0])
    );
}
