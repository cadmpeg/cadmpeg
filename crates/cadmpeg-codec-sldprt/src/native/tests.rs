// SPDX-License-Identifier: Apache-2.0
//! Native catalogue load and store tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::history::resolved_features_payload_with_names;
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::history::sldprt_with_body_and_resolved_features;
use crate::test_support::history::sldprt_with_compact_relation_pair;
use crate::test_support::history::sldprt_with_nested_sketch_profile;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::pmi::pmi_semantic_payload;
use crate::SldprtCodec;

fn emitter_models() -> &'static [crate::native::SldprtNative] {
    static MODELS: std::sync::OnceLock<Vec<crate::native::SldprtNative>> =
        std::sync::OnceLock::new();
    MODELS.get_or_init(|| {
        let body = triangle_body();
        let mut pmi_source = sldprt_with_body(&body);
        pmi_source.extend(make_block(
            0x49,
            "Contents/PMISemanticDataDB",
            &pmi_semantic_payload(),
        ));
        let mut models = [
            sldprt_with_body_and_history(&body),
            sldprt_with_body_and_resolved_features(&body, &[0, 1]),
            sldprt_with_compact_relation_pair(&body),
            sldprt_with_nested_sketch_profile(&body),
            pmi_source,
        ]
        .into_iter()
        .map(|source| {
            let decoded = SldprtCodec
                .decode(&mut Cursor::new(source), &DecodeOptions::default())
                .unwrap();
            sldprt_native(decoded.ir())
        })
        .collect::<Vec<_>>();
        let lane = models
            .iter_mut()
            .flat_map(|model| &mut model.feature_input_lanes)
            .next()
            .unwrap();
        let parent = lane.id.clone();
        let feature_ref = "sldprt:native:feature#0".to_owned();
        let object_name_ref = "sldprt:native:name#0".to_owned();
        lane.body_selections
            .push(crate::records::FeatureInputBodySelection {
                id: "sldprt:native:body-selection#0".into(),
                parent: parent.clone(),
                ordinal: 0,
                offset: 0,
                object_name_ref: object_name_ref.clone(),
                feature_ref: feature_ref.clone(),
                local_body_ids: vec![1],
                body_state_ids: Vec::new(),
                mode: None,
            });
        lane.edge_selections
            .push(crate::records::FeatureInputEdgeSelection {
                id: "sldprt:native:edge-selection#0".into(),
                parent: parent.clone(),
                ordinal: 0,
                offset: 0,
                object_name_ref: object_name_ref.clone(),
                feature_ref: feature_ref.clone(),
                local_edge_ids: vec![1],
                components: Vec::new(),
                references: Vec::new(),
                producer_feature_refs: Vec::new(),
                terminal_feature_ref: None,
            });
        lane.surface_selections
            .push(crate::records::FeatureInputSurfaceSelection {
                id: "sldprt:native:surface-selection#0".into(),
                parent: parent.clone(),
                ordinal: 0,
                offset: 0,
                selector: 0,
                kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
                object_name_ref,
                feature_ref: feature_ref.clone(),
                producer_feature_refs: Vec::new(),
                terminal_feature_ref: None,
                components: Vec::new(),
            });
        lane.generated_surface_identities.push(
            crate::records::FeatureInputGeneratedSurfaceIdentity {
                id: "sldprt:native:generated-surface#0".into(),
                parent: parent.clone(),
                ordinal: 0,
                offset: 0,
                type_prefix: *b"FACE",
                feature_source_id: crate::brep::feature_source::FeatureSourceId::try_from(1_u32)
                    .unwrap(),
                local_identity: 1,
                components: Vec::new(),
            },
        );
        lane.relation_instances
            .push(crate::records::FeatureInputRelationInstance {
                id: "sldprt:native:relation-instance#0".into(),
                parent,
                ordinal: 0,
                offset: 0,
                family: crate::records::FeatureInputRelationFamily::CircleDiameter,
                class_ref: "sldprt:native:class#0".into(),
                feature_ref,
                scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                    vec!["sldprt:native:scalar#0".into()],
                    None,
                    None,
                )
                .unwrap(),
                operands: Vec::new(),
            });
        models
    })
}

#[test]
fn native_child_emitter_fixtures_cover_each_populated_arena() {
    let missing = super::SLDPRT_FAMILIES
        .iter()
        .filter(|row| {
            !matches!(row.arena, "feature_histories" | "feature_input_lanes")
                && emitter_models().iter().all(|model| (row.len)(model) == 0)
        })
        .map(|row| row.arena)
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "unpopulated emitter fixtures: {missing:?}"
    );
}

