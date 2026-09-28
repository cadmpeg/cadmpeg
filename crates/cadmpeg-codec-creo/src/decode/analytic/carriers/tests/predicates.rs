// SPDX-License-Identifier: Apache-2.0

use super::super::*;

fn valid_parameter_polygon_service(polygon: &[[f64; 2]]) -> bool {
    crate::decode::with_test_decode_ctx(|ctx| valid_parameter_polygon(ctx, polygon))
        .expect("service polygon validation")
}

#[test]
fn valid_parameter_polygon_refuses_normalized_points() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let error = valid_parameter_polygon(&ctx, &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])
        .expect_err("normalized points exceed limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo normalized polygon points"));
}
#[test]
fn numerical_audit_polygon_incidence_ignores_length_scale() {
    for scale in [1., 1e-5] {
        assert!(segments_intersect(
            [[-scale, 0.], [scale, 0.]],
            [[0., -scale], [0., scale]]
        ));
        assert!(polygon_strictly_contains(
            &[[-scale, 0.], [0., -scale], [scale, 0.], [0., scale]],
            [0., 0.]
        ));
        assert!(!polygon_strictly_contains(
            &[[-scale, 0.], [0., -scale], [scale, 0.], [0., scale]],
            [scale, 0.]
        ));
    }
}
#[test]
fn numerical_audit_polygon_admission_ignores_translation() {
    for offset in [0., 1e4] {
        let p = [[0., 0.], [0.001, 0.], [0.001, 0.001], [0., 0.001]]
            .map(|p| [p[0] + offset, p[1] + offset]);
        assert!(valid_parameter_polygon_service(&p));
    }
    assert!(!valid_parameter_polygon_service(&[[0., 0.], [1., 0.], [2., 0.]]));
}
