// SPDX-License-Identifier: Apache-2.0
use super::super::PrimitiveTriangleStrip;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn empty_strip_validation_is_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(
        PrimitiveTriangleStrip::new(&ctx, 17, Vec::new(), None, Vec::new())
            .expect("no span")
            .is_none()
    );

}

#[test]
fn strip_validation_admits_present_spans_and_stops_at_the_first_invalid_span() {
    for (spans, vertices, valid) in [([2, 3], 3, false), ([3, 2], 6, false), ([3, 3], 6, true)] {
        let result = super::work_output(|ctx| {
            PrimitiveTriangleStrip::new(
                ctx,
                17,
                super::finite_points(vec![[0.0; 3]; vertices]),
                None,
                spans.to_vec(),
            )
        });
        if valid {
            let strip = result.expect("complete source strip");
            assert_eq!(strip.offset, 17);
            assert_eq!(strip.strip_lengths(), &spans);
            assert_eq!(strip.positions().len(), vertices);
            assert!(strip.positions().all(|position| position.get() == [0.0; 3]));
            assert!(strip.normals().is_none());
        } else {
            assert!(result.is_none());
        }
    }
}