fn assert_child_emitter_refuses_before_clone(arena_name: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let row = super::SLDPRT_FAMILIES
        .iter()
        .find(|row| row.arena == arena_name)
        .unwrap();
    let model = emitter_models()
        .iter()
        .find(|model| (row.len)(model) > 0)
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let (refusal, allocations) = crate::test_support::allocation::count_allocations(|| {
        (row.emit)(&limited, model, row, &mut namespace)
    });
    assert_eq!(
        allocations, 0,
        "{arena_name} allocated before arena admission"
    );
    assert!(matches!(
        cadmpeg_core::CodecError::from(refusal.unwrap_err()),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain native arena name"
    ));
    assert!(namespace.arenas().is_empty());

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    (row.emit)(&service, model, row, &mut namespace).unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), (row.len)(model));
}

macro_rules! child_emitter_limit_test {
    ($name:ident, $arena:literal) => {
        #[test]
        fn $name() {
            assert_child_emitter_refuses_before_clone($arena);
        }
    };
}

child_emitter_limit_test!(
    native_pmi_dimension_emitter_refuses_before_child_clone,
    "pmi_dimensions"
);
child_emitter_limit_test!(
    native_configuration_emitter_refuses_before_child_clone,
    "configurations"
);
child_emitter_limit_test!(
    native_feature_emitter_refuses_before_child_clone,
    "features"
);
child_emitter_limit_test!(
    native_body_selection_emitter_refuses_before_child_clone,
    "feature_input_body_selections"
);
child_emitter_limit_test!(
    native_edge_selection_emitter_refuses_before_child_clone,
    "feature_input_edge_selections"
);
child_emitter_limit_test!(
    native_surface_selection_emitter_refuses_before_child_clone,
    "feature_input_surface_selections"
);
child_emitter_limit_test!(
    native_generated_surface_emitter_refuses_before_child_clone,
    "feature_input_generated_surface_identities"
);
child_emitter_limit_test!(
    native_class_emitter_refuses_before_child_clone,
    "feature_input_classes"
);
child_emitter_limit_test!(
    native_name_emitter_refuses_before_child_clone,
    "feature_input_names"
);
child_emitter_limit_test!(
    native_scalar_emitter_refuses_before_child_clone,
    "feature_input_scalars"
);
child_emitter_limit_test!(
    native_reference_emitter_refuses_before_child_clone,
    "feature_input_references"
);
child_emitter_limit_test!(
    native_relation_binding_emitter_refuses_before_child_clone,
    "feature_input_relation_bindings"
);
child_emitter_limit_test!(
    native_relation_instance_emitter_refuses_before_child_clone,
    "feature_input_relation_instances"
);
child_emitter_limit_test!(
    native_sketch_entity_emitter_refuses_before_child_clone,
    "sketch_input_entities"
);

#[test]
fn native_load_retained_limit_refuses_before_typed_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let namespace = decoded.ir().native.namespace("sldprt").unwrap();
    let first = &namespace.arenas()["feature_histories"][0];
    let needed = u64::try_from(serde_json::to_vec(first).unwrap().len()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = needed - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::native::SldprtNative::load_charged(&limited, namespace).unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "load typed native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        crate::native::SldprtNative::load_charged(&service, namespace).unwrap(),
        crate::native::SldprtNative::load(namespace).unwrap()
    );
}

#[test]
fn native_load_materialized_limit_refuses_before_expected_lane_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_compact_relation_pair(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let namespace = decoded.ir().native.namespace("sldprt").unwrap();
    let native = crate::native::SldprtNative::load(namespace).unwrap();
    assert!(!native.feature_input_lanes.is_empty());
    let needed = native
        .feature_input_lanes
        .iter()
        .map(|lane| u64::try_from(serde_json::to_vec(lane).unwrap().len()).unwrap())
        .sum::<u64>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = needed - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::records::FEATURE_INPUT_LANE_CLONE_COUNT.with(|count| count.set(0));
    let error = crate::native::SldprtNative::load_charged(&limited, namespace).unwrap_err();
    assert_eq!(
        crate::records::FEATURE_INPUT_LANE_CLONE_COUNT.with(std::cell::Cell::get),
        0
    );
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "validate SLDPRT expected lane copies"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        crate::native::SldprtNative::load_charged(&service, namespace).unwrap(),
        native
    );
}

#[test]
fn native_store_materialized_limit_refuses_before_feature_validation_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&limited, &mut namespace).unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "validate SLDPRT store features"
    ));
    assert!(namespace.arenas().is_empty());

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    native.store(&service, &mut namespace).unwrap();
    assert!(!namespace.arenas()["features"].is_empty());
}

