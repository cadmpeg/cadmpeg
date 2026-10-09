//! Generated surface identity path admission.

use super::super::generated_surface_identities;
use crate::records::{FeatureInputClass, FeatureInputLane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn generated_surface_identity_path_keeps_absent_source_component() {
    let class_name = "moWzdHoleSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let mut payload = [0xff, 0xff, 0x01, 0x00].to_vec();
    payload.extend(u16::try_from(class_name.len()).unwrap().to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    let offset = payload.len();
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(1u32.to_le_bytes());
    payload.extend(0x85b5u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    payload.extend(3u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.into(),
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: class_name.into(),
        }],
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
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let identities = generated_surface_identities(&ctx, &lane).unwrap();
    assert_eq!(identities.len(), 1);
    let identity = &identities[0];
    assert_eq!(identity.offset, u64::try_from(offset).unwrap());
    assert_eq!(identity.feature_source_id.value(), 89);
    assert_eq!(identity.local_identity, 0x85b5);
    assert_eq!(identity.components.len(), 2);
    assert_eq!(identity.components[0].instance, None);
    assert_eq!(identity.components[0].local_id, None);
    assert_eq!(identity.components[1].instance, Some(0x85b5));
    assert_eq!(
        &identity.components[1].type_signature[4..8],
        &u32::MAX.to_le_bytes()
    );
    assert_eq!(identity.components[1].local_id, Some(3));
}

#[test]
fn generated_surface_identity_duplicates_use_the_component_index() {
    let class_name = "moWzdHoleSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let mut payload = [0xff, 0xff, 0x01, 0x00].to_vec();
    payload.extend(u16::try_from(class_name.len()).unwrap().to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    let first = payload.len();
    for _ in 0..2 {
        for identity in 1u32..=2048 {
            payload.extend(prefix);
            payload.extend(89u32.to_le_bytes());
            payload.extend(identity.to_le_bytes());
            payload.extend(3u32.to_le_bytes());
        }
    }
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: class_name.into(),
        }],
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 6_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let identities = generated_surface_identities(&ctx, &lane).unwrap();
    assert_eq!(identities.len(), 2048);
    for (index, identity) in identities.iter().enumerate() {
        assert_eq!(identity.offset, u64::try_from(first + index * 16).unwrap());
        assert_eq!(identity.feature_source_id.value(), 89);
        assert_eq!(identity.local_identity, 3);
        assert_eq!(identity.components.len(), 1);
        assert_eq!(identity.components[0].local_id, Some(3));
        assert_eq!(
            &identity.components[0].type_signature[8..12],
            &u32::try_from(index + 1).unwrap().to_le_bytes()
        );
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn split_surface_queries_reuse_one_scoped_identity_parse() {
    use super::super::operation_surface_selection_candidates;
    use crate::classification::FeatureClass;

    let class_name = "moPLineSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let mut payload = [0xff, 0xff, 0x01, 0x00].to_vec();
    payload.extend(u16::try_from(class_name.len()).unwrap().to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(711u32.to_le_bytes());
    payload.extend(1u32.to_le_bytes());
    payload.extend(0x80a7u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(314u32.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    payload.extend(3u32.to_le_bytes());
    payload.resize(4096, 0x22);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![
            FeatureInputClass {
                id: "surface-class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                name: class_name.into(),
            },
            FeatureInputClass {
                id: "projection-class".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 4096,
                name: "moPLineProjIdRep_c".into(),
            },
        ],
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 400_000;
    policy.limits.max_materialized_bytes = 1_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = None;
    for _ in 0..32 {
        let selections = operation_surface_selection_candidates(
            &ctx,
            FeatureClass::SplitFace,
            &lane,
            0,
            lane.native_payload.len(),
            Some(711),
            &mut cache,
        )
        .unwrap();
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].1.len(), 2);
        assert_eq!(selections[0].1[1].local_id, Some(3));
    }
    drop(cache);
    drop(
        ctx.reserve_scoped(1_000_000, "released surface identity cache")
            .unwrap(),
    );
    assert!(ctx.resource_refusal().is_none());
}
