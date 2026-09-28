// SPDX-License-Identifier: Apache-2.0
//! Schema appearance resource-limit tests.

use super::{appearance_connected_to, appearance_record, texture_record};

fn schema_appearance_error(
    records: &[cadmpeg_protein::DecodedRecord],
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    super::super::appearances_from_schema_records(&ctx, records).unwrap_err()
}

#[test]
fn schema_appearance_property_map_refuses_collection_limit() {
    let property = cadmpeg_protein::property::DecodedProperty {
        value_offset: 0,
        content: cadmpeg_protein::property::PropertyContent::Value {
            value: cadmpeg_protein::property::PropertyValue::Float(0.5),
            connections: Vec::new(),
        },
    };
    let record = appearance_record(
        "GenericSchema",
        std::collections::BTreeMap::from([("generic_reflectivity".into(), property)]),
    );
    let error = schema_appearance_error(&[record], 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D appearance properties"));
}

#[test]
fn schema_appearance_property_name_refuses_retained_limit() {
    let property = cadmpeg_protein::property::DecodedProperty {
        value_offset: 0,
        content: cadmpeg_protein::property::PropertyContent::Value {
            value: cadmpeg_protein::property::PropertyValue::Float(0.5),
            connections: Vec::new(),
        },
    };
    let record = appearance_record(
        "GenericSchema",
        std::collections::BTreeMap::from([("generic_reflectivity".into(), property)]),
    );
    let error = schema_appearance_error(&[record], u64::MAX, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D appearance property name"));
}

#[test]
fn schema_appearance_vector_refuses_collection_limit() {
    let record = appearance_record("GenericSchema", Default::default());
    let error = schema_appearance_error(&[record], 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D appearances"));
}

#[test]
fn schema_texture_index_refuses_collection_limit() {
    let record = texture_record("aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb", "textures/a.png");
    let error = schema_appearance_error(&[record], 1, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D texture assets"));
}

#[test]
fn schema_texture_key_refuses_retained_limit() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let path = "textures/a.png";
    let record = texture_record(guid, path);
    let already_retained = path.len() + guid.len() + "UnifiedBitmapSchema".len();
    let error = schema_appearance_error(&[record], u64::MAX, u64::try_from(already_retained).unwrap());
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D texture asset key"));
}

#[test]
fn material_library_id_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = super::super::library_id(&ctx, "L").unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D appearance library ID"));
}

fn fixed_appearance_error(asset_lib_id: &str, max_retained: u64) -> cadmpeg_core::CodecError {
    let mut record = super::super::RECORD_MARKER.to_vec();
    for value in ["PhysMatSchema", "g", "b", asset_lib_id] {
        super::super::push_lp(&mut record, value).unwrap();
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    super::super::decode_fixed_record(&ctx, &record).unwrap_err()
}

#[test]
fn fixed_appearance_guid_copy_refuses_retained_limit() {
    let retained_fields = "PhysMatSchema".len() + "g".len() + "b".len();
    let error = fixed_appearance_error("", u64::try_from(retained_fields).unwrap());
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D fixed appearance GUID"));
}

#[test]
fn fixed_appearance_library_copy_refuses_retained_limit() {
    let retained_fields = "PhysMatSchema".len() + "g".len() + "b".len() + "L".len() + "g".len();
    let error = fixed_appearance_error("L", u64::try_from(retained_fields).unwrap());
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D appearance library ID"));
}

#[test]
fn schema_connected_texture_vector_refuses_collection_limit() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let records = [texture_record(guid, "textures/a.png"), appearance_connected_to(guid)];
    let error = schema_appearance_error(&records, 4, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D connected textures"));
}

macro_rules! schema_appearance_text_limit_test {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let record = appearance_record("GenericSchema", Default::default());
            let error = schema_appearance_error(&[record], u64::MAX, $limit);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

schema_appearance_text_limit_test!(schema_appearance_name_refuses_retained_limit, 0, "copy F3D appearance name");
schema_appearance_text_limit_test!(schema_appearance_guid_refuses_retained_limit, 9, "copy F3D appearance GUID");
schema_appearance_text_limit_test!(schema_appearance_visual_guid_refuses_retained_limit, 45, "copy F3D appearance visual GUID");
schema_appearance_text_limit_test!(schema_appearance_schema_refuses_retained_limit, 81, "copy F3D appearance schema");

#[test]
fn material_note_format_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let note = "Protein A record 1 rejected: B";
    policy.limits.max_retained_bytes = u64::try_from(note.len() - 1).unwrap();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = super::super::format_material_text_charged(
        &ctx,
        format_args!("Protein {} record {} rejected: {}", "A", 1, "B"),
        "retain F3D protein rejection note",
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D protein rejection note"));
}

#[test]
fn material_note_vector_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let mut notes = Vec::new();
    let error = super::super::push_material_item(
        &ctx,
        &mut notes,
        "note".to_owned(),
        "collect F3D protein rejection notes",
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D protein rejection notes"));
}

#[test]
fn material_appearance_merge_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let mut merged = Vec::new();
    let error = super::super::append_material_items(
        &ctx,
        &mut merged,
        vec![1, 2],
        "merge F3D fixed appearances",
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "merge F3D fixed appearances"));
}

#[test]
fn material_asset_appearance_vector_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let mut appearances = Vec::new();
    let error = super::super::append_material_items(
        &ctx,
        &mut appearances,
        vec![1, 2],
        "collect F3D asset appearances",
    )
    .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D asset appearances"));
}

#[test]
fn material_schema_id_index_refuses_collection_limit() {
    let (appearances, _) = super::schema_appearances(&[
        appearance_record("GenericSchema", Default::default()),
    ])
    .expect("schema appearance");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = super::super::index_schema_appearance_ids(&ctx, &appearances).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D schema appearance IDs"));
}

macro_rules! material_item_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("test decode context");
            let mut items = Vec::new();
            let error = super::super::push_material_item(&ctx, &mut items, 1, $operation)
                .unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

material_item_limit_test!(material_assignment_vector_refuses_collection_limit, "collect F3D material assignments");
material_item_limit_test!(material_browser_appearance_vector_refuses_collection_limit, "collect F3D browser body appearances");
material_item_limit_test!(material_body_override_vector_refuses_collection_limit, "collect F3D body appearance overrides");
material_item_limit_test!(material_assignment_appearance_vector_refuses_collection_limit, "collect F3D assignment appearances");
material_item_limit_test!(material_override_binding_vector_refuses_collection_limit, "collect F3D override appearance bindings");

#[test]
fn material_assignment_id_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let error = crate::ids::native_scoped_id_charged(&ctx, "BulkStream", "material-assignment", 1)
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID"));
}