fn native_delete_body_fixture() -> crate::native::SldprtNative {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Body-Delete/Keep 1" Type="Body-Delete/Keep " id="41"/></Keywords>"#,
    ));
    let mut payload =
        resolved_feature_classes_with_ids(&[("moDeleteBody_c", "Body-Delete/Keep 1", 41)]);
    payload.extend([0xff, 0xff, 0x01, 0x00]);
    payload.extend(18u16.to_le_bytes());
    payload.extend(b"moDeleteBodyData_c");
    payload.extend([0x08, 0x00]);
    let mut state = [0u8; 83];
    state[0..2].copy_from_slice(&0x89a4u16.to_le_bytes());
    state[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    state[11..15].copy_from_slice(&287u32.to_le_bytes());
    state[15..19].copy_from_slice(&287u32.to_le_bytes());
    state[47..63].fill(0xff);
    payload.extend(state);
    payload.extend([0x30, 0x80]);
    payload.extend(1u32.to_le_bytes());
    payload.extend([0; 4]);
    payload.extend(11000u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(287u32.to_le_bytes());
    payload.extend(115u32.to_le_bytes());
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend([0; 12]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(decoded.ir());
    native
}

#[test]
fn native_body_validation_collection_limit_refuses_before_candidates() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let native = native_delete_body_fixture();
    let lane = native
        .feature_input_lanes
        .iter()
        .find(|lane| !lane.body_selections.is_empty())
        .unwrap();
    let record = &lane.body_selections[0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items =
        u64::try_from(super::selection_payload_span(lane, record.offset)).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        super::body_selection_disagrees_with_payload(super::admission::NativeAdmission::Decode(&limited), lane, record).unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "validate SLDPRT body selection candidates"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(!super::body_selection_disagrees_with_payload(super::admission::NativeAdmission::Decode(&service), lane, record).unwrap());
}

#[test]
fn native_body_state_validation_limit_refuses_before_state_ids() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let native = native_delete_body_fixture();
    let lane = native
        .feature_input_lanes
        .iter()
        .find(|lane| !lane.body_selections.is_empty())
        .unwrap();
    let record = &lane.body_selections[0];
    let name = lane
        .names
        .iter()
        .find(|name| name.id == record.object_name_ref)
        .unwrap();
    let source_units = record.offset - name.offset;
    assert!(source_units > 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = source_units - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        super::body_state_ids_disagree_with_payload(super::admission::NativeAdmission::Decode(&limited), lane, record).unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "validate SLDPRT body state candidates"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(!super::body_state_ids_disagree_with_payload(super::admission::NativeAdmission::Decode(&service), lane, record).unwrap());
}

#[test]
fn native_edge_validation_collection_limit_refuses_before_candidates() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut lane = emitter_models()
        .iter()
        .flat_map(|model| &model.feature_input_lanes)
        .find(|lane| !lane.edge_selections.is_empty())
        .unwrap()
        .clone();
    let marker = lane.native_payload.len() + 12;
    lane.native_payload.extend(1u32.to_le_bytes());
    lane.native_payload
        .extend([0x00, 0x02, 0x00, 0x00, 0, 0, 0, 0]);
    lane.native_payload.extend([
        0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2,
        0x54,
    ]);
    lane.native_payload.extend([0, 0]);
    lane.native_payload.extend(0x818bu32.to_le_bytes());
    lane.native_payload.extend([
        0x00, 0x81, 0x03, 0x01, 0x2c, 0, 0, 0, 0x63, 0x18, 0x58, 0x69,
    ]);
    lane.native_payload.extend(7u32.to_le_bytes());
    let mut record = lane.edge_selections[0].clone();
    record.offset = u64::try_from(marker).unwrap();
    record.local_edge_ids = vec![7];
    record.components = crate::resolved_features::selections::compact_edge_component_path_at(
        &lane.native_payload,
        marker,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items =
        u64::try_from(super::selection_payload_span(&lane, record.offset)).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::edge_selection_disagrees_with_payload(super::admission::NativeAdmission::Decode(&limited), &lane, &record, &[])
        .unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "validate SLDPRT edge selection candidates"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(
        !super::edge_selection_disagrees_with_payload(super::admission::NativeAdmission::Decode(&service), &lane, &record, &[]).unwrap()
    );
}

#[test]
fn native_surface_validation_collection_limit_refuses_before_candidates() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut lane = emitter_models()
        .iter()
        .flat_map(|model| &model.feature_input_lanes)
        .find(|lane| !lane.surface_selections.is_empty())
        .unwrap()
        .clone();
    let marker = lane.native_payload.len() + 12;
    lane.native_payload.extend(6u32.to_le_bytes());
    lane.native_payload.extend([0x04, 0x02, 0, 0]);
    lane.native_payload.extend(0x1234u32.to_le_bytes());
    lane.native_payload.extend([
        0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2,
        0x54,
    ]);
    lane.native_payload.extend([0, 0]);
    lane.native_payload.extend(0x8c20u32.to_le_bytes());
    let signature = [0x34, 0x80, 0x37, 0, 0x89, 0, 0, 0, 0xe2, 0x56, 0xdf, 0x5e];
    lane.native_payload.extend(signature);
    lane.native_payload.extend(12u32.to_le_bytes());
    lane.native_payload.extend([0; 24]);
    let mut record = lane.surface_selections[0].clone();
    record.offset = u64::try_from(marker).unwrap();
    record.components = vec![crate::records::FeatureInputComponentPathEntry {
        instance: Some(0x8c20),
        type_signature: signature,
        local_id: Some(12),
    }];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items =
        u64::try_from(super::selection_payload_span(&lane, record.offset)).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        super::surface_selection_disagrees_with_payload(super::admission::NativeAdmission::Decode(&limited), &lane, &record, &[])
            .unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "validate SLDPRT surface selection candidates"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(
        !super::surface_selection_disagrees_with_payload(super::admission::NativeAdmission::Decode(&service), &lane, &record, &[])
            .unwrap()
    );
}

#[test]
fn native_derived_lane_collection_limit_refuses_before_reconstruction() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_compact_relation_pair(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let lane_count = u64::try_from(native.feature_input_lanes.len()).unwrap();
    let payload_bytes = native
        .feature_input_lanes
        .iter()
        .map(|lane| u64::try_from(lane.native_payload.len()).unwrap())
        .sum::<u64>();
    assert!(payload_bytes > 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = lane_count * 2 + payload_bytes - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::lanes::admit(&native, super::admission::NativeAdmission::Decode(&limited)).unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "validate SLDPRT derived lanes"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    super::lanes::admit(&native, super::admission::NativeAdmission::Decode(&service)).unwrap();
}

#[test]
fn native_generated_surface_validation_limit_refuses_before_identity_rows() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let class_name = "moWzdHoleSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let mut payload = [0xff, 0xff, 0x01, 0x00].to_vec();
    payload.extend(u16::try_from(class_name.len()).unwrap().to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(0x52e4_6185u32.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    let mut lane = crate::records::FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![crate::records::FeatureInputClass {
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
    lane.generated_surface_identities =
        crate::resolved_features::selections::generated_surface_identities(&lane);
    assert_eq!(lane.generated_surface_identities.len(), 1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::try_from(lane.native_payload.len()).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::generated_surface_identities_disagree_with_payload(super::admission::NativeAdmission::Decode(&limited), &lane)
        .unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "validate SLDPRT generated surface identities"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(
        !super::generated_surface_identities_disagree_with_payload(super::admission::NativeAdmission::Decode(&service), &lane).unwrap()
    );
}

#[test]
fn native_scalar_operand_validation_limit_refuses_before_resolution() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_compact_relation_pair(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let lane = native
        .feature_input_lanes
        .iter()
        .find(|lane| {
            lane.scalars
                .iter()
                .any(|scalar| !scalar.operands.is_empty())
        })
        .unwrap();
    let scalar = lane
        .scalars
        .iter()
        .find(|scalar| !scalar.operands.is_empty())
        .unwrap();
    let source_items = lane.sketch_entities.len() + scalar.operands.len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::try_from(source_items).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::resolved_scalar_operand_markers(&limited, lane, scalar).unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "validate SLDPRT scalar operand candidates"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::resolved_scalar_operand_markers(&service, lane, scalar)
            .unwrap()
            .len(),
        scalar.operands.len()
    );
}

#[test]
fn native_history_class_validation_limit_refuses_before_lookup_maps() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let source_items = native
        .feature_input_lanes
        .iter()
        .map(|lane| lane.names.len() + lane.classes.len())
        .sum::<usize>();
    assert!(source_items > 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::try_from(source_items).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut histories = native.feature_histories.clone();
    let error =
        super::bind_history_classes_charged(&limited, &mut histories, &native.feature_input_lanes)
            .unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "validate SLDPRT history class candidates"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    super::bind_history_classes_charged(&service, &mut histories, &native.feature_input_lanes)
        .unwrap();
}

#[test]
fn native_history_borrowed_view_matches_cleared_record_json_bytes() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let history = &native.feature_histories[0];
    let mut cleared = history.clone();
    cleared.configurations.clear();
    cleared.features.clear();
    assert_eq!(
        serde_json::to_vec(&super::HistoryArenaView(history)).unwrap(),
        serde_json::to_vec(&cleared).unwrap()
    );
}

#[test]
fn native_history_retained_limit_refuses_before_history_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let row = super::SLDPRT_FAMILIES
        .iter()
        .find(|row| row.arena == "feature_histories")
        .unwrap();
    let needed = row.arena.len()
        + serde_json::to_vec(&super::HistoryArenaView(&native.feature_histories[0]))
            .unwrap()
            .len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    crate::records::FEATURE_HISTORY_CLONE_COUNT.with(|count| count.set(0));
    let error = (row.emit)(&limited, &native, row, &mut namespace).unwrap_err();
    crate::records::FEATURE_HISTORY_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    (row.emit)(&service, &native, row, &mut namespace).unwrap();
    assert_eq!(namespace.arenas()[row.arena].len(), 1);
}

#[test]
fn native_lane_borrowed_view_matches_cleared_record_json_bytes() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    let mut cleared = lane.clone();
    cleared.classes.clear();
    cleared.names.clear();
    cleared.scalars.clear();
    cleared.relation_bindings.clear();
    cleared.relation_instances.clear();
    cleared.body_selections.clear();
    cleared.edge_selections.clear();
    cleared.surface_selections.clear();
    cleared.generated_surface_identities.clear();
    cleared.references.clear();
    cleared.sketch_entities.clear();
    assert_eq!(
        serde_json::to_vec(&super::LaneArenaView(lane)).unwrap(),
        serde_json::to_vec(&cleared).unwrap()
    );
}

#[test]
fn native_lane_retained_limit_refuses_before_lane_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = sldprt_native(decoded.ir());
    let row = super::SLDPRT_FAMILIES
        .iter()
        .find(|row| row.arena == "feature_input_lanes")
        .unwrap();
    let needed = row.arena.len()
        + serde_json::to_vec(&super::LaneArenaView(&native.feature_input_lanes[0]))
            .unwrap()
            .len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    crate::records::FEATURE_INPUT_LANE_CLONE_COUNT.with(|count| count.set(0));
    let error = (row.emit)(&limited, &native, row, &mut namespace).unwrap_err();
    crate::records::FEATURE_INPUT_LANE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    (row.emit)(&service, &native, row, &mut namespace).unwrap();
    assert_eq!(namespace.arenas()[row.arena].len(), 1);
}

#[test]
fn native_arenas_have_pinned_shape_and_typed_round_trip() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = decoded.ir().native.namespace("sldprt").unwrap();
    let typed = crate::native::SldprtNative::load(original).unwrap();
    let mut round_trip = cadmpeg_ir::NativeNamespace::default();
    typed
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut round_trip,
        )
        .unwrap();
    assert_eq!(
        typed,
        crate::native::SldprtNative::load(&round_trip).unwrap()
    );
    assert_eq!(
        round_trip
            .arenas()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        crate::native::SLDPRT_ARENA_NAMES
    );
    for records in round_trip.arenas().values() {
        for record in records {
            let json = serde_json::to_value(record).unwrap();
            assert_eq!(json["id"], record.id());
            assert!(json.as_object().unwrap().len() > 1);
        }
    }
}

