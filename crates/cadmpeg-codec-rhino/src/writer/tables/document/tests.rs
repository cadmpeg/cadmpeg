// SPDX-License-Identifier: Apache-2.0
//! Current inward document attributes preserve independent native values.

use super::{properties, property};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::write::NativeDisposition;
use cadmpeg_ir::{CadIr, NativeRecord};
use serde_json::{json, Map, Value};

fn fields(arena: &str) -> Map<String, Value> {
    serde_json::from_value(match arena {
        "revisions" => json!({"created_by":"Source author","created_utc_fields":[1,2,3,4,5,126,6,127],"last_edited_by":"Source editor","last_edited_utc_fields":[8,9,10,11,5,126,4,158],"revision_count":7}),
        "document_notes" => json!({"html":true,"text":"Source notes","visible":true,"window_rectangle":[10,20,300,400],"locked":true}),
        "applications" => json!({"name":"Source application","url":"https://example.invalid/source","details":"Source application details"}),
        _ => panic!("document property arena"),
    }).expect("native document fields")
}

fn read(records: &[Vec<u8>], project: bool) -> CadIr {
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::writer::tests::table(crate::writer::TCODE_PROPERTIES_TABLE, records)
                .expect("properties table"),
            crate::writer::tests::table(crate::writer::TCODE_SETTINGS_TABLE, &[])
                .expect("settings table"),
            crate::writer::tests::table(crate::writer::TCODE_OBJECT_TABLE, &[])
                .expect("objects table"),
        ],
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&archive, &arena, &policy).expect("decode context");
    let scan = crate::container::scan(&ctx, &archive).expect("scan properties");
    let mut ir = CadIr::empty();
    let installed = crate::document_data::install(&ctx, &scan, &mut ir)
        .expect("read actual document property grammar");
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    assert!(
        installed.opaque_records.is_empty(),
        "{:?}",
        installed.opaque_records
    );
    if project {
        let before = serde_json::to_value(&ir.native).expect("source native");
        let losses = crate::neutral_projection::project(&ctx, &scan, &mut ir)
            .expect("project actual document fields");
        assert!(losses.is_empty(), "{losses:?}");
        assert_eq!(
            serde_json::to_value(&ir.native).expect("native after projection"),
            before
        );
    }
    ir
}

fn source() -> CadIr {
    let encoded = ["revisions", "document_notes", "applications"]
        .map(|arena| property(arena, &fields(arena)).expect("author actual document property"));
    read(&encoded, true)
}

fn native<'a>(ir: &'a CadIr, arena: &str) -> &'a NativeRecord {
    let records = &ir.native.namespace("rhino").expect("namespace").arenas()[arena];
    assert_eq!(records.len(), 1, "{arena}");
    &records[0]
}

#[test]
fn document_projection_keeps_author_application_and_note_values_without_backlinks() {
    let ir = source();
    assert_eq!(ir.model.attributes.len(), 13);
    for arena in ["revisions", "document_notes", "applications"] {
        let record = native(&ir, arena);
        assert_eq!(
            record.fields().expect("native source values"),
            fields(arena)
        );
        assert_eq!(record.field("neutral_attributes"), None);
        let expected = if arena == "applications" { 3 } else { 5 };
        assert_eq!(
            ir.model
                .attributes
                .iter()
                .filter(|attribute| attribute.native_ref.as_deref() == Some(record.id()))
                .count(),
            expected
        );
    }
}

#[test]
fn equal_current_attributes_and_absent_last_counterparts_write_the_same_native_records() {
    let mut ir = source();
    let linked = properties(&ir).expect("equal current attributes");
    assert!(linked.losses.is_empty(), "{:?}", linked.losses);
    assert!(linked.regenerated.is_empty());
    assert_eq!(linked.written.len(), 3);
    let before = serde_json::to_value(&ir.native).expect("native snapshot");
    ir.model.attributes.clear();
    assert!(ir.scope.model);
    let unlinked = properties(&ir).expect("native W1 properties");
    assert!(unlinked.losses.is_empty(), "{:?}", unlinked.losses);
    assert_eq!(unlinked.records, linked.records);
    assert_eq!(unlinked.written, linked.written);
    let output = read(&unlinked.records, false);
    let dispositions = crate::writer::native_dispositions(&ir).expect("dispositions");
    for arena in ["revisions", "document_notes", "applications"] {
        let record = native(&ir, arena);
        assert_eq!(
            native(&output, arena)
                .fields()
                .expect("native written values"),
            record.fields().expect("held values")
        );
        assert_eq!(
            dispositions.record_disposition("rhino", arena, record.id()),
            NativeDisposition::Written
        );
    }
    assert_eq!(
        serde_json::to_value(&ir.native).expect("native state remains"),
        before
    );
}

