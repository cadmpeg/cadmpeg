// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::writer::TCODE_MATERIAL_TABLE;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::scalar::FiniteReal;

#[test]
fn current_material_values_override_native_copies_without_source_bytes() {
    let mut ir = CadIr::empty();
    let id: cadmpeg_ir::ids::AppearanceId = "test:appearance:material#edited"
        .try_into()
        .expect("identity");
    let color = cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("color");
    ir.model.appearances.push(Appearance {
        native_ref: None,
        id: id.clone(),
        name: Some("Edited material".to_owned()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(color),
        properties: [(
            cadmpeg_core::text::NonBlankString::new("reflectivity").expect("property"),
            FiniteReal::new(0.75).expect("finite"),
        )]
        .into(),
        textures: Vec::new(),
    });
    let record = cadmpeg_ir::NativeRecord::new(cadmpeg_ir::ids::Identity::new("rhino:presentation:material#source").expect("identity"), serde_json::from_value(serde_json::json!({"neutral_ref":id,"name":"Old name","diffuse":{"r":0.0,"g":1.0,"b":0.0,"a":1.0},"reflectivity":0.1,"ambient":{"r":0.0,"g":0.0,"b":0.0,"a":1.0}})).expect("fields")).expect("typed native");
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("materials".to_owned(), vec![record]);
    let authored = materials(&ir).expect("source-free material");
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::writer::tests::table(crate::writer::TCODE_SETTINGS_TABLE, &[])
                .expect("settings"),
            crate::writer::tests::table(TCODE_MATERIAL_TABLE, &authored).expect("table"),
            crate::writer::tests::table(crate::writer::TCODE_OBJECT_TABLE, &[]).expect("objects"),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(&archive, &arena, &DecodePolicy::default())
        .expect("context");
    let scan = crate::container::scan(&ctx, &archive).expect("scan");
    let mut decoded = CadIr::empty();
    let installed = crate::presentation::install(&ctx, view, &scan, &mut decoded)
        .expect("read authored material");
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    let record = &decoded
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["materials"][0];
    assert_eq!(
        record.field("name"),
        Some(Value::String("Edited material".to_owned()))
    );
    assert_eq!(
        record.field("diffuse"),
        Some(
            serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::new([
                255_u8, 0, 0, 0
            ]))
            .expect("native packed color")
        )
    );
    assert_eq!(record.field("reflectivity"), Some(serde_json::json!(0.75)));
}

mod linkage;