#[test]
fn native_store_rejects_mismatched_nested_owners_atomically() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let mut native = sldprt_native(decoded.ir());
    native.feature_histories[0].features[0].parent = "missing-history".into();
    let before = decoded.ir().native.namespace("sldprt").unwrap().clone();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            decoded.ir_mut().native.namespace_mut("sldprt"),
        )
        .unwrap_err();
    assert!(error.to_string().contains("invalid owner"));
    assert_eq!(decoded.ir().native.namespace("sldprt").unwrap(), &before);
}

#[test]
fn native_store_rejects_missing_sketch_marker_feature_owner() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    native.feature_input_lanes[0]
        .sketch_entities
        .last_mut()
        .expect("sketch marker")
        .feature_ref = Some("sldprt:history:feature#missing".into());

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("inconsistent lane or feature ownership"));
}

#[test]
fn native_store_rejects_edited_history_feature_class() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Round" Type="Fillet" id="41"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    native.feature_histories[0].features[0].input_class = Some("moRefPlane_c".into());

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("feature classes do not match the feature-input index"));
}

#[test]
fn native_store_rejects_missing_sketch_marker_local_link() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    let entity = &mut native.feature_input_lanes[0].sketch_entities[0];
    entity.links = crate::records::SketchInputLinks::new(
        0,
        vec![crate::records::SketchInputLink {
            local_id: 7,
            entity_ref: "sldprt:feature-input:sketch-entity#missing".into(),
        }],
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap_err();
    assert!(error.to_string().contains("missing local-link target"));
}

