// SPDX-License-Identifier: Apache-2.0
//! Layer writers resolve native state from current inward links.

use cadmpeg_ir::codec::write::NativeDisposition;
use cadmpeg_ir::ids::{Identity, LayerId};
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::presentation::PresentationLayer;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions, NativeRecord};
use serde_json::{json, Value};
use std::io::Cursor;

const NATIVE: &str = "rhino:presentation:layer#source";

fn native(id: &str, value: Value) -> NativeRecord {
    NativeRecord::new(
        Identity::new(id).expect("native identity"),
        serde_json::from_value(value).expect("native fields"),
    )
    .expect("native record")
}

fn layer(id: &str, name: &str, native_ref: Option<&str>) -> PresentationLayer {
    PresentationLayer {
        id: LayerId::mint(id).expect("layer identity"),
        name: name.into(),
        native_ref: native_ref.map(str::to_owned),
        description: None,
        parent: None,
        visible: Some(false),
        items: Vec::new(),
    }
}

#[test]
fn native_layer_fields_follow_the_current_neutral_link_and_edits() {
    let mut ir = CadIr::empty();
    ir.model.presentation_layers.push(layer(
        "cadir:model:layer#edited",
        "Current name",
        Some(NATIVE),
    ));
    ir.native.namespace_mut("rhino").arenas_mut().insert("layers".into(), vec![
        native("rhino:presentation:layer#unrelated", json!({"name":"Current name","color":NativeBytes::from([90_u8,80,70,0])})),
        native(NATIVE, json!({"archive_index":47,"name":"Source name","parent_uuid":"11111111-1111-1111-1111-111111111111","description":"Source description","visible":true,"color":NativeBytes::from([10_u8,20,30,0]),"iges_level":44,"locked":true})),
    ]);
    let before = ir.clone();
    let fields =
        super::own_layer(&ir, &ir.model.presentation_layers[0]).expect("exact inward native link");
    assert_eq!(fields["name"], "Source name");
    assert_eq!(fields["description"], "Source description");
    assert_eq!(fields["visible"], true);
    let archive = crate::chunks::ArchiveVersion::V8;
    let materials = super::materials::MaterialPlan::new(&ir).unwrap();
    let layer_records = super::layers(&ir, &materials).expect("write current layer");
    let source = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::test_support::test_dump::table(
                archive,
                0x1000_0014,
                &[crate::test_support::test_dump::short_chunk(
                    archive,
                    0xa000_0026,
                    202_608_010,
                )],
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0011, &layer_records),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("read written layer table");
    assert_eq!(decoded.ir().model.presentation_layers.len(), 1);
    let written = &decoded.ir().model.presentation_layers[0];
    assert_eq!(written.name, "Current name");
    assert_eq!(written.visible, Some(false));
    assert_eq!(written.parent, None);
    assert_eq!(written.description, None);
    let native = &decoded
        .ir()
        .native
        .namespace("rhino")
        .expect("Rhino namespace")
        .arenas()["layers"][0];
    assert_eq!(
        native.field("color"),
        Some(json!(NativeBytes::from([10_u8, 20, 30, 0])))
    );
    assert_eq!(native.field("iges_level"), Some(json!(44)));
    assert_eq!(native.field("locked"), Some(json!(true)));
    assert_eq!(native.field("neutral_ref"), None);
    assert_eq!(written.native_ref.as_deref(), Some(native.id()));
    assert_eq!(
        super::super::native_dispositions(&ir, crate::RhinoArchiveVersion::V8)
            .expect("layer disposition")
            .record_disposition("rhino", "layers", NATIVE),
        NativeDisposition::Regenerated
    );
    assert_eq!(ir, before, "writing does not strip retained native values");
    ir.model.presentation_layers[0].native_ref = None;
    assert!(
        super::own_layer(&ir, &ir.model.presentation_layers[0]).is_none(),
        "matching names do not establish a native link"
    );
    assert_eq!(
        super::super::native_dispositions(&ir, crate::RhinoArchiveVersion::V8)
            .expect("unlinked disposition")
            .record_disposition("rhino", "layers", NATIVE),
        NativeDisposition::Dropped
    );
}

#[test]
fn layer_linetype_resolves_source_archive_index() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let mut ir = CadIr::empty();
    ir.model
        .presentation_layers
        .push(layer("cadir:model:layer#edited", "layer", Some(NATIVE)));
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "layers".into(),
        vec![native(
            NATIVE,
            json!({"archive_index": 0, "name": "layer", "linetype_index": 7}),
        )],
    );
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "linetypes".into(),
        vec![native(
            "rhino:presentation:linetype#first",
            json!({"archive_index": 7, "name": "line", "segments": [], "taper_points": []}),
        )],
    );
    let mut bytes = Vec::new();
    let report = crate::RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    assert!(report.losses.iter().any(|loss| loss.code
        == crate::loss::RhinoLossCode::WriterLayerLinetypeReduced.kind()
        && loss.message.contains(NATIVE)
        && loss.message.contains("linetype index 7 is written as 0")));
    assert_eq!(
        report
            .native_dispositions
            .record_disposition("rhino", "layers", NATIVE),
        NativeDisposition::Dropped
    );
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let records = &decoded.ir().native.namespace("rhino").unwrap().arenas()["layers"];
    assert_eq!(records[0].field("linetype_index"), Some(json!(0)));
}

