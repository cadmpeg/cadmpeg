// SPDX-License-Identifier: Apache-2.0

const TINY_PARAMETER_DOMAIN: f64 = 1e-12;

#[test]
fn inverse_coordinate_scale_admits_each_point() {
    let points = [
        cadmpeg_ir::math::Point3::new(2.0, 3.0, 4.0),
        cadmpeg_ir::math::Point3::new(5.0, 6.0, 7.0),
    ];
    crate::test_support::work_refusal_at("measure Parasolid inverse coordinate scale", |ctx| {
        crate::brep::graph::inverse::inverse_coordinate_tolerance(ctx, points)
    });
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        crate::brep::graph::inverse::inverse_coordinate_tolerance(&ctx, []).unwrap(),
        crate::brep::graph::inverse::INVERSE_ABSOLUTE_TOLERANCE_MM
    );
    assert_eq!(
        crate::brep::graph::inverse::inverse_coordinate_tolerance(&ctx, points).unwrap(),
        crate::brep::graph::inverse::looser_tolerance(
            crate::brep::graph::inverse::INVERSE_ABSOLUTE_TOLERANCE_MM,
            7.0 * crate::brep::graph::inverse::INVERSE_RELATIVE_TOLERANCE
        )
    );
}
#[test]
fn numerical_followup_inverse_ambiguity_is_independent_of_parameter_units() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    for domain in [1e-200, TINY_PARAMETER_DOMAIN, 1.0, 1e200] {
        assert!(matches!(
            crate::brep::graph::inverse::unique_inverse_parameter(
                &ctx,
                vec![(0.25 * domain, 0.), (0.75 * domain, 0.)],
                0.001,
                [0., domain]
            )
            .unwrap(),
            crate::brep::graph::inverse::InverseResolution::Ambiguous
        ));
        assert!(matches!(
            crate::brep::graph::inverse::unique_inverse_parameter(
                &ctx,
                vec![(0.25 * domain, 0.), (0.25 * domain, 0.)],
                0.001,
                [0., domain]
            )
            .unwrap(),
            crate::brep::graph::inverse::InverseResolution::Unique(_)
        ));
    }
}
