// SPDX-License-Identifier: Apache-2.0
//! ShapeAppearance ambiguity through archive decode and schema admission.

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::{Codec, DecodeFailure, DecodeOptions};

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;

fn ambiguous_material_archive(schema: &str) -> Vec<u8> {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1">
<Property name="Shape" type="Part::PropertyPartShape">
<Part ElementMap="1.0" file="Shape.brp"/>
<ElementMap new="1" count="0"/>
<ElementMap2 count="0">1 PostfixCount 0 MapCount 1 ElementMap 1 1 2 Face ChildCount 0 NameCount 0 Face ChildCount 0 NameCount 0 EndMap</ElementMap2>
</Property></Properties></Object></ObjectData></Document>"#;
    let gui = format!(
        r#"<Document SchemaVersion="{schema}"><ViewProviderData Count="1">
<ViewProvider name="Shape"><Properties Count="1"><Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="materials.bin" version="2"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#
    );
    let mut materials = 1_u32.to_le_bytes().to_vec();
    materials.extend([0_u8; 24]);
    archive_entries(&[
        ("Document.xml", document),
        ("GuiDocument.xml", gui.as_bytes()),
        ("Shape.brp", &[]),
        ("materials.bin", &materials),
    ])
}

#[test]
fn schema_one_archive_rejects_single_material_face_ambiguity() {
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(ambiguous_material_archive("1")),
            &DecodeOptions::default(),
        )
        .expect_err("single-material ambiguity");
    assert!(
        matches!(error, DecodeFailure::Codec(CodecError::Malformed(message))
        if message == "Shape property fcstd:native:property#Shape:Shape has multiple Face groups")
    );
}

#[test]
fn foreign_schema_archive_discards_ambiguous_material_plan() {
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(ambiguous_material_archive("2")),
            &DecodeOptions::default(),
        )
        .expect("foreign schema degrades after ambiguity");
    assert!(result.ir().model.appearances.is_empty());
    assert!(result.ir().model.appearance_bindings.is_empty());
    assert!(result.ir().model.presentation_documents.is_empty());
    assert!(result.ir().model.view_presentations.is_empty());
    let losses: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "source.gui-schema-unverified")
        .collect();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("has multiple Face groups"));
}
