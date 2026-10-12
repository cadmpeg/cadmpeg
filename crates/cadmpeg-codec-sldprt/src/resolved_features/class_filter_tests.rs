//! Class filtering admits traversal and text comparisons.

use crate::records::{FeatureInputClass, FeatureInputLane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn lane(classes: &[(&str, u64)]) -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: classes
            .iter()
            .enumerate()
            .map(|(ordinal, (name, offset))| FeatureInputClass {
                id: format!("class#{ordinal}"),
                parent: "lane".into(),
                ordinal: u32::try_from(ordinal).unwrap(),
                offset: *offset,
                name: (*name).into(),
            })
            .collect(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

#[test]
fn unmatched_class_filter_charges_operand_comparisons() {
    const OPERATION: &str = "test fixed SLDPRT class filter";
    const REPEATS: u64 = 64;
    let lane = lane(&[("moLineRef_x", 0), ("moLineRef_y", 1), ("moLineRef_z", 2)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..REPEATS {
        let mut storage = ctx.reserve_scoped(0, OPERATION).unwrap();
        assert!(
            super::sorted_classes(&ctx, &mut storage, &lane, "moLineRef_w", OPERATION)
                .unwrap()
                .is_empty()
        );
    }
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "compare SLDPRT filtered class name",
        |cap| {
            let arena = DecodeArena::new();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, OPERATION)?;
            let result = super::sorted_classes(&ctx, &mut storage, &lane, "moLineRef_w", OPERATION);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "compare SLDPRT filtered class name");
    assert!(limit.additional > 1);
}

#[test]
fn fixed_class_filter_keeps_stable_offset_order_and_exact_class_names() {
    let lane = lane(&[
        ("moLineRef_w", 20),
        ("moLineRef_x", 0),
        ("moLineRef_w", 10),
        ("moLineRef_w", 10),
    ]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut storage = ctx.reserve_scoped(0, "test sorted class names").unwrap();
    let classes = super::sorted_classes(
        &ctx,
        &mut storage,
        &lane,
        "moLineRef_w",
        "test sorted class names",
    )
    .unwrap();
    assert_eq!(
        classes
            .iter()
            .map(|class| class.ordinal)
            .collect::<Vec<_>>(),
        [2, 3, 0]
    );
}
