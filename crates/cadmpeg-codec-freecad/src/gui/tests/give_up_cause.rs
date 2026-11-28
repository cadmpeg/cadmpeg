// SPDX-License-Identifier: Apache-2.0
//! The cause of each `GuiDocument.xml` give-up and its pinned admission.

use super::*;

/// A GUI document of schema 1 for one view provider of `Model` whose one
/// property is `property`.
fn gui_with_property(property: &str) -> String {
    format!(
        r#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1">{property}</Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#
    )
}

/// A material list of a version the decoder has no reading for gives up
/// `GuiDocument.xml` as one `record.undecoded`, which a pinned decode
/// accepts.
#[test]
fn an_unknown_material_list_version_gives_up_the_gui_document_as_undecoded() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = gui_with_property(
        r#"<Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="ShapeAppearance" version="4"/></Property>"#,
    );
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui.as_bytes()),
                ("ShapeAppearance", &0_u32.to_le_bytes()),
            ])),
            &options,
        )
        .expect("a pinned decode accepts an undecoded GUI document");
    let loss = assert_gui_metadata_recovery(&decoded);
    assert_eq!(
        loss.code,
        cadmpeg_ir::unknown::part::record_undecoded_kind()
    );
    assert!(
        loss.message.contains("material-list version 4"),
        "{}",
        loss.message
    );
}

/// A `GuiDocument.xml` that is not UTF-8 breaks the XML grammar: it gives
/// up as one `record.unreadable`, which a pinned decode refuses.
#[test]
fn a_gui_document_that_is_not_utf8_gives_up_as_unreadable() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let source = archive_entries(&[("Document.xml", document), ("GuiDocument.xml", b"\xff")]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("the persistence document decodes");
    let loss = assert_gui_metadata_recovery(&decoded);
    assert_eq!(
        loss.code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let error = FcstdCodec
        .decode(&mut Cursor::new(&source), &options)
        .expect_err("a pinned decode refuses an unreadable GUI document");
    let cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } = &error else {
        panic!("a strict refusal is a policy class: {error:?}");
    };
    assert_eq!(
        rejection.loss().code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
}

/// A GUI document of an unverified schema that the schema-1 reading does
/// not read gives up as one `record.undecoded`.
#[test]
fn an_unverified_gui_schema_that_does_not_read_gives_up_as_undecoded() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui = br#"<Document SchemaVersion="99"><ViewProviderData Count="2"/></Document>"#;
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &options,
        )
        .expect("a pinned decode accepts an undecoded GUI document");
    let loss = gui_document_given_up(&decoded).expect("the GUI document is given up");
    assert_eq!(
        loss.code,
        cadmpeg_ir::unknown::part::record_undecoded_kind()
    );
}

/// The XML reader admits a wide tag within the session budget and retains
/// its root attributes in the native GUI document.
#[test]
fn a_gui_document_with_many_attributes_retains_them() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let attributes = (0..300)
        .map(|index| [" a", &index.to_string(), "=\"0\""].concat())
        .collect::<String>();
    let gui =
        format!(r#"<Document SchemaVersion="1"{attributes}><Camera settings=""/></Document>"#);
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui.as_bytes()),
            ])),
            &options,
        )
        .expect("a pinned decode accepts a wide tag within the session budget");
    assert!(gui_document_given_up(&decoded).is_none());
    let native = decoded.ir().native.namespace("fcstd").expect("native");
    let documents = native
        .arena_as::<crate::native::GuiDocumentRecord>("gui_documents")
        .expect("GUI documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].attributes.len(), 301);
    assert_eq!(documents[0].attributes["SchemaVersion"], "1");
    assert_eq!(documents[0].attributes["a299"], "0");
}
