// SPDX-License-Identifier: Apache-2.0
//! GUI metadata recovery keeps geometry and a native GUI document carrier.

use super::*;
use crate::native::GuiDocumentRecord;
use crate::test_support::test_archive::{assert_valid_document, rewrite_entry, GEOMETRY};

#[test]
fn malformed_gui_metadata_keeps_geometry_and_valid_native_state() {
    let expected = FcstdCodec
        .decode(&mut Cursor::new(GEOMETRY), &DecodeOptions::default())
        .unwrap();
    for gui in [
        b"<Document SchemaVersion=\"1\"><ViewProviderData Count=\"0\"/></Document>".as_slice(),
        b"<Document SchemaVersion=\"1\"><Camera settings=\"\"/><Camera settings=\"\"/></Document>",
        b"<Document SchemaVersion=\"99\"><ViewProviderData Count=\"2\"/></Document>",
        b"<Document SchemaVersion=\"1\"><Camera/></Document>",
        b"<Document",
        b"\xff",
    ] {
        let source = rewrite_entry(GEOMETRY, "GuiDocument.xml", |_| gui.to_vec());
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        // Presentation is owned by GUI metadata; geometric entities are independent.
        assert_eq!(recovered.ir().model.faces, expected.ir().model.faces);
        let without_style = |bodies: &[cadmpeg_ir::topology::Body]| {
            let mut bodies = bodies.to_vec();
            for body in &mut bodies {
                body.visible = None;
                body.color = None;
            }
            bodies
        };
        assert_eq!(
            without_style(&recovered.ir().model.bodies),
            without_style(&expected.ir().model.bodies)
        );
        assert_eq!(recovered.ir().model.surfaces, expected.ir().model.surfaces);
        assert_valid_document(recovered.ir());
        assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
        let documents = recovered
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<GuiDocumentRecord>("gui_documents")
            .unwrap();
        assert_eq!(documents.len(), 1);
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "source.gui-metadata-unresolved"));
    }
}

#[test]
fn gui_metadata_recovery_preserves_resource_refusals() {
    crate::test_support::assert_retained_refusal_at(&[], "FCStd GUI metadata diagnostic", |ctx| {
        super::super::source_only_graph(
            ctx,
            None,
            &cadmpeg_core::CodecError::Malformed("unreadable".into()),
        )
    });
    crate::test_support::assert_collection_refusal_at(&[], "FCStd GUI metadata losses", |ctx| {
        super::super::source_only_graph(
            ctx,
            None,
            &cadmpeg_core::CodecError::Malformed("unreadable".into()),
        )
    });
}