#[test]
fn layer_material_reference_follows_emitted_table_and_names_missing_material() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let mut ir = CadIr::empty();
    ir.model
        .presentation_layers
        .push(layer("cadir:model:layer#edited", "layer", Some(NATIVE)));
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "layers".into(),
        vec![native(
            NATIVE,
            json!({"archive_index":0,"name":"layer","material_index":7}),
        )],
    );
    let color = NativeBytes::from([0_u8, 0, 0, 0]);
    let material_id = "rhino:presentation:material#source";
    ir.native.namespace_mut("rhino").arenas_mut().insert("materials".into(), vec![native(material_id, json!({
        "archive_index":7,"name":"material","source_uuid":null,"plugin_uuid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        "ambient":color,"diffuse":color,"emission":color,"specular":color,"reflection":color,"transparent":color,
        "index_of_refraction":1.0,"reflectivity":0.0,"shine":0.0,"transparency":0.0,
        "reflection_glossiness":0.0,"refraction_glossiness":0.0,"fresnel_index_of_refraction":1.0,
        "shareable":false,"disable_lighting":false,"fresnel_reflections":false,
        "diffuse_texture_alpha_transparency":false,"rdk_instance_uuid":null,"texture_count":0,"textures":[]
    }))]);
    let mut bytes = Vec::new();
    let report = crate::RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    assert!(report.losses.iter().any(|loss| loss.code
        == crate::loss::RhinoLossCode::WriterLayerMaterialReduced.kind()
        && loss.message.contains(NATIVE)
        && loss.message.contains("material index 7 is written as 0")));
    assert_eq!(
        report
            .native_dispositions
            .record_disposition("rhino", "layers", NATIVE),
        NativeDisposition::Dropped
    );
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let records = &decoded.ir().native.namespace("rhino").unwrap().arenas()["layers"];
    assert_eq!(records[0].field("material_index"), Some(json!(0)));
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .remove("materials");
    let materials = super::materials::MaterialPlan::new(&ir).unwrap();
    assert_eq!(
        super::layer_material(&ir, &ir.model.presentation_layers[0], &materials),
        (7, -1)
    );
    assert!(super::layer_material_losses(&ir, &materials)
        .iter()
        .any(|loss| loss.message.contains(NATIVE)));
}

#[test]
fn invalid_linetype_is_named_and_valid_sibling_remains_linked() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let mut ir = CadIr::empty();
    ir.model
        .presentation_layers
        .push(layer("cadir:model:layer#edited", "layer", Some(NATIVE)));
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "layers".into(),
        vec![native(
            NATIVE,
            json!({"archive_index":0,"name":"layer","linetype_index":7}),
        )],
    );
    ir.native.namespace_mut("rhino").arenas_mut().insert("linetypes".into(), vec![
        native("rhino:presentation:linetype#invalid", json!({"archive_index":3,"name":"bad","segments":[{"length_millimeters":null}],"taper_points":[]})),
        native("rhino:presentation:linetype#valid", json!({"archive_index":7,"name":"valid","segments":[],"taper_points":[]})),
    ]);
    let mut bytes = Vec::new();
    let report = crate::RhinoCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    assert!(report.losses.iter().any(|loss| loss.code
        == crate::loss::RhinoLossCode::WriterLinetypeOmitted.kind()
        && loss.severity == cadmpeg_ir::report::Severity::Error
        && loss.message.contains("rhino:presentation:linetype#invalid")));
    assert_eq!(
        report.native_dispositions.record_disposition(
            "rhino",
            "linetypes",
            "rhino:presentation:linetype#invalid"
        ),
        NativeDisposition::Dropped
    );
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let arenas = decoded.ir().native.namespace("rhino").unwrap().arenas();
    assert_eq!(arenas["linetypes"].len(), 1);
    assert_eq!(arenas["linetypes"][0].field("name"), Some(json!("valid")));
    assert_eq!(arenas["layers"][0].field("linetype_index"), Some(json!(0)));
}

#[test]
fn current_layer_setting_tracks_the_neutral_native_ref_after_reordering() {
    let mut ir = CadIr::empty();
    ir.model.presentation_layers = vec![
        layer(
            "cadir:model:layer#first",
            "Source name",
            Some("rhino:presentation:layer#other"),
        ),
        layer("cadir:model:layer#second", "Other name", Some(NATIVE)),
    ];
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "layers".into(),
        vec![
            native(NATIVE, json!({"archive_index":47})),
            native(
                "rhino:presentation:layer#other",
                json!({"archive_index":12}),
            ),
        ],
    );
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "document_settings".into(),
        vec![native(
            "rhino:document:settings#0",
            json!({"current_layer_index":47}),
        )],
    );
    assert_eq!(
        super::document::settings(
            &ir,
            &super::materials::MaterialPlan::new(&ir).expect("material plan"),
            &mut Vec::new()
        )
        .expect("write linked current layer")
        .records,
        vec![super::short_chunk(0xa000_0038, 1)]
    );
    ir.model.presentation_layers[1].native_ref = None;
    assert!(super::document::settings(
        &ir,
        &super::materials::MaterialPlan::new(&ir).expect("material plan"),
        &mut Vec::new()
    )
    .expect("no unlinked current layer guess")
    .records
    .is_empty());
}
