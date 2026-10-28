// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::CadIr;
use serde::Serialize;

#[derive(Serialize)]
struct TextRecord {
    id: &'static str,
    text: NativeBytes<[u8; 2]>,
}

#[derive(Serialize)]
struct MacroDefinition {
    id: &'static str,
    defined_entity_type: i64,
}

#[test]
fn exact_native_text_bytes_stay_typed_and_macro_definitions_keep_their_disposition() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut ir = CadIr::empty();
    assert!(super::untyped_records(&ir).is_empty());
    ir.native
        .namespace_mut("iges")
        .set_arena_from(
            &ctx,
            "annotations",
            [TextRecord {
                id: "iges:native:annotation#D3",
                text: NativeBytes::from([0xff, 0xfe]),
            }],
        )
        .unwrap();
    assert!(super::untyped_records(&ir).is_empty());
    ir.native
        .namespace_mut("iges")
        .set_arena_from(
            &ctx,
            "macro_definitions",
            [
                MacroDefinition {
                    id: "iges:native:macro-definition#D9",
                    defined_entity_type: 621,
                },
                MacroDefinition {
                    id: "iges:native:macro-definition#D1",
                    defined_entity_type: 621,
                },
            ],
        )
        .unwrap();
    let before = ir.native.clone();
    assert_eq!(
        super::untyped_records(&ir),
        [
            "iges:native:macro-definition#D1",
            "iges:native:macro-definition#D9",
        ]
    );
    assert_eq!(ir.native, before);
    let text = &ir.native.namespace("iges").unwrap().arenas()["annotations"][0];
    assert_eq!(text.field("text"), Some(serde_json::json!("fffe")));
}
