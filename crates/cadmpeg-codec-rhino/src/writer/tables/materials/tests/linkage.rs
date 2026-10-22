// SPDX-License-Identifier: Apache-2.0
//! Material values survive inward projection and current-state synthesis.

use super::super::{native_complete, MaterialPlan};
use super::{materials, native_index};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::NativeDisposition;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::{CadIr, NativeRecord};
use serde_json::{json, Value};

fn material() -> NativeRecord {
    let packed = |bytes| serde_json::to_value(NativeBytes::new(bytes)).expect("packed color");
    NativeRecord::new(
        "rhino:presentation:material#11111111-2222-3333-4444-555555555555".try_into().expect("native identity"),
        serde_json::from_value(json!({
            "archive_index":7,
            "source_uuid":"11111111-2222-3333-4444-555555555555",
            "name":"Source material",
            "plugin_uuid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "ambient":packed([1_u8,2,3,4]),
            "diffuse":packed([10_u8,20,30,40]),
            "emission":packed([5_u8,6,7,8]),
            "specular":packed([9_u8,10,11,12]),
            "reflection":packed([13_u8,14,15,16]),
            "transparent":packed([17_u8,18,19,20]),
            "index_of_refraction":1.25,"reflectivity":0.125,"shine":2.5,"transparency":0.25,
            "texture_count":0,"textures":[],
            "shareable":true,"disable_lighting":true,"fresnel_reflections":true,
            "reflection_glossiness":0.375,"refraction_glossiness":0.625,"fresnel_index_of_refraction":1.5,
            "rdk_instance_uuid":"bbbbbbbb-cccc-dddd-eeee-ffffffffffff",
            "diffuse_texture_alpha_transparency":true
        })).expect("typed material fields"),
    ).expect("native material")
}

pub(super) fn read(records: &[Vec<u8>], project: bool) -> CadIr {
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::writer::tests::table(crate::writer::TCODE_SETTINGS_TABLE, &[])
                .expect("settings"),
            crate::writer::tests::table(crate::writer::TCODE_MATERIAL_TABLE, records)
                .expect("material table"),
            crate::writer::tests::table(crate::writer::TCODE_OBJECT_TABLE, &[])
                .expect("object table"),
        ],
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, view) = DecodeContext::from_root_bytes(&archive, &arena, &policy).expect("context");
    let scan = crate::container::scan(&ctx, &archive).expect("scan material archive");
    let mut ir = CadIr::empty();
    let installed =
        crate::presentation::install(&ctx, view, &scan, &mut ir).expect("read material grammar");
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    assert!(
        installed.opaque_records.is_empty(),
        "{:?}",
        installed.opaque_records
    );
    if project {
        let native = serde_json::to_value(&ir.native).expect("native snapshot");
        let losses =
            crate::neutral_projection::project(&ctx, &scan, &mut ir).expect("project material");
        assert!(losses.is_empty(), "{losses:?}");
        assert_eq!(
            serde_json::to_value(&ir.native).expect("projected native"),
            native
        );
    }
    ir
}

pub(super) fn source() -> CadIr {
    let record = material();
    assert!(native_complete(&record));
    let fields = record.fields().expect("source fields");
    let encoded = super::super::encode_material(record.id(), 0, None, Some(&fields), None)
        .expect("simple native material");
    read(&[encoded], true)
}

fn native(ir: &CadIr) -> &NativeRecord {
    let records = &ir.native.namespace("rhino").expect("namespace").arenas()["materials"];
    assert_eq!(records.len(), 1);
    &records[0]
}

#[test]
fn material_projection_keeps_all_native_fields_and_uses_only_the_inward_link() {
    let ir = source();
    let record = native(&ir);
    assert_eq!(ir.model.appearances.len(), 1);
    assert_eq!(
        ir.model.appearances[0].native_ref.as_deref(),
        Some(record.id())
    );
    assert_eq!(record.field("neutral_ref"), None);
    assert_eq!(record.field("name"), Some(json!("Source material")));
    assert_eq!(record.field("reflectivity"), Some(json!(0.125)));
    for name in [
        "ambient",
        "diffuse",
        "emission",
        "specular",
        "reflection",
        "transparent",
    ] {
        assert_eq!(record.field(name), material().field(name));
    }
    assert_eq!(record.field("plugin_uuid"), material().field("plugin_uuid"));
    assert_eq!(
        record.field("rdk_instance_uuid"),
        material().field("rdk_instance_uuid")
    );
}

