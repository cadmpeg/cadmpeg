//! Typed pattern lookup admits visits while fixed class comparisons stay free.

use crate::records::{Feature, FeatureInputClass, FeatureInputLane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn unmatched_pattern_class_lookup_pays_its_visits_and_end_probe() {
    const OPERATION: &str = "look up SLDPRT linear pattern dimensions";
    const VISITS_AND_END: u64 = 4;
    const REPEATS: u64 = 64;
    let lane = FeatureInputLane {
        id: "lane".into(), configuration: None, native_payload: Vec::new(),
        classes: ["moNumberDim_x", "moNumberDim_y", "moNumberDim_z"]
            .into_iter().enumerate().map(|(ordinal, name)| FeatureInputClass {
                id: format!("class#{ordinal}"), parent: "lane".into(),
                ordinal: u32::try_from(ordinal).unwrap(), offset: u64::try_from(ordinal).unwrap(),
                name: name.into(),
            }).collect(),
        names: Vec::new(), scalars: Vec::new(), relation_bindings: Vec::new(),
        relation_instances: Vec::new(), body_selections: Vec::new(), edge_selections: Vec::new(),
        surface_selections: Vec::new(), generated_surface_identities: Vec::new(),
        references: Vec::new(), sketch_entities: Vec::new(),
    };
    let feature = Feature {
        id: "pattern".into(), parent: "history".into(), xml_tag: "Feature".into(),
        tree_parent: None, source_id: None, ordinal: 0, name: "Pattern".into(),
        kind: String::new(), input_class: Some("moLPattern_c".into()), suppressed: false,
        parameters: BTreeMap::new(), dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(), text: None, content: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = VISITS_AND_END * REPEATS;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..REPEATS {
        assert_eq!(super::typed_linear_pattern_dimensions(&ctx, &feature, &lane, 0, 400)
            .unwrap(), None);
    }
    let arena = DecodeArena::new();
    policy.limits.max_work_units = VISITS_AND_END - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = super::typed_linear_pattern_dimensions(
        &ctx, &feature, &lane, 0, 400,
    ).unwrap_err() else { panic!("work refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.used, 3);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