#[test]
fn current_document_edits_and_explicit_empty_text_override_only_their_actual_roles() {
    let mut ir = source();
    let before = serde_json::to_value(&ir.native).expect("source snapshot");
    for attribute in &mut ir.model.attributes {
        attribute.values = match attribute.name.as_str() {
            "rhino.revisions.revision_count" => vec![AttributeValue::Integer(9)],
            "rhino.applications.name" => vec![AttributeValue::String("Current application".into())],
            "rhino.document_notes.text" => vec![AttributeValue::String(String::new())],
            _ => continue,
        };
    }
    let exported = properties(&ir).expect("current property edits");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert!(exported.written.is_empty());
    assert_eq!(exported.regenerated.len(), 3);
    let output = read(&exported.records, false);
    assert_eq!(
        native(&output, "revisions").field("revision_count"),
        Some(json!(9))
    );
    assert_eq!(
        native(&output, "applications").field("name"),
        Some(json!("Current application"))
    );
    assert_eq!(
        native(&output, "document_notes").field("text"),
        Some(json!(""))
    );
    for (arena, changed, value) in [
        ("revisions", "revision_count", json!(9)),
        ("applications", "name", json!("Current application")),
        ("document_notes", "text", json!("")),
    ] {
        let mut expected = fields(arena);
        expected.insert(changed.into(), value);
        assert_eq!(
            native(&output, arena).fields().expect("all output values"),
            expected
        );
    }
    assert_eq!(
        serde_json::to_value(&ir.native).expect("unchanged source values"),
        before
    );
}

#[test]
fn partial_current_attribute_groups_keep_other_current_typed_native_values() {
    let mut ir = source();
    ir.model
        .attributes
        .retain(|attribute| attribute.name == "rhino.revisions.created_by");
    ir.model.attributes[0].values = vec![AttributeValue::String("Current author".into())];
    let exported = properties(&ir).expect("partial current override");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(exported.regenerated.len(), 1);
    assert_eq!(exported.written.len(), 2);
    let output = read(&exported.records, false);
    let mut revision = fields("revisions");
    revision.insert("created_by".into(), json!("Current author"));
    assert_eq!(
        native(&output, "revisions")
            .fields()
            .expect("current revision"),
        revision
    );
    assert_eq!(
        native(&output, "document_notes")
            .fields()
            .expect("independent notes"),
        fields("document_notes")
    );
    assert_eq!(
        native(&output, "applications")
            .fields()
            .expect("independent application"),
        fields("applications")
    );
}

#[test]
fn duplicate_or_unrecognized_linked_document_roles_are_named_and_not_selected() {
    for duplicate in [true, false] {
        let mut ir = source();
        let source_id = native(&ir, "applications").id().to_owned();
        let index = ir
            .model
            .attributes
            .iter()
            .position(|attribute| attribute.name == "rhino.applications.name")
            .expect("application name role");
        if duplicate {
            let mut other = ir.model.attributes[index].clone();
            other.id = "test:model:attribute#duplicate-application-name"
                .try_into()
                .expect("duplicate attribute identity");
            other.values = vec![AttributeValue::String("Conflicting name".into())];
            ir.model.attributes.push(other);
        } else {
            ir.model.attributes[index].name = "rhino.applications.unrecognized".into();
        }
        let before = serde_json::to_value(&ir.native).expect("native source");
        let exported = properties(&ir).expect("report current role conflict");
        assert_eq!(exported.records.len(), 2);
        assert!(exported
            .losses
            .iter()
            .any(|loss| loss.message.contains(&source_id)
                && loss
                    .message
                    .contains(if duplicate { "duplicate" } else { "field role" })));
        assert!(!exported.written.contains(&source_id));
        assert!(!exported.regenerated.contains(&source_id));
        assert_eq!(
            serde_json::to_value(&ir.native).expect("source fields kept"),
            before
        );
    }
}

#[test]
fn wrong_current_boolean_type_is_not_defaulted_to_a_false_value() {
    let mut ir = source();
    let source_id = native(&ir, "document_notes").id().to_owned();
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "rhino.document_notes.html")
        .expect("html role");
    attribute.values = vec![AttributeValue::String("false".into())];
    let exported = properties(&ir).expect("report incompatible html type");
    assert_eq!(exported.records.len(), 2);
    assert!(exported
        .losses
        .iter()
        .any(|loss| loss.message.contains(&source_id)
            && loss.message.contains("html needs a boolean value")));
    assert_eq!(
        native(&ir, "document_notes").field("html"),
        Some(Value::Bool(true))
    );
    let dispositions = crate::writer::native_dispositions(&ir).expect("dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "document_notes", &source_id),
        NativeDisposition::Dropped
    );
}
