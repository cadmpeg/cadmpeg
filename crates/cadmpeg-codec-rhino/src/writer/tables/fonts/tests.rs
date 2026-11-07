// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::writer::TCODE_FONT_TABLE;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn source_free_text_style_retains_font_names_weight_and_panose() {
    let mut ir = CadIr::empty();
    let native = serde_json::json!({
        "name":"Engineering text", "source_uuid":"d74b5273-c7e0-4ad5-a431-8e1db48265cb",
        "font_description":"Authored font description",
        "font":{
            "characteristics":4, "windows_logfont_name":"Example Face", "postscript_name":"ExampleFace-Regular",
            "obsolete_description":"Legacy label", "windows_logfont_weight":500,"apple_weight_trait":0.25,
            "point_size":11.0,"family_name":"Example Family", "locale_name":"en-US", "localized_postscript_name":"ExampleFace-Localized",
            "english_postscript_name":"ExampleFace-Regular", "localized_logfont_name":"Example Face", "english_logfont_name":"Example Face",
            "localized_family_name":"Example Family", "english_family_name":"Example Family", "localized_face_name":"Regular", "english_face_name":"Regular",
            "panose":{"family_type":2,"serif_style":11,"weight":5,"proportion":3,"contrast":2,"stroke_variation":2,"arm_style":2,"letterform":2,"midline":2,"x_height":4},
            "quartet_member":1
        }
    });
    let record = cadmpeg_ir::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new("rhino:presentation:text_style#authored").unwrap(),
        serde_json::from_value(native.clone()).unwrap(),
    )
    .unwrap();
    assert!(complete(&record));
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("text_styles".to_owned(), vec![record]);
    let authored = styles(&ir).expect("typed font inverse").records;
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::writer::tests::table(crate::writer::TCODE_SETTINGS_TABLE, &[])
                .expect("settings"),
            crate::writer::tests::table(TCODE_FONT_TABLE, &authored).unwrap(),
            crate::writer::tests::table(crate::writer::TCODE_OBJECT_TABLE, &[]).expect("objects"),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&archive, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&ctx, &archive).unwrap();
    let mut decoded = CadIr::empty();
    let (installed, _) = crate::presentation::install(&ctx, view, &scan, &mut decoded).unwrap();
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    let parsed = &decoded.native.namespace("rhino").unwrap().arenas()["text_styles"][0];
    for field in ["name", "source_uuid", "font_description", "font"] {
        assert_eq!(parsed.field(field).as_ref(), native.get(field), "{field}");
    }
}

#[test]
fn malformed_style_is_named_and_valid_style_keeps_references() {
    let mut ir = CadIr::empty();
    let make = |id: &str, fields: Value| {
        cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id).unwrap(),
            serde_json::from_value(fields).unwrap(),
        )
        .unwrap()
    };
    let bad = "rhino:presentation:text_style#bad";
    let good = "rhino:presentation:text_style#good";
    ir.native.namespace_mut("rhino").arenas_mut().insert("text_styles".into(), vec![
        make(bad, serde_json::json!({"archive_index": 0, "name":"bad", "font":null})),
        make(good, serde_json::json!({"archive_index": 7, "name":"good", "font": {"windows_logfont_name":"Face"}})),
    ]);
    let output = styles(&ir).unwrap();
    assert_eq!(output.records.len(), 1);
    assert_eq!(output.indices.get(good), Some(&0));
    assert!(output.losses.iter().any(|loss| loss.code
        == RhinoLossCode::WriterTextStyleOmitted.kind()
        && loss.message.contains(bad)));
    assert_eq!(style_index(&ir, 7).unwrap(), Some(0));
    assert_eq!(style_index(&ir, 0).unwrap(), None);
}

#[test]
fn document_current_font_follows_emitted_style_index() {
    let mut ir = CadIr::empty();
    let make = |id: &str, fields: Value| {
        cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id).unwrap(),
            serde_json::from_value(fields).unwrap(),
        )
        .unwrap()
    };
    let setting = "rhino:document:setting#font";
    ir.native.namespace_mut("rhino").arenas_mut().insert("text_styles".into(), vec![
        make("rhino:presentation:text_style#valid", serde_json::json!({"archive_index":7,"name":"valid","font":{"windows_logfont_name":"Face"}})),
    ]);
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "document_settings".into(),
        vec![make(setting, serde_json::json!({"current_font_index":7}))],
    );
    let materials = super::super::materials::MaterialPlan::new(&ir).unwrap();
    let mut losses = Vec::new();
    let settings = super::super::document::settings(&ir, &materials, &mut losses).unwrap();
    assert!(settings
        .records
        .contains(&crate::writer::short_chunk(0xa000_0132, 0)));
    assert!(losses.iter().any(|loss| loss.message.contains(setting)));
}
