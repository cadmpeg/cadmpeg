// SPDX-License-Identifier: Apache-2.0
//! Current inward document attributes preserve independent native values.

use super::{properties, property, uncarried_attribute_losses};
use crate::loss::RhinoLossCode;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
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
        let losses =
            crate::neutral_projection::project(&ctx, &scan, Vec::new(), Vec::new(), &mut ir)
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
    let dispositions = crate::writer::native_dispositions(&ir, crate::RhinoArchiveVersion::V8)
        .expect("dispositions");
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
            "rhino.revisions.revision_count" => vec![Some(AttributeValue::Integer(9))],
            "rhino.applications.name" => {
                vec![Some(AttributeValue::String("Current application".into()))]
            }
            "rhino.document_notes.text" => vec![Some(AttributeValue::String(String::new()))],
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
    ir.model.attributes[0].values = vec![Some(AttributeValue::String("Current author".into()))];
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
            other.values = vec![Some(AttributeValue::String("Conflicting name".into()))];
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
    attribute.values = vec![Some(AttributeValue::String("false".into()))];
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
    let dispositions = crate::writer::native_dispositions(&ir, crate::RhinoArchiveVersion::V8)
        .expect("dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "document_notes", &source_id),
        NativeDisposition::Dropped
    );
}

#[test]
fn unreadable_scalar_attribute_omits_its_carrier_without_native_replay() {
    let mut ir = source();
    let source_id = native(&ir, "document_notes").id().to_owned();
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "rhino.document_notes.html")
        .unwrap();
    let id = attribute.id.clone();
    attribute.values = vec![Some(AttributeValue::Boolean(false)), None];
    let before = ir.clone();
    let output = properties(&ir).unwrap();
    assert_eq!(ir, before);
    assert_eq!(output.records.len(), 2);
    assert!(!output.written.contains(&source_id));
    assert!(!output.regenerated.contains(&source_id));
    assert_eq!(output.losses.len(), 1);
    assert_eq!(
        output.losses[0].code,
        RhinoLossCode::WriterAttributesOmitted.note("").code
    );
    assert!(output.losses[0].message.contains(id.as_str()));
    assert!(output.losses[0].message.contains("None positions [1]"));
    assert_eq!(
        native(&ir, "document_notes").field("html"),
        Some(Value::Bool(true))
    );
}

#[test]
fn structured_scalar_value_is_named_and_never_stringified() {
    let mut ir = source();
    let source_id = native(&ir, "document_notes").id().to_owned();
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "rhino.document_notes.text")
        .unwrap();
    let id = attribute.id.clone();
    attribute.values = vec![Some(AttributeValue::Json(
        cadmpeg_ir::attributes::AttributeJson::Null,
    ))];
    let before = ir.clone();
    let output = properties(&ir).unwrap();
    assert_eq!(ir, before);
    assert_eq!(output.records.len(), 2);
    assert!(!output.written.contains(&source_id));
    assert!(!output.regenerated.contains(&source_id));
    assert_eq!(output.losses.len(), 1);
    assert_eq!(
        output.losses[0].code,
        RhinoLossCode::WriterAttributesOmitted.note("").code
    );
    assert!(output.losses[0].message.contains(id.as_str()));
    assert_eq!(
        native(&ir, "document_notes").field("text"),
        Some(json!("Source notes"))
    );
}

#[test]
fn plain_scalar_attribute_without_carrier_names_its_own_omission() {
    let mut ir = CadIr::empty();
    let plain = cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#plain").unwrap();
    ir.model
        .attributes
        .push(cadmpeg_ir::attributes::SourceAttribute {
            id: plain.clone(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "author".into(),
            values: vec![Some(AttributeValue::String("source author".into()))],
        });
    let before = ir.clone();
    let mut losses = Vec::new();
    uncarried_attribute_losses(&ir, &mut losses);
    assert_eq!(ir, before);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        RhinoLossCode::WriterAttributesOmitted.note("").code
    );
    assert!(losses[0].message.contains(plain.as_str()));
}

