// SPDX-License-Identifier: Apache-2.0
//! Native catalogue load, store, and version-migration tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

use super::{SldprtNative, SLDPRT_NATIVE_VERSION};

#[test]
fn version_twelve_adds_generated_surface_identity_arena() {
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    SldprtNative::default()
        .store(&mut namespace)
        .expect("required invariant");
    namespace.version = 12;
    namespace
        .arenas
        .remove("feature_input_generated_surface_identities");

    let migrated = SldprtNative::load(&namespace).expect("required invariant");
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).expect("required invariant");

    assert_eq!(current.version, SLDPRT_NATIVE_VERSION);
    assert!(current
        .arenas
        .contains_key("feature_input_generated_surface_identities"));
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
    typed.store(&mut round_trip).unwrap();
    assert_eq!(
        typed,
        crate::native::SldprtNative::load(&round_trip).unwrap()
    );
    assert_eq!(round_trip.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert_eq!(
        round_trip
            .arenas
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        crate::native::SLDPRT_ARENA_NAMES
    );
    for records in round_trip.arenas.values() {
        for record in records {
            let json = serde_json::to_value(record).unwrap();
            assert_eq!(json["id"], record.id());
            assert!(json.as_object().unwrap().len() > 1);
        }
    }
}

#[test]
fn native_version_one_migrates_the_body_selection_arena() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let mut legacy = decoded.ir().native.namespace("sldprt").unwrap().clone();
    legacy.version = 1;
    legacy.arenas.remove("feature_input_body_selections");

    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert_eq!(migrated.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(migrated
        .feature_input_lanes
        .iter()
        .all(|lane| lane.body_selections.is_empty()));
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).unwrap();
    assert_eq!(current.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(current.arenas.contains_key("feature_input_body_selections"));

    *decoded.ir_mut().native.namespace_mut("sldprt") = legacy;
    assert!(crate::validate_native(decoded.ir()).is_empty());
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn native_version_two_migrates_the_edge_selection_arena() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut legacy = decoded.ir().native.namespace("sldprt").unwrap().clone();
    legacy.version = 2;
    legacy.arenas.remove("feature_input_edge_selections");

    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert_eq!(migrated.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(migrated
        .feature_input_lanes
        .iter()
        .all(|lane| lane.edge_selections.is_empty()));
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).unwrap();
    assert_eq!(current.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(current.arenas.contains_key("feature_input_edge_selections"));
}

#[test]
fn native_version_three_migrates_the_surface_selection_arena() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut legacy = decoded.ir().native.namespace("sldprt").unwrap().clone();
    legacy.version = 3;
    legacy.arenas.remove("feature_input_surface_selections");
    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert!(migrated
        .feature_input_lanes
        .iter()
        .all(|lane| lane.surface_selections.is_empty()));
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).unwrap();
    assert_eq!(current.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(current
        .arenas
        .contains_key("feature_input_surface_selections"));
}

#[test]
fn native_version_four_migrates_sketch_marker_object_indices() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut legacy = decoded.ir().native.namespace("sldprt").unwrap().clone();
    legacy.version = 4;
    for record in legacy.arenas.get_mut("sketch_input_entities").unwrap() {
        let mut fields = record.fields();
        fields.remove("object_index");
        *record = cadmpeg_ir::NativeRecord::new(record.id().to_string(), fields);
    }
    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert!(migrated.feature_input_lanes.iter().all(|lane| {
        lane.sketch_entities.iter().all(|entity| {
            usize::try_from(entity.offset).ok().and_then(|offset| {
                crate::resolved_features::markers::marker_object_index(&lane.native_payload, offset)
            }) == entity.object_index
        })
    }));
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).unwrap();
    assert_eq!(current.version, crate::native::SLDPRT_NATIVE_VERSION);

    let mut sentinel = decoded.ir().native.namespace("sldprt").unwrap().clone();
    sentinel.version = 6;
    let sentinel_entity = &mut sentinel.arenas.get_mut("sketch_input_entities").unwrap()[0];
    let mut sentinel_fields = sentinel_entity.fields();
    sentinel_fields.insert("object_index".into(), serde_json::json!(u32::MAX));
    *sentinel_entity =
        cadmpeg_ir::NativeRecord::new(sentinel_entity.id().to_string(), sentinel_fields);
    let migrated = crate::native::SldprtNative::load(&sentinel).unwrap();
    assert_eq!(
        migrated.feature_input_lanes[0].sketch_entities[0].object_index,
        None
    );
}

