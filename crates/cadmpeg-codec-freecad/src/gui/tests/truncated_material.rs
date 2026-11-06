// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn truncated_gui_material_list_keeps_the_readable_property() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="ShapeAppearance" version="3"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let recovered = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("ShapeAppearance", &1_u32.to_le_bytes()),
            ])),
            &DecodeOptions::default(),
        )
        .expect("independent persistence survives malformed GUI metadata");
    let properties = recovered
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    let material = properties
        .iter()
        .find(|property| property.name == "ShapeAppearance")
        .unwrap();
    assert!(!material.values.is_empty());
    assert!(!material.side_entries.is_empty());
    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"
            && loss.message.contains("freecad.material_list_material")));
    assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
}