#[test]
fn uncarried_holes_and_structured_values_have_named_attribute_losses() {
    let mut ir = CadIr::empty();
    let hole = cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#hole").unwrap();
    let structured = cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#structured").unwrap();
    ir.model
        .attributes
        .push(cadmpeg_ir::attributes::SourceAttribute {
            id: hole.clone(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "source".into(),
            values: vec![None, Some(AttributeValue::Integer(5)), None],
        });
    ir.model
        .attributes
        .push(cadmpeg_ir::attributes::SourceAttribute {
            id: structured.clone(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "source".into(),
            values: vec![Some(AttributeValue::EmptyReference)],
        });
    let before = ir.clone();
    let mut losses = Vec::new();
    uncarried_attribute_losses(&ir, &mut losses);
    assert_eq!(ir, before);
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code == RhinoLossCode::WriterAttributesOmitted.note("").code));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains(hole.as_str())
            && loss.message.contains("None positions [0, 2]")));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains(structured.as_str())));
}

#[test]
fn linked_preview_bytes_write_the_preview_record() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let mut ir = CadIr::empty();
    let preview_id = "rhino:document:preview#0";
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "previews".into(),
        vec![NativeRecord::new(
            preview_id.try_into().expect("preview identity"),
            serde_json::from_value(
                serde_json::json!({"byte_len": 4, "compressed": false, "sha256": "unused"}),
            )
            .expect("preview fields"),
        )
        .expect("preview record")],
    );
    let bytes = vec![0x42, 0x4d, 0x00, 0x00];
    ir.model.assets.push(cadmpeg_ir::assets::Asset {
        id: "cadir:model:asset#preview".try_into().expect("identity"),
        name: None,
        media_type: None,
        content: cadmpeg_ir::assets::AssetContent::Embedded {
            data: cadmpeg_ir::assets::AssetData::new(bytes.clone()).expect("preview bytes"),
        },
        native_ref: Some(preview_id.to_owned()),
    });
    ir.finalize();
    let exported = properties(&ir).expect("preview properties");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(exported.previews_written.len(), 1);
    assert!(exported.previews_written.contains(preview_id));

    let mut written = Vec::new();
    crate::RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(crate::RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut written))
        .expect("preview document writes");
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("written previews decode");

    let previews = &decoded
        .ir()
        .native
        .namespace("rhino")
        .expect("rhino namespace")
        .arenas()["previews"];
    assert_eq!(previews.len(), 1, "the preview record reappears");
    let asset = decoded
        .ir()
        .model
        .assets
        .iter()
        .find(|asset| asset.native_ref.as_deref() == Some(previews[0].id()))
        .expect("the preview asset reappears");
    let cadmpeg_ir::assets::AssetContent::Embedded { data } = &asset.content else {
        panic!("the preview asset keeps embedded bytes");
    };
    assert_eq!(
        data.as_bytes(),
        bytes.as_slice(),
        "preview bytes survive the round trip"
    );
}

#[test]
fn annotation_settings_and_grid_defaults_round_trip_from_typed_fields() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let mut ir = CadIr::empty();
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "annotation_settings".into(),
        vec![NativeRecord::new(
            "rhino:document:annotation_settings#current"
                .try_into()
                .expect("identity"),
            serde_json::from_value(serde_json::json!({
                "dimension_scale": 1.0,
                "text_height_mm": 3.5,
                "extension_line_extension_mm": 1.0,
                "extension_line_offset_mm": 0.5,
                "arrow_length_mm": 2.5,
                "arrow_width_mm": 1.0,
                "center_mark_mm": 1.5,
                "dimension_units": 2,
                "arrow_type": 1,
                "angular_units": 1,
                "length_format": 1,
                "angle_format": 1,
                "obsolete_text_alignment": 0,
                "resolution": 100,
                "font_face": "Arial",
                "world_view_text_scale": 1.0,
                "annotation_scaling": true,
                "world_view_hatch_scale": 2.0,
                "hatch_scaling": false,
                "model_space_annotation_scaling": true,
                "layout_space_annotation_scaling": false,
                "use_dimension_layer": false,
                "dimension_layer_uuid": null,
            }))
            .expect("annotation settings fields"),
        )
        .expect("annotation settings record")],
    );
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "grid_defaults".into(),
        vec![NativeRecord::new(
            "rhino:document:grid_defaults#current"
                .try_into()
                .expect("identity"),
            serde_json::from_value(serde_json::json!({
                "grid_spacing_mm": 10.0,
                "snap_spacing_mm": 1.0,
                "grid_line_count": 100,
                "thick_line_frequency": 10,
                "show_grid": true,
                "show_grid_axes": true,
                "show_world_axes": false,
            }))
            .expect("grid default fields"),
        )
        .expect("grid defaults record")],
    );
    ir.finalize();
    let exported =
        super::table_settings(&ir, crate::RhinoArchiveVersion::V8).expect("settings tables");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(exported.written.len(), 2);
    assert_eq!(exported.records.len(), 2);

    let mut written = Vec::new();
    crate::RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(crate::RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut written))
        .expect("settings document writes");
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("written settings decode");
    for arena in ["annotation_settings", "grid_defaults"] {
        let records = &decoded
            .ir()
            .native
            .namespace("rhino")
            .expect("rhino namespace")
            .arenas()[arena];
        assert_eq!(records.len(), 1, "{arena} reappears");
        let before = &ir.native.namespace("rhino").expect("ns").arenas()[arena][0];
        assert_eq!(
            records[0].fields().expect("redecoded fields"),
            before.fields().expect("authored fields"),
            "{arena} round trips"
        );
    }
}

