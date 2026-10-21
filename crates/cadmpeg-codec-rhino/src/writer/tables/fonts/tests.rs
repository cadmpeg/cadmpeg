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
    let authored = styles(&ir).expect("typed font inverse");
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::writer::table(crate::writer::TCODE_SETTINGS_TABLE, &[]).expect("settings"),
            super::super::super::table(TCODE_FONT_TABLE, &authored).unwrap(),
            crate::writer::table(crate::writer::TCODE_OBJECT_TABLE, &[]).expect("objects"),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&archive, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&ctx, &archive).unwrap();
    let mut decoded = CadIr::empty();
    let installed = crate::presentation::install(&ctx, view, &scan, &mut decoded).unwrap();
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    let parsed = &decoded.native.namespace("rhino").unwrap().arenas()["text_styles"][0];
    for field in ["name", "source_uuid", "font_description", "font"] {
        assert_eq!(parsed.field(field).as_ref(), native.get(field), "{field}");
    }
}
