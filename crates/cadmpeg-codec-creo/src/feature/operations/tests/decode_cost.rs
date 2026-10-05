use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::OperationKind;

#[test]
fn operation_kind_cost_counts_variant_tag_and_stored_text() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("root context");
    let stored = OperationKind::Stored(String::from("stored operation"));

    assert_eq!(
        DecodeCost::decode_cost(&stored, &ctx, "operation kind field cost")
            .expect("stored kind cost"),
        1 + cadmpeg_core::decode::u64_from_index("stored operation".len())
    );
    for kind in [OperationKind::Extrude, OperationKind::Revolve, OperationKind::Native] {
        assert_eq!(
            DecodeCost::decode_cost(&kind, &ctx, "operation kind field cost")
                .expect("fixed kind cost"),
            1
        );
    }
}

#[test]
fn operation_kind_equality_refuses_before_comparing_stored_text() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let kind = OperationKind::Stored(String::from("stored operation"));

    let error = ctx
        .equal(&kind, &kind, "creo feature operation kind comparison")
        .expect_err("the stored name exceeds the work budget");
    let CodecError::ResourceLimit(resource) = error else {
        panic!("expected a work refusal");
    };
    assert_eq!(resource.dimension, ResourceDimension::WorkUnits);
    assert_eq!(resource.operation, "creo feature operation kind comparison");
}