#[test]
fn native_store_preserves_midpoint_with_two_point_markers() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    let entities = &mut native.feature_input_lanes[0].sketch_entities;
    let owner = entities[0].feature_ref.clone();
    let point_id = entities[1].id().to_string();
    let second_point_id = entities[2].id().to_string();
    entities[1].feature_ref = owner.clone();
    entities[1] = entities[1].with_test_identity(entities[1].object_index(), Some(7));
    entities[1].reclassify(crate::records::SketchInputKind::Point);
    entities[2].feature_ref = owner;
    entities[2] = entities[2].with_test_identity(entities[2].object_index(), Some(8));
    entities[2].reclassify(crate::records::SketchInputKind::ConstrainedPoint);
    entities[0].reclassify(crate::records::SketchInputKind::Relation(
        crate::records::SketchRelationKind::Midpoint,
    ));
    entities[0].links = crate::records::SketchInputLinks::new(
        0,
        vec![
            crate::records::SketchInputLink {
                local_id: 7,
                entity_ref: point_id.clone(),
            },
            crate::records::SketchInputLink {
                local_id: 8,
                entity_ref: second_point_id.clone(),
            },
        ],
    );
    let lane = &mut native.feature_input_lanes[0];
    for (index, local_id) in [(1, 7u32), (2, 8u32)] {
        let offset = lane.sketch_entities[index].offset() as usize + 88;
        lane.native_payload[offset..offset + 4].copy_from_slice(&local_id.to_le_bytes());
    }
    for entity in &mut lane.sketch_entities {
        *entity = entity.with_test_identity(
            crate::resolved_features::markers::marker_object_index(
                &lane.native_payload,
                entity.offset() as usize,
            ),
            entity.local_id(),
        );
    }
    let expected = crate::native::lanes::expected_lanes(&native).remove(0).1;
    let lane = &mut native.feature_input_lanes[0];
    lane.scalars = expected.scalars;
    lane.relation_bindings = expected.relation_bindings;
    lane.relation_instances = expected.relation_instances;
    lane.references = expected.references;

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
    let stored = crate::native::SldprtNative::load(&namespace).unwrap();
    assert_eq!(
        stored.feature_input_lanes[0].sketch_entities[0]
            .links()
            .len(),
        2
    );
}