#[test]
fn current_material_edits_win_without_changing_the_typed_source_values() {
    let mut ir = source();
    let before = serde_json::to_value(&ir.native).expect("source snapshot");
    let source_id = native(&ir).id().to_owned();
    let appearance = &mut ir.model.appearances[0];
    appearance.name = Some("Current material".into());
    appearance.base_color =
        Some(cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("current color"));
    appearance.properties.insert(
        cadmpeg_core::text::NonBlankString::new("reflectivity").expect("property"),
        cadmpeg_ir::scalar::FiniteReal::new(0.75).expect("current reflectivity"),
    );
    let encoded = materials(&ir).expect("current material output");
    assert_eq!(encoded.len(), 1);
    let output = read(&encoded, false);
    let current = native(&output);
    assert_eq!(current.field("name"), Some(json!("Current material")));
    assert_eq!(
        current.field("diffuse"),
        Some(
            serde_json::to_value(NativeBytes::new([255_u8, 0, 0, 0]))
                .expect("current packed color")
        )
    );
    assert_eq!(current.field("reflectivity"), Some(json!(0.75)));
    for name in [
        "ambient",
        "emission",
        "specular",
        "reflection",
        "transparent",
        "plugin_uuid",
        "rdk_instance_uuid",
        "shareable",
        "disable_lighting",
    ] {
        assert_eq!(current.field(name), native(&ir).field(name));
    }
    assert_eq!(
        serde_json::to_value(&ir.native).expect("unchanged source"),
        before
    );
    let dispositions = crate::writer::native_dispositions(&ir).expect("dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "materials", &source_id),
        NativeDisposition::Regenerated
    );
    ir.model.appearances[0].name = None;
    let unnamed = read(&materials(&ir).expect("removed current name"), false);
    assert_eq!(native(&unnamed).field("name"), Some(json!("")));
}

#[test]
fn removing_the_last_current_appearance_reemits_all_simple_native_material_values() {
    let mut ir = source();
    let source_id = native(&ir).id().to_owned();
    let before = native(&ir).fields().expect("source native fields");
    ir.model.appearances.clear();
    assert!(ir.scope.model);
    assert_eq!(
        native_index(&ir, &source_id).expect("material index"),
        Some(0)
    );
    let encoded = materials(&ir).expect("typed W1 output");
    assert_eq!(encoded.len(), 1);
    let output = read(&encoded, false);
    assert_eq!(
        native(&output).fields().expect("written typed fields"),
        before
    );
    assert_eq!(
        native(&ir).fields().expect("untouched typed fields"),
        before
    );
    let dispositions = crate::writer::native_dispositions(&ir).expect("dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "materials", &source_id),
        NativeDisposition::Written
    );
}

#[test]
fn unsupported_absent_legacy_fields_are_not_filled_to_claim_native_w1_support() {
    let mut ir = CadIr::empty();
    let source = material();
    let mut fields = source.fields().expect("material fields");
    fields.insert("reflection_glossiness".into(), Value::Null);
    fields.insert("refraction_glossiness".into(), Value::Null);
    fields.insert("fresnel_index_of_refraction".into(), Value::Null);
    fields.insert("rdk_instance_uuid".into(), Value::Null);
    fields.insert("fresnel_reflections".into(), Value::Bool(false));
    fields.insert("diffuse_texture_alpha_transparency".into(), Value::Null);
    let record = NativeRecord::new(source.id().try_into().expect("identity"), fields)
        .expect("legacy field states");
    assert!(!native_complete(&record));
    let id = record.id().to_owned();
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("materials".into(), vec![record]);
    let before = serde_json::to_value(&ir.native).expect("legacy snapshot");
    assert!(materials(&ir)
        .expect("unsupported legacy omission")
        .is_empty());
    assert_eq!(
        native_index(&ir, &id).expect("unsupported material index"),
        None
    );
    let dispositions = crate::writer::native_dispositions(&ir).expect("dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "materials", &id),
        NativeDisposition::Dropped
    );
    assert_eq!(
        serde_json::to_value(&ir.native).expect("legacy state remains"),
        before
    );
}

