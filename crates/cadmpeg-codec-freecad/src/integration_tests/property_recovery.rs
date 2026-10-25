// SPDX-License-Identifier: Apache-2.0
//! A duplicate property does not discard an independently readable sibling.

use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn duplicate_document_and_object_properties_keep_their_readable_peer() {
    let properties = r#"<Properties Count="3"><Property name="Duplicate" type="App::PropertyString"><String value="first"/></Property><Property name="Peer" type="App::PropertyString"><String value="survives"/></Property><Property name="Duplicate" type="App::PropertyString"><String value="second"/></Property></Properties>"#;
    for document_owned in [true, false] {
        let root = if document_owned { properties } else { "" };
        let object = if document_owned { "" } else { properties };
        let document = format!(
            r#"<Document SchemaVersion="4">{root}<Objects Count="1"><Object name="Owner" type="App::Feature"/></Objects><ObjectData Count="1"><Object name="Owner">{object}</Object></ObjectData></Document>"#
        );
        let decoded = crate::FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(&document)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = decoded.ir().native.namespace("fcstd").unwrap();
        let properties = native
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        assert_eq!(properties.len(), 1);
        let peer = &properties[0];
        assert_eq!(peer.name, "Peer");
        assert_eq!(
            peer.owner,
            crate::native::native_id(
                if document_owned { "document" } else { "object" },
                if document_owned { "0" } else { "Owner" },
            )
        );
        assert_eq!(peer.values()[0].attributes["value"], "survives");
        let fidelity = decoded.source_fidelity();
        let failures = fidelity
            .parts()
            .values()
            .filter(|part| {
                part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            })
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 2);
        for part in failures {
            assert_eq!(part.level().as_str(), "freecad.property");
            let parent = &fidelity.parts()[part.parent().unwrap()];
            assert_eq!(parent.level().as_str(), "freecad.properties");
            assert_eq!(
                parent.state(),
                &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
            );
        }
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code.local_code() == "record.unreadable"
                        && loss.severity == cadmpeg_ir::report::Severity::Error
                        && loss.message.contains("duplicate property name Duplicate")
                        && loss.message.contains(if document_owned {
                            "for document"
                        } else {
                            "for object"
                        })
                })
                .count(),
            2
        );
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
        crate::test_support::test_archive::assert_valid_document(decoded.ir());
    }
}
