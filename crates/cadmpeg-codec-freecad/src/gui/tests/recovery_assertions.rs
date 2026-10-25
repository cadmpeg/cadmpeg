// SPDX-License-Identifier: Apache-2.0
//! Assertions for bounded GUI source recovery.

use super::*;

/// Decodes one shape whose view provider states `shininess` as its
/// `ShapeMaterial` shininess and checks that the refusal is reported while
/// retaining the GUI source. The invalid material never enters appearance.
pub(super) fn assert_material_shininess_remains_source_only(shininess: &str, expected: &str) {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="A"/></Objects>
<ObjectData Count="1"><Object name="A"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = format!(
        r#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="A"><Properties Count="2">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="287454020"/></Property>
<Property name="ShapeMaterial" type="App::PropertyMaterial"><PropertyMaterial ambientColor="1" diffuseColor="2" specularColor="3" emissiveColor="4" shininess="{shininess}" transparency="0.25"/></Property>
</Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#
    );
    let decoded = FcstdCodec.decode(
        &mut Cursor::new(archive_entries(&[
            ("Document.xml", document),
            ("GuiDocument.xml", gui.as_bytes()),
        ])),
        &DecodeOptions::default(),
    );
    let recovered = decoded.expect("malformed GUI material remains source-only");
    let loss = assert_gui_part_recovery(&recovered, &["freecad.gui_property_value"]);
    assert!(loss.message.contains("shininess"));
    assert!(
        loss.message.contains(if shininess == "nan" {
            "finite"
        } else {
            "invalid"
        }),
        "{expected}"
    );
    assert_eq!(recovered.ir().model.appearances.len(), 1);
}

/// #215 replaces whole-GUI Warning recovery with exact listed source-part Error recovery.
pub(super) fn assert_gui_part_recovery<'a>(
    result: &'a cadmpeg_ir::codec::DecodeResult,
    expected: &[&str],
) -> &'a cadmpeg_ir::report::loss::LossNote {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    let failed = result
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp { .. }))
        .collect::<Vec<_>>();
    let mut actual = failed
        .iter()
        .map(|part| part.level().as_str())
        .collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected, "exact failed parts and cardinality");
    let unreadable = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(
        unreadable.len(),
        failed.len(),
        "one Error for every given-up part"
    );
    for part in &failed {
        let losses = unreadable
            .iter()
            .filter(|loss| {
                loss.provenance.as_ref().is_some_and(|where_| {
                    where_.offset == part.offset()
                        && where_.stream_name() == Some(part.stream())
                        // #217 §5.6 / shared helper: an empty failed part has
                        // no provenance span; positive spans remain exact.
                        && where_.byte_len() == (part.byte_len() != 0).then_some(part.byte_len())
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), 1, "one exact-span Error per failed part");
        assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    }
    crate::fidelity::b1::assert_classified_spans(result);
    let loss = *unreadable.first().expect("bounded GUI failure Error");
    loss
}

pub(super) fn assert_gui_metadata_recovery(
    result: &cadmpeg_ir::codec::DecodeResult,
) -> &cadmpeg_ir::report::loss::LossNote {
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "source.gui-metadata-unresolved")
        .expect("GUI recovery diagnostic");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
    loss
}

pub(super) fn assert_truncated_material_slot(result: &cadmpeg_ir::codec::DecodeResult) {
    let namespace = result.ir().native.namespace("fcstd").unwrap();
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.name() == "ShapeAppearance")
        .unwrap();
    let crate::native::entry_contents::EntryContents::Material { version, values } =
        &entry.contents()[0]
    else {
        panic!("typed material list");
    };
    assert_eq!(*version, 3);
    assert_eq!(values.len(), 1);
    assert!(values[0].fixed.is_none());
    assert!(
        values[0].image.is_none() && values[0].image_path.is_none() && values[0].uuid.is_none()
    );
    assert!(result.ir().model.appearances.is_empty());
    let failed = result
        .source_fidelity()
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.material_list_material")
        .unwrap();
    assert_eq!((failed.offset(), failed.byte_len()), (4, 0));
    let parent = &result.source_fidelity().parts()[failed.parent().unwrap()];
    assert_eq!(parent.level().as_str(), "freecad.material_list_entry");
    assert_eq!((parent.offset(), parent.byte_len()), (0, 4));
    assert!(!matches!(
        parent.state(),
        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
    ));
}
