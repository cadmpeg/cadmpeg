// SPDX-License-Identifier: Apache-2.0
//! The original selected ordinal survives sparse native choices and output packing.

use crate::native::GuiPropertyRecord;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn source(selected: usize) -> cadmpeg_ir::codec::DecodeResult {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='App::FeaturePython' name='A'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Selector' type='App::PropertyEnumeration'><Integer value='{selected}' CustomEnum='true'/><CustomEnumList count='3'><Enum value='Fixed'/><Enum/><Enum value='Revolute'/></CustomEnumList></Property></Properties></ViewProvider></ViewProviderData></Document>");
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_slice()),
        ("GuiDocument.xml", gui.as_bytes()),
    ]);
    crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn selected_surviving_enum_is_reindexed_only_on_the_output_clone() {
    let before = source(2);
    let property = before
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<GuiPropertyRecord>("gui_properties")
        .unwrap()
        .remove(0);
    let original = property.clone();
    let output = crate::writer::xml::native_gui(&property).unwrap().unwrap();
    let xml = roxmltree::Document::parse(&output).unwrap();
    assert_eq!(
        xml.descendants()
            .find(|node| node.has_tag_name("Integer"))
            .unwrap()
            .attribute("value"),
        Some("1")
    );
    assert_eq!(
        xml.descendants()
            .find(|node| node.has_tag_name("CustomEnumList"))
            .unwrap()
            .attribute("count"),
        Some("2")
    );
    assert_eq!(
        xml.descendants()
            .filter(|node| node.has_tag_name("Enum"))
            .map(|node| node.attribute("value").unwrap())
            .collect::<Vec<_>>(),
        ["Fixed", "Revolute"]
    );
    assert_eq!(property, original);
    assert_eq!(property.values[0].attributes["value"], "2");
    let choices = property
        .values
        .iter()
        .filter(|value| value.tag == "Enum")
        .map(|value| value.item_index)
        .collect::<Vec<_>>();
    assert_eq!(choices, [Some(0), Some(2)]);
}

#[test]
fn selected_missing_enum_omits_the_property_with_its_actual_owner_loss() {
    let before = source(1);
    let property = before
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<GuiPropertyRecord>("gui_properties")
        .unwrap()
        .remove(0);
    assert!(crate::writer::xml::native_gui(&property).unwrap().is_none());
    let plan = crate::FcstdCodec
        .plan(
            EncodeInput::new(before.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.undecoded-omitted"
            && loss.message.contains(&property.id)));
}

#[test]
fn an_out_of_range_selector_keeps_its_numeric_family_when_choices_are_packed() {
    let before = source(5);
    let property = before
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<GuiPropertyRecord>("gui_properties")
        .unwrap()
        .remove(0);
    let original = property.clone();
    let output = crate::writer::xml::native_gui(&property).unwrap().unwrap();
    let xml = roxmltree::Document::parse(&output).unwrap();
    assert_eq!(
        xml.descendants()
            .find(|node| node.has_tag_name("Integer"))
            .unwrap()
            .attribute("value"),
        Some("5")
    );
    assert_eq!(
        xml.descendants()
            .find(|node| node.has_tag_name("CustomEnumList"))
            .unwrap()
            .attribute("count"),
        Some("2")
    );
    assert_eq!(property, original);
}
