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
    let layer_records = super::layers(&ir).expect("write current layer");
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
        super::super::native_dispositions(&ir)
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
        super::super::native_dispositions(&ir)
            .expect("unlinked disposition")
            .record_disposition("rhino", "layers", NATIVE),
        NativeDisposition::Dropped
    );
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
        super::document::settings(&ir).expect("write linked current layer"),
        vec![super::short_chunk(0xa000_0038, 1)]
    );
    ir.model.presentation_layers[1].native_ref = None;
    assert!(super::document::settings(&ir)
        .expect("no unlinked current layer guess")
        .is_empty());
}