#[test]
fn native_future_version_remains_rejected() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut future = decoded.ir().native.namespace("sldprt").unwrap().clone();
    future.version = crate::native::SLDPRT_NATIVE_VERSION + 1;
    let error = crate::native::SldprtNative::load(&future).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::NativeConvertError::UnsupportedVersion(
            cadmpeg_ir::native::catalogue::NativeVersionError::Unsupported {
                version,
                minimum: crate::native::SLDPRT_MIN_NATIVE_VERSION,
                maximum: crate::native::SLDPRT_NATIVE_VERSION,
            }
        ) if version == crate::native::SLDPRT_NATIVE_VERSION + 1
    ));
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
        .store(decoded.ir_mut().native.namespace_mut("sldprt"))
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
    let error = native.store(&mut namespace).unwrap_err();
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
    let error = native.store(&mut namespace).unwrap_err();
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
    entity.links = vec![crate::records::SketchInputLink {
        local_id: 7,
        entity_ref: "sldprt:feature-input:sketch-entity#missing".into(),
    }];
    entity.link_selector = Some(0);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
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
    let point_id = entities[1].id.clone();
    let second_point_id = entities[2].id.clone();
    entities[1].feature_ref = owner.clone();
    entities[1].local_id = Some(7);
    entities[1].kind = crate::records::SketchInputKind::Point;
    entities[2].feature_ref = owner;
    entities[2].local_id = Some(8);
    entities[2].kind = crate::records::SketchInputKind::ConstrainedPoint;
    entities[0].kind =
        crate::records::SketchInputKind::Relation(crate::records::SketchRelationKind::Midpoint);
    entities[0].links = vec![
        crate::records::SketchInputLink {
            local_id: 7,
            entity_ref: point_id,
        },
        crate::records::SketchInputLink {
            local_id: 8,
            entity_ref: second_point_id,
        },
    ];
    entities[0].link_selector = Some(0);
    for scalar in &mut native.feature_input_lanes[0].scalars {
        for operand in &mut scalar.operands {
            operand.entity_ref = None;
        }
    }
    for relation in &mut native.feature_input_lanes[0].relation_instances {
        for operand in &mut relation.operands {
            operand.entity_ref = None;
        }
    }

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).unwrap();
    let stored = crate::native::SldprtNative::load(&namespace).unwrap();
    assert_eq!(
        stored.feature_input_lanes[0].sketch_entities[0].links.len(),
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
    let error = native.store(&mut namespace).unwrap_err();
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
    let duplicate = native.feature_input_lanes[0].relation_instances[0].scalar_refs[0].clone();
    native.feature_input_lanes[0].relation_instances[0]
        .scalar_refs
        .push(duplicate);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
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
    assert_eq!(relation.scalar_refs.len(), 2);
    relation.scalar_refs.push(relation.scalar_refs[0].clone());
    namespace
        .set_arena("feature_input_relation_instances", &relations)
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
    let error = native.store(&mut namespace).unwrap_err();
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
    let wrong_target = native.feature_input_lanes[0].sketch_entities[0].id.clone();
    native.feature_input_lanes[0].scalars[0].operands[1].entity_ref = Some(wrong_target.clone());
    native.feature_input_lanes[0].relation_instances[0].operands[1].entity_ref = Some(wrong_target);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
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
    lane.sketch_entities[1].local_id = lane.sketch_entities[0].local_id;

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).unwrap();
}