#[test]
fn current_material_setting_resolves_inward_and_survives_a_native_only_material() {
    let mut ir = source();
    let source_id = native(&ir).id().to_owned();
    let settings = NativeRecord::new(
        "rhino:document:settings#material"
            .try_into()
            .expect("settings identity"),
        serde_json::from_value(json!({"current_material_index":0,"current_material_source":2}))
            .expect("settings fields"),
    )
    .expect("settings");
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("document_settings".into(), vec![settings]);
    let expected = crate::writer::crc_chunk(
        0x2000_8039,
        &[0_i32.to_le_bytes(), 2_i32.to_le_bytes()].concat(),
    )
    .expect("expected setting");
    assert!(crate::writer::tables::document::settings(
        &ir,
        &MaterialPlan::new(&ir).expect("material plan"),
        &mut Vec::new()
    )
    .expect("linked setting")
    .contains(&expected));
    let mut other = ir.model.appearances[0].clone();
    other.id = "test:appearance:material#unlinked"
        .try_into()
        .expect("independent appearance identity");
    other.native_ref = None;
    ir.model.appearances.insert(0, other);
    let reordered = crate::writer::crc_chunk(
        0x2000_8039,
        &[1_i32.to_le_bytes(), 2_i32.to_le_bytes()].concat(),
    )
    .expect("reordered current setting");
    assert!(crate::writer::tables::document::settings(
        &ir,
        &MaterialPlan::new(&ir).expect("material plan"),
        &mut Vec::new()
    )
    .expect("reordered inward setting")
    .contains(&reordered));
    ir.model.appearances.clear();
    assert_eq!(
        native_index(&ir, &source_id).expect("material index"),
        Some(0)
    );
    assert!(crate::writer::tables::document::settings(
        &ir,
        &MaterialPlan::new(&ir).expect("material plan"),
        &mut Vec::new()
    )
    .expect("native-only setting")
    .contains(&expected));
}

#[test]
fn native_only_nil_source_uuid_remains_absent_instead_of_becoming_a_generated_uuid() {
    let mut ir = source();
    let source = native(&ir);
    let mut fields = source.fields().expect("source values");
    fields.insert("source_uuid".into(), Value::Null);
    let record = NativeRecord::new(
        source.id().try_into().expect("native identity"),
        fields.clone(),
    )
    .expect("nil native source uuid");
    assert!(native_complete(&record));
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("materials".into(), vec![record]);
    ir.model.appearances.clear();
    let output = read(&materials(&ir).expect("nil uuid native W1 output"), false);
    assert_eq!(
        native(&output).fields().expect("native written values"),
        fields
    );
}

#[test]
fn present_nil_optional_native_uuids_are_not_claimed_written_as_absent_values() {
    for field in ["source_uuid", "rdk_instance_uuid"] {
        let mut ir = source();
        let source = native(&ir);
        let id = source.id().to_owned();
        let mut fields = source.fields().expect("current native fields");
        fields.insert(field.into(), json!("00000000-0000-0000-0000-000000000000"));
        let record = NativeRecord::new(
            id.as_str().try_into().expect("native identity"),
            fields.clone(),
        )
        .expect("present nil optional uuid");
        assert!(!native_complete(&record));
        ir.native
            .namespace_mut("rhino")
            .arenas_mut()
            .insert("materials".into(), vec![record]);
        ir.model.appearances.clear();
        assert!(materials(&ir)
            .expect("unrepresentable present nil omission")
            .is_empty());
        assert_eq!(native_index(&ir, &id).expect("material index"), None);
        assert_eq!(
            native(&ir).fields().expect("native field state retained"),
            fields
        );
        let dispositions = crate::writer::native_dispositions(&ir).expect("native dispositions");
        assert_eq!(
            dispositions.record_disposition("rhino", "materials", &id),
            NativeDisposition::Dropped
        );
    }
}