#[test]
fn minimal_render_settings_round_trip() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let mut ir = CadIr::empty();
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "render_settings".into(),
        vec![NativeRecord::new(
            "rhino:document:render_settings#current"
                .try_into()
                .expect("identity"),
            serde_json::from_value(serde_json::json!({
                "custom_image_size": false,
                "scale_background_to_fit": false,
                "transparent_background": false,
                "image_width_pixels": 800,
                "image_height_pixels": 600,
                "ambient_light": "0a141e00",
                "background_style": 0,
                "background_color": "ffffff00",
                "background_bitmap_path": "",
                "use_hidden_lights": false,
                "depth_cue": false,
                "flat_shade": false,
                "render_backfaces": true,
                "render_points": false,
                "render_curves": false,
                "render_isoparams": false,
                "render_mesh_edges": false,
                "render_annotations": false,
                "antialias_style": 1,
                "shadowmap_style": 1,
                "shadowmap_size_pixels": [1000, 1000],
                "shadowmap_offset_mm": 750.0,
                "specific_viewport": "",
                "named_view": "",
                "snapshot": "",
            }))
            .expect("render settings fields"),
        )
        .expect("render settings record")],
    );
    ir.finalize();
    let exported =
        super::table_settings(&ir, crate::RhinoArchiveVersion::V8).expect("settings tables");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);

    let mut written = Vec::new();
    crate::RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(crate::RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut written))
        .expect("render settings document writes");
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("written render settings decode");
    eprintln!(
        "LOSSES2={:?}",
        decoded
            .report()
            .losses
            .iter()
            .map(|l| (l.code.to_string(), l.message.clone()))
            .collect::<Vec<_>>()
    );
    let records = &decoded
        .ir()
        .native
        .namespace("rhino")
        .expect("rhino namespace")
        .arenas()["render_settings"];
    assert_eq!(records.len(), 1, "render settings reappear");
}

