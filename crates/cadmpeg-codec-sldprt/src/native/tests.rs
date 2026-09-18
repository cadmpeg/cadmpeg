// SPDX-License-Identifier: Apache-2.0
//! Native catalogue load and store tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::make_block;
use crate::test_support::resolved_feature_classes_with_ids;
use crate::test_support::resolved_features_payload_with_names;
use crate::test_support::sldprt_native;
use crate::test_support::sldprt_with_body;
use crate::test_support::sldprt_with_body_and_history;
use crate::test_support::sldprt_with_body_and_resolved_features;
use crate::test_support::sldprt_with_compact_relation_pair;
use crate::test_support::sldprt_with_nested_sketch_profile;
use crate::test_support::triangle_body;
use crate::SldprtCodec;

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
    entity.links = crate::records::SketchInputLinks::new(
        0,
        vec![crate::records::SketchInputLink {
            local_id: 7,
            entity_ref: "sldprt:feature-input:sketch-entity#missing".into(),
        }],
    );

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
    native.store(&mut namespace).unwrap();
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
    let duplicate = native.feature_input_lanes[0].relation_instances[0].scalar_refs()[0].clone();
    native.feature_input_lanes[0].relation_instances[0]
        .scalars
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
    assert_eq!(relation.scalar_refs().len(), 2);
    relation.scalars.push(relation.scalar_refs()[0].clone());
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
    let wrong_target = native.feature_input_lanes[0].sketch_entities[0]
        .id()
        .to_string();
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
    native.store(&mut namespace).unwrap();
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
    typed.store(&mut forged).unwrap();
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
