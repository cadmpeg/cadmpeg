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
        let unresolved = recovered
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "source.gui-metadata-unresolved")
            .count();
        // An explicitly empty schema-1 provider table is a complete GUI fact.
        if gui == b"<Document SchemaVersion=\"1\"><ViewProviderData Count=\"0\"/></Document>" {
            assert_eq!(unresolved, 0);
            assert!(documents[0].states.is_empty());
            assert!(recovered.ir().model.view_presentations.is_empty());
        } else {
            assert_eq!(unresolved, 1, "{:?}", recovered.report().losses);
        }
    }
}