#[test]
fn modern_render_settings_round_trip_from_typed_fields() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let mut ir = CadIr::empty();
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "render_settings".into(),
        vec![NativeRecord::new(
            "rhino:document:render_settings#current"
                .try_into()
                .expect("identity"),
            serde_json::from_value(serde_json::json!({
                "custom_image_size": true,
                "scale_background_to_fit": true,
                "transparent_background": true,
                "image_width_pixels": 1920,
                "image_height_pixels": 1080,
                "image_dpi": 144.0,
                "image_unit_system": 9,
                "ambient_light": "0a141e00",
                "background_style": 1,
                "background_color": "28323c00",
                "background_bottom_color": "46505a00",
                "background_bitmap_path": "C:\\renders\\backdrop.png",
                "use_hidden_lights": false,
                "depth_cue": true,
                "flat_shade": false,
                "render_backfaces": true,
                "render_points": false,
                "render_curves": true,
                "render_isoparams": false,
                "render_mesh_edges": true,
                "render_annotations": false,
                "antialias_style": 2,
                "shadowmap_style": 1,
                "shadowmap_size_pixels": [512, 512],
                "shadowmap_offset_mm": 0.5,
                "obsolete_focal_blur": [1.0, 2.0, 3.0, 4.0, 5.0],
                "rendering_source": 2,
                "specific_viewport": "Perspective",
                "named_view": "Top",
                "snapshot": "Snap",
                "force_viewport_aspect_ratio": true,
            }))
            .expect("render settings fields"),
        )
        .expect("render settings record")],
    );
    ir.finalize();
    let exported =
        super::table_settings(&ir, crate::RhinoArchiveVersion::V8).expect("settings tables");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(exported.written.len(), 1);

    let mut written = Vec::new();
    crate::RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(crate::RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut written))
        .expect("render settings document writes");
    std::fs::write("/tmp/render_test.3dm", &written).expect("dump");
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("written render settings decode");
    eprintln!(
        "LOSSES2={:?}",
        decoded
            .report()
            .losses
            .iter()
            .map(|l| (l.code.to_string(), l.message.clone()))
            .collect::<Vec<_>>()
    );
    let records = &decoded
        .ir()
        .native
        .namespace("rhino")
        .expect("rhino namespace")
        .arenas()["render_settings"];
    assert_eq!(records.len(), 1, "render settings reappear");
    assert_eq!(
        records[0].fields().expect("redecoded fields"),
        ir.native.namespace("rhino").expect("ns").arenas()["render_settings"][0]
            .fields()
            .expect("authored fields"),
        "modern render settings round trip"
    );
}

#[test]
fn legacy_render_settings_write_the_legacy_form_below_archive_60() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let mut ir = CadIr::empty();
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "render_settings".into(),
        vec![NativeRecord::new(
            "rhino:document:render_settings#current"
                .try_into()
                .expect("identity"),
            serde_json::from_value(serde_json::json!({
                "custom_image_size": false,
                "scale_background_to_fit": false,
                "transparent_background": false,
                "image_width_pixels": 640,
                "image_height_pixels": 480,
                "ambient_light": "00000000",
                "background_style": 0,
                "background_color": "ffffffff",
                "background_bitmap_path": "",
                "use_hidden_lights": false,
                "depth_cue": false,
                "flat_shade": false,
                "render_backfaces": true,
                "render_points": false,
                "render_curves": false,
                "render_isoparams": false,
                "render_mesh_edges": false,
                "render_annotations": false,
                "antialias_style": 0,
                "shadowmap_style": 0,
                "shadowmap_size_pixels": [256, 256],
                "shadowmap_offset_mm": 1.0,
                "specific_viewport": "",
                "named_view": "",
                "snapshot": "",
            }))
            .expect("legacy render settings fields"),
        )
        .expect("render settings record")],
    );
    ir.finalize();
    let exported =
        super::table_settings(&ir, crate::RhinoArchiveVersion::V5).expect("settings tables");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(exported.written.len(), 1);

    let mut written = Vec::new();
    crate::RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(crate::RhinoArchiveVersion::V5.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut written))
        .expect("legacy render settings document writes");
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("written legacy render settings decode");
    let records = &decoded
        .ir()
        .native
        .namespace("rhino")
        .expect("rhino namespace")
        .arenas()["render_settings"];
    assert_eq!(records.len(), 1, "render settings reappear");
    let authored = ir.native.namespace("rhino").expect("ns").arenas()["render_settings"][0]
        .fields()
        .expect("authored fields");
    let redecoded = records[0].fields().expect("redecoded fields");
    for (key, value) in redecoded.iter() {
        match authored.get(key) {
            Some(expected) => assert_eq!(value, expected, "legacy field {key} round trips"),
            None => assert!(
                value.is_null(),
                "unheld legacy field {key} re-decodes unset"
            ),
        }
    }
    for (key, value) in authored.iter().filter(|(_, value)| !value.is_null()) {
        assert_eq!(
            redecoded.get(key),
            Some(value),
            "held legacy field {key} survives"
        );
    }
}
