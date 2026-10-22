// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::test_archive::rewrite_entry;
use cadmpeg_ir::appearance::AppearanceTarget;

const GUI_MODEL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
));

#[test]
fn face_materials_follow_shape_indices_without_complete_persistent_names() {
    for names in [
        "",
        "<ElementMap/>",
        r#"<ElementMap count="2"><Element key="named-first" value="Face1"/><Element key="named-third" value="Face3"/></ElementMap>"#,
    ] {
        let source = rewrite_entry(GUI_MODEL, "Document.xml", |bytes| {
            std::str::from_utf8(bytes)
                .unwrap()
                .replace("<ElementMap/>", names)
                .into_bytes()
        });
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        assert!(
            decoded.report().losses.is_empty(),
            "{:?}",
            decoded.report().losses
        );
        let model = &decoded.ir().model;
        assert_eq!(model.faces.len(), 8);
        let face_bindings = model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .collect::<Vec<_>>();
        assert_eq!(face_bindings.len(), 8);
        for face in &model.faces {
            assert_eq!(
                face_bindings
                    .iter()
                    .filter(|binding| { binding.target == AppearanceTarget::Face(face.id.clone()) })
                    .count(),
                1
            );
        }
        assert_valid_document(decoded.ir());
    }
}
