// SPDX-License-Identifier: Apache-2.0
//! Native catalogue load and store tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
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
    entities[0].links = crate::records::SketchInputLinks::new(
        0,
        vec![
            crate::records::SketchInputLink {
                local_id: 7,
                entity_ref: point_id,
            },
            crate::records::SketchInputLink {
                local_id: 8,
                entity_ref: second_point_id,
            },
        ],
    );
    let lane = &mut native.feature_input_lanes[0];
    for (index, local_id) in [(1, 7u32), (2, 8u32)] {
        let offset = lane.sketch_entities[index].offset() as usize + 88;
        lane.native_payload[offset..offset + 4].copy_from_slice(&local_id.to_le_bytes());
    }
    for entity in &mut lane.sketch_entities {
        entity.object_index = crate::resolved_features::markers::marker_object_index(
            &lane.native_payload,
            entity.offset() as usize,
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
        ("ordinal", serde_json::json!(3), "ordinal"),
        ("offset", serde_json::json!(u64::MAX), "offset"),
        ("object_index", serde_json::json!(77), "object index"),
        ("local_id", serde_json::json!(77), "local object id"),
    ] {
        let mut wire = original.clone();
        wire["sketch_input_entities"][0][field] = value;
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(wire).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(error.to_string().contains(message), "{field}: {error}");
    }
    for field in ["ordinal", "offset"] {
        let mut wire = original.clone();
        wire["sketch_input_entities"][1][field] = wire["sketch_input_entities"][0][field].clone();
        let namespace: cadmpeg_ir::NativeNamespace = serde_json::from_value(wire).unwrap();
        let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
        assert!(error.to_string().contains(field), "{field}: {error}");
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