#[test]
fn native_store_rejects_relation_scalar_owner_disagreement() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    assert!(native.feature_input_lanes[0].relation_bindings[0]
        .feature_ref
        .is_some());
    native.feature_input_lanes[0].relation_bindings[0].feature_ref = None;

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("disagrees with its scalar owner"));
}

#[test]
fn native_store_rejects_nonlocal_relation_scalar_groups() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    let duplicate = native.feature_input_lanes[0].relation_instances[0].scalar_refs()[0].clone();
    native.feature_input_lanes[0].relation_instances[0]
        .scalars
        .push(duplicate);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("relation instance")
            && error.to_string().contains("inconsistent ownership")
    );
}

#[test]
fn native_load_rejects_nonadjacent_duplicate_relation_scalars() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut namespace = decoded
        .ir()
        .native
        .namespace("sldprt")
        .expect("SLDPRT namespace")
        .clone();
    let mut relations: Vec<crate::records::FeatureInputRelationInstance> = namespace
        .arena_as("feature_input_relation_instances")
        .unwrap();
    let relation = relations.first_mut().expect("relation instance");
    assert_eq!(relation.scalar_refs().len(), 2);
    relation.scalars.push(relation.scalar_refs()[0].clone());
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "feature_input_relation_instances",
            &relations,
        )
        .unwrap();

    let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
    assert!(error.to_string().contains("relation instance"));
}

#[test]
fn native_store_rejects_relation_instance_operand_disagreement() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    native.feature_input_lanes[0].relation_instances[0].operands[0].entity_index += 1;

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("relation instance")
            && error.to_string().contains("inconsistent ownership")
    );
}

#[test]
fn native_store_rejects_inconsistent_scalar_marker_target() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0, 0, 2], &["Sketch1", "D1"]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    let wrong_target = native.feature_input_lanes[0].sketch_entities[0]
        .id()
        .to_string();
    native.feature_input_lanes[0].scalars[0].operands[1].entity_ref = Some(wrong_target.clone());
    native.feature_input_lanes[0].relation_instances[0].operands[1].entity_ref = Some(wrong_target);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap_err();
    assert!(error.to_string().contains("inconsistent sketch marker"));
}

#[test]
fn native_store_accepts_duplicate_local_ids_for_scalar_ordinals() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0, 0, 2], &["Sketch1", "D1"]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(decoded.ir());
    let lane = &mut native.feature_input_lanes[0];
    assert_eq!(lane.scalars[0].operands[0].entity_index, 0);
    assert!(lane.scalars[0].operands[0].entity_ref.is_some());
    let local_id = lane.sketch_entities[0]
        .local_id()
        .expect("first marker local id");
    lane.sketch_entities[1] = lane.sketch_entities[1]
        .with_test_identity(lane.sketch_entities[1].object_index(), Some(local_id));
    let local_id_offset = crate::resolved_features::markers::marker_local_id_offset(
        &lane.native_payload,
        usize::try_from(lane.sketch_entities[1].offset()).expect("marker offset"),
    )
    .expect("local id offset");
    lane.native_payload[local_id_offset..local_id_offset + 4]
        .copy_from_slice(&local_id.to_le_bytes());
    let next = &mut lane.sketch_entities[2];
    *next = next.with_test_identity(Some(local_id), next.local_id());

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
}

