//! Typed pattern lookup admits traversal and class text comparisons.

use crate::records::{Feature, FeatureInputClass, FeatureInputLane, FeatureInputName, ObjectId};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn unmatched_pattern_class_lookup_charges_operand_comparisons() {
    const REPEATS: u64 = 64;
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: ["moNumberDim_x", "moNumberDim_y", "moNumberDim_z"]
            .into_iter()
            .enumerate()
            .map(|(ordinal, name)| FeatureInputClass {
                id: format!("class#{ordinal}"),
                parent: "lane".into(),
                ordinal: u32::try_from(ordinal).unwrap(),
                offset: u64::try_from(ordinal).unwrap(),
                name: name.into(),
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
    };
    let feature = Feature {
        id: "pattern".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        ordinal: 0,
        name: "Pattern".into(),
        kind: String::new(),
        input_class: Some("moLPattern_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..REPEATS {
        assert_eq!(
            super::typed_linear_pattern_dimensions(&ctx, &feature, &lane, 0, 400).unwrap(),
            None
        );
    }
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "compare SLDPRT linear pattern class name",
        |cap| {
            let arena = DecodeArena::new();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::typed_linear_pattern_dimensions(&ctx, &feature, &lane, 0, 400);
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
    assert_eq!(limit.operation, "compare SLDPRT linear pattern class name");
    assert!(limit.additional > 1);
}

#[test]
fn linear_pattern_display_name_comparison_has_its_own_work_boundary() {
    const OPERATION: &str = "compare SLDPRT linear pattern display name";
    let mut payload = [0_u8; 185];
    payload[32..40].copy_from_slice(&1.0f64.to_le_bytes());
    payload[161..169].copy_from_slice(&1.0f64.to_le_bytes());
    for (value, spacing) in [("D3", [Some(1.0), None]), ("D4", [None, Some(1.0)])] {
        let names = [FeatureInputName {
            id: "display-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: Some(ObjectId::Absent),
            value: value.into(),
        }];
        let ctx = cadmpeg_test_support::service_decode_context();
        let directions = super::linear_pattern_display_directions(
            &ctx,
            &payload,
            0,
            payload.len(),
            &names,
            spacing,
        )
        .unwrap();
        assert_eq!(directions.len(), 1);
        assert_eq!(
            *directions[0].as_raw(),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
        );
        ctx.finish_session().unwrap();
        crate::test_support::work_refusal_at(OPERATION, |ctx| {
            super::linear_pattern_display_directions(
                ctx,
                &payload,
                0,
                payload.len(),
                &names,
                spacing,
            )
        });
    }
}
