// SPDX-License-Identifier: Apache-2.0

use super::*;

/// A GUI property whose key is blank names nothing, so its value cannot be
/// keyed. The property is charged by name against its own record and the rest
/// of the GUI presentation graph survives: a blank key is one unreadable
/// property, not a reason to answer no presentation at all.
#[test]
fn a_blank_gui_property_key_is_charged_and_the_presentation_graph_survives() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1">
 <ViewProviderData Count="1">
  <ViewProvider name="Model">
   <Properties Count="2">
    <Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="   " type="App::PropertyBool"><Bool value="false"/></Property>
   </Properties>
  </ViewProvider>
 </ViewProviderData>
 <Camera settings="OrthographicCamera { position 1 2 3 orientation 0 0 1 0 }"/>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("a blank property key does not refuse the GUI document");

    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "source.gui-property-key-blank")
        .expect("the blank key is charged");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
    assert!(
        loss.message.contains("states a property with a blank key"),
        "{}",
        loss.message
    );
    assert!(
        !result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "source.gui-schema-unverified"),
        "a blank key is not a schema declaration problem"
    );

    // The rest of the GUI presentation graph is still there.
    assert_eq!(result.ir().model.presentation_documents.len(), 1);
    let view = result
        .ir()
        .model
        .view_presentations
        .first()
        .expect("the provider's view presentation survives");
    assert_eq!(view.visible, Some(true));
    let native = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("GUI native namespace")
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert!(native
        .iter()
        .any(|property| property.name == "Visibility" && !property.values.is_empty()));
    assert!(
        !view.properties.contains_key("Visibility"),
        "the visibility field is canonical"
    );
}
