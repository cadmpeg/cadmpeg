//! Input identity filtering and retained publication boundaries.

use crate::records::{Feature, FeatureInputComponentPathEntry, FeatureSource};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fixture() -> (Vec<Feature>, Vec<FeatureInputComponentPathEntry>) {
    let feature = |id: &str, source, parent: &str| Feature {
        id: id.into(),
        parent: parent.into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal: 0,
        name: String::new(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: std::collections::BTreeMap::new(),
        dimension_properties: std::collections::BTreeMap::new(),
        properties: std::collections::BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let features = vec![
        feature("a", 10, "history"),
        feature("b", 20, "history"),
        feature("future", 40, "history"),
        feature("consumer", 30, "history"),
        feature("ambiguous-a", 50, "history"),
        feature("ambiguous-b", 50, "history"),
        feature("z", 60, "history"),
        feature("z", 15, "history"),
        feature("other-history", 5, "other"),
    ];
    let components = [20u32, 10, 20, 40, 30, 50, 15, 5]
        .into_iter()
        .map(|source| {
            let mut signature = [0; 12];
            signature[4..8].copy_from_slice(&source.to_le_bytes());
            FeatureInputComponentPathEntry {
                instance: Some(0x8032),
                type_signature: signature,
                local_id: Some(7),
            }
        })
        .collect();
    (features, components)
}

fn retained_policy(bytes: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = bytes;
    policy
}

#[test]
fn input_feature_projection_retains_only_preceding_identity_bytes() {
    let (features, components) = fixture();
    // Core amortized Vec<String> storage starts at four slots. Only b and a are copied.
    let slots = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<String>());
    for retained in [slots + 2, slots + 1] {
        let arena = DecodeArena::new();
        let policy = retained_policy(retained);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result =
            super::super::component_path_input_features(&ctx, &components, &features, "consumer");
        if retained == slots + 2 {
            assert_eq!(result.unwrap(), ["b", "a"]);
            ctx.finish_session().unwrap();
        } else {
            let CodecError::ResourceLimit(limit) = result.unwrap_err() else {
                panic!("retained identity refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "resolve SLDPRT component path producers");
            assert_eq!(limit.used, slots + 1);
            assert_eq!(limit.additional, 1);
            let CodecError::ResourceLimit(repeated) = super::super::component_path_input_features(
                &ctx,
                &components,
                &features,
                "consumer",
            )
            .unwrap_err() else {
                panic!("original refusal");
            };
            assert_eq!(repeated, limit);
            let CodecError::ResourceLimit(finished) = ctx.finish_session().unwrap_err() else {
                panic!("sticky session refusal");
            };
            assert_eq!(finished, limit);
        }
    }
}

#[test]
fn input_feature_projection_missing_consumer_keeps_no_identity_bytes() {
    let (features, components) = fixture();
    let arena = DecodeArena::new();
    let policy = retained_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        super::super::component_path_input_features(&ctx, &components, &features, "absent",)
            .unwrap()
            .is_empty()
    );
    ctx.finish_session().unwrap();
}

#[test]
fn edge_owner_fallback_copies_only_last_input_identity() {
    let (features, components) = fixture();
    for retained in [1, 0] {
        let arena = DecodeArena::new();
        let policy = retained_policy(retained);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let query = || {
            crate::resolved_features::selections::compact_edge_owner_feature_at(
                &ctx,
                &fallback_payload(),
                12,
                &components,
                &features,
                "consumer",
            )
        };
        let result = query();
        if retained == 1 {
            assert_eq!(result.unwrap().as_deref(), Some("a"));
            ctx.finish_session().unwrap();
        } else {
            let CodecError::ResourceLimit(limit) = result.unwrap_err() else {
                panic!("retained owner refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "resolve SLDPRT compact edge owner");
            assert_eq!(limit.used, 0);
            assert_eq!(limit.additional, 1);
            let CodecError::ResourceLimit(repeated) = query().unwrap_err() else {
                panic!("original owner refusal");
            };
            assert_eq!(repeated, limit);
            let CodecError::ResourceLimit(finished) = ctx.finish_session().unwrap_err() else {
                panic!("sticky session refusal");
            };
            assert_eq!(finished, limit);
        }
    }
}

#[test]
fn edge_producer_fallback_reuses_last_input_identity() {
    let (features, components) = fixture();
    let slots = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<String>());
    let arena = DecodeArena::new();
    let policy = retained_policy(slots + 2);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        crate::resolved_features::selections::compact_edge_producer_features_at(
            &ctx,
            &fallback_payload(),
            12,
            &components,
            &features,
            "consumer",
        )
        .unwrap(),
        ["b", "a"]
    );
    ctx.finish_session().unwrap();
}

fn fallback_payload() -> [u8; 50] {
    let mut payload = [0; 50];
    payload[..4].copy_from_slice(&1u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[12..28]
        .copy_from_slice(&crate::resolved_features::selections::COMPACT_EDGE_VECTOR_MARKER);
    payload[30..34].copy_from_slice(&[0x3d, 0x80, 0, 0]);
    payload[34..38].copy_from_slice(&[0x34, 0x80, 0x37, 0]);
    payload[38..42].copy_from_slice(&20u32.to_le_bytes());
    payload[42..46].copy_from_slice(&1u32.to_le_bytes());
    payload[46..50].copy_from_slice(&7u32.to_le_bytes());
    payload
}

fn explicit_owner_payload() -> [u8; 86] {
    let mut payload = [0; 86];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[12..28]
        .copy_from_slice(&crate::resolved_features::selections::COMPACT_EDGE_VECTOR_MARKER);
    payload[30..34].copy_from_slice(&[0x3d, 0x80, 0, 0]);
    payload[34..38].copy_from_slice(&[0x34, 0x80, 0x37, 0]);
    payload[38..42].copy_from_slice(&20u32.to_le_bytes());
    payload[42..46].copy_from_slice(&1u32.to_le_bytes());
    payload[46..50].copy_from_slice(&7u32.to_le_bytes());
    payload[50] = 1;
    payload[58..62].copy_from_slice(&[0x4a, 0x80, 0, 0]);
    payload[62..66].copy_from_slice(&[0x34, 0x80, 0x37, 0]);
    payload[66..70].copy_from_slice(&20u32.to_le_bytes());
    payload[70..74].copy_from_slice(&1u32.to_le_bytes());
    payload
}

#[test]
fn edge_explicit_owner_keeps_only_selected_identity_bytes() {
    let (features, components) = fixture();
    let payload = explicit_owner_payload();
    for retained in [1, 0] {
        let arena = DecodeArena::new();
        let policy = retained_policy(retained);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let query = || {
            crate::resolved_features::selections::compact_edge_owner_feature_at(
                &ctx,
                &payload,
                12,
                &components,
                &features,
                "consumer",
            )
        };
        if retained == 1 {
            assert_eq!(query().unwrap().as_deref(), Some("b"));
            ctx.finish_session().unwrap();
        } else {
            let CodecError::ResourceLimit(limit) = query().unwrap_err() else {
                panic!("retained explicit owner refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, "resolve SLDPRT compact edge owner");
            assert_eq!(limit.used, 0);
            assert_eq!(limit.additional, 1);
            let CodecError::ResourceLimit(repeated) = query().unwrap_err() else {
                panic!("original explicit owner refusal");
            };
            assert_eq!(repeated, limit);
            let CodecError::ResourceLimit(finished) = ctx.finish_session().unwrap_err() else {
                panic!("sticky session refusal");
            };
            assert_eq!(finished, limit);
        }
    }
}

#[test]
fn edge_producer_explicit_owner_reuses_existing_identity() {
    let (features, components) = fixture();
    let slots = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<String>());
    let arena = DecodeArena::new();
    let policy = retained_policy(slots + 2);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        crate::resolved_features::selections::compact_edge_producer_features_at(
            &ctx,
            &explicit_owner_payload(),
            12,
            &components,
            &features,
            "consumer",
        )
        .unwrap(),
        ["b", "a"]
    );
    ctx.finish_session().unwrap();
}