#[test]
fn native_load_rejects_fabricated_payload_lane_rows_from_json() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let original = decoded.ir().native.namespace("sldprt").unwrap();
    for (arena, message) in [
        ("feature_input_classes", "class index"),
        ("feature_input_names", "name structure"),
        ("feature_input_scalars", "scalar index"),
        ("feature_input_relation_bindings", "relation bindings"),
        ("feature_input_relation_instances", "relation instances"),
        ("feature_input_references", "reference index"),
    ] {
        let mut wire = serde_json::to_value(original).unwrap();
        let record = wire[arena].as_array_mut().unwrap().first_mut().unwrap();
        let ordinal = record["ordinal"].as_u64().unwrap();
        record["ordinal"] = serde_json::json!(ordinal + 100);
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(wire).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(error.to_string().contains(message), "{arena}: {error}");
    }
    let mut wire = serde_json::to_value(original).unwrap();
    assert!(!wire["sketch_input_entities"].as_array().unwrap().is_empty());
    wire["sketch_input_entities"] = serde_json::json!([]);
    let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(wire).unwrap();
    let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
    assert!(error.to_string().contains("omits marker"), "{error}");
}

#[test]
fn native_load_rejects_edited_object_name_identity_from_json() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let original = serde_json::to_value(decoded.ir().native.namespace("sldprt").unwrap()).unwrap();
    let names = original["feature_input_names"]
        .as_array()
        .expect("decoded lane has a name arena");
    assert!(
        !names.is_empty(),
        "fixture must admit at least one object name"
    );

    let mut object_id_edit = original;
    let current_object_id = object_id_edit["feature_input_names"][0]["object_id"].as_u64();
    let forged_object_id = if current_object_id == Some(1) { 2 } else { 1 };
    object_id_edit["feature_input_names"][0]["object_id"] = serde_json::json!(forged_object_id);
    let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(object_id_edit).unwrap();
    let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
    assert!(
        error.to_string().contains("name structure does not match"),
        "edited object name identifier was admitted: {error}"
    );
}

#[test]
fn native_load_admits_the_unedited_namespace_and_refuses_every_object_name_edit() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let original = serde_json::to_value(decoded.ir().native.namespace("sldprt").unwrap()).unwrap();

    let control: cadmpeg_ir::NativeNamespace = serde_json::from_value(original.clone()).unwrap();
    crate::native::SldprtNative::load(&control).expect("the unedited namespace loads");

    let stated = original["feature_input_names"][0]["value"]
        .as_str()
        .expect("fixture admits an object name")
        .to_string();
    assert!(!original["feature_input_scalars"]
        .as_array()
        .expect("fixture admits a scalar arena")
        .is_empty());

    let mut scalar_edit = original.clone();
    let offset = scalar_edit["feature_input_scalars"][0]["offset"]
        .as_u64()
        .expect("a scalar states its payload offset");
    scalar_edit["feature_input_scalars"][0]["offset"] = serde_json::json!(offset + 7);
    let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(scalar_edit).unwrap();
    let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
    assert!(
        error.to_string().contains("scalar index does not match"),
        "edited scalar offset was admitted: {error}"
    );

    let mut object_id_edit = original.clone();
    let current = object_id_edit["feature_input_names"][0]["object_id"].as_u64();
    object_id_edit["feature_input_names"][0]["object_id"] =
        serde_json::json!(if current == Some(1) { 2 } else { 1 });
    let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(object_id_edit).unwrap();
    let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
    assert!(
        error.to_string().contains("name structure does not match"),
        "edited object identifier was admitted: {error}"
    );

    let same_length = "z".repeat(stated.encode_utf16().count());
    assert_ne!(same_length, stated);
    for forged in [same_length, format!("{stated}-longer")] {
        let mut value_edit = original.clone();
        value_edit["feature_input_names"][0]["value"] = serde_json::json!(forged);
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(value_edit).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("name value does not match its native payload"),
            "edited object name {forged:?} was admitted or refused elsewhere: {error}"
        );
    }
}

#[test]
fn native_load_rejects_invalid_sketch_marker_positions_from_json() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = serde_json::to_value(decoded.ir().native.namespace("sldprt").unwrap()).unwrap();
    for (field, value, message) in [
        (
            "ordinal",
            serde_json::json!(3),
            "SolidWorks feature-input lane expects entity ordinal",
        ),
        (
            "offset",
            serde_json::json!(u64::MAX),
            "sketch entity offset is not a marker in native_payload",
        ),
        (
            "object_index",
            serde_json::json!(77),
            "SolidWorks feature-input object index does not match its native payload",
        ),
        (
            "local_id",
            serde_json::json!(77),
            "SolidWorks feature-input local object id does not match its native payload",
        ),
    ] {
        let mut wire = original.clone();
        wire["sketch_input_entities"][0][field] = value;
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(wire).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(error.to_string().contains(message), "{field}: {error}");
    }
    for (field, message) in [
        (
            "ordinal",
            "SolidWorks feature-input lane expects entity ordinal",
        ),
        (
            "offset",
            "SolidWorks feature-input object index does not match its native payload",
        ),
    ] {
        let mut wire = original.clone();
        wire["sketch_input_entities"][1][field] = wire["sketch_input_entities"][0][field].clone();
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(wire).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(error.to_string().contains(message), "{field}: {error}");
    }
}

#[test]
fn native_load_rejects_duplicate_history_ordinals_from_json() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = serde_json::to_value(decoded.ir().native.namespace("sldprt").unwrap()).unwrap();
    for (arena, message) in [
        ("features", "repeats feature ordinal"),
        ("configurations", "repeats configuration ordinal"),
    ] {
        let mut wire = original.clone();
        let records = wire[arena].as_array_mut().unwrap();
        let mut duplicate = records[0].clone();
        duplicate["id"] =
            serde_json::json!(format!("{}-duplicate", records[0]["id"].as_str().unwrap()));
        records.push(duplicate);
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(wire).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(error.to_string().contains(message), "{arena}: {error}");
    }
}

/// The document used for the object-name edit tests: one feature-input lane
/// whose object names and scalar arena both derive from the same payload.
fn document_with_named_scalars() -> Vec<u8> {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source
}

#[test]
fn native_load_refuses_a_forged_object_name_length_byte_after_a_store() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(document_with_named_scalars()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let namespace = decoded.ir().native.namespace("sldprt").unwrap();
    let mut typed = crate::native::SldprtNative::load(namespace).unwrap();

    // Byte five of an object-name record is its UTF-16 length; the name's own
    // `value` and every later record's offset derive from it.
    let length_byte = usize::try_from(typed.feature_input_lanes[0].names[0].offset).unwrap() + 5;
    let stated = typed.feature_input_lanes[0].native_payload[length_byte];
    typed.feature_input_lanes[0].native_payload[length_byte] = stated + 1;

    let mut forged = cadmpeg_ir::NativeNamespace::default();
    typed
        .store(&cadmpeg_test_support::service_decode_context(), &mut forged)
        .unwrap();
    let error = crate::native::SldprtNative::load(&forged).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("name structure does not match its native payload"),
        "a forged name length byte was admitted: {error}"
    );
}

#[test]
fn native_load_refuses_every_object_name_value_edit_and_leaves_the_scalar_arena_alone() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(document_with_named_scalars()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = serde_json::to_value(decoded.ir().native.namespace("sldprt").unwrap()).unwrap();
    let stated = original["feature_input_names"][0]["value"]
        .as_str()
        .expect("fixture admits an object name")
        .to_string();
    let lane = original["feature_input_names"][0]["parent"]
        .as_str()
        .expect("a name states its lane")
        .to_string();
    let ordinal = original["feature_input_names"][0]["ordinal"]
        .as_u64()
        .expect("a name states its position in the lane");
    let scalars = original["feature_input_scalars"].clone();
    assert!(!scalars
        .as_array()
        .expect("fixture admits a scalar arena")
        .is_empty());

    let same_length = "z".repeat(stated.encode_utf16().count());
    assert_ne!(same_length, stated);
    let shorter = "z".to_string();
    assert!(shorter.encode_utf16().count() < stated.encode_utf16().count());
    for forged in [same_length, format!("{stated}-longer"), shorter] {
        let mut edit = original.clone();
        edit["feature_input_names"][0]["value"] = serde_json::json!(forged);
        assert_eq!(
            edit["feature_input_scalars"], scalars,
            "the value edit moved the scalar arena"
        );
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(edit).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(
            error.to_string().contains(&format!(
                "name value does not match its native payload: lane {lane} name {ordinal} states {forged:?}, its payload states {stated:?}"
            )),
            "edited object name {forged:?} was admitted or refused elsewhere: {error}"
        );
    }
}

#[test]
fn native_load_refuses_an_object_name_offset_the_payload_does_not_state() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(document_with_named_scalars()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = serde_json::to_value(decoded.ir().native.namespace("sldprt").unwrap()).unwrap();
    let payload_length = original["feature_input_lanes"][0]["native_payload"]
        .as_str()
        .map(str::len)
        .or_else(|| {
            original["feature_input_lanes"][0]["native_payload"]
                .as_array()
                .map(Vec::len)
        })
        .expect("a lane states its payload");
    assert!(payload_length > 0);

    for forged in [u64::MAX, payload_length as u64 + 1] {
        let mut edit = original.clone();
        edit["feature_input_names"][0]["offset"] = serde_json::json!(forged);
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(edit).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("name structure does not match its native payload"),
            "an object name offset outside the payload was admitted: {error}"
        );
    }
}
