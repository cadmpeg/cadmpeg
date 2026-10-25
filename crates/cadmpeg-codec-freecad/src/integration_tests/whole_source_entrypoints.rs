// SPDX-License-Identifier: Apache-2.0
//! Inspection and decoding share the complete source schedule.

use crate::test_support::test_archive::broken_optional_member_archive as archive;

#[test]
fn container_depth_keeps_actual_gui_grammar_native_rows_and_failure_homes() {
    use cadmpeg_core::decode::DecodeDepth;
    use cadmpeg_ir::source_fidelity::parts::PartState;
    use cadmpeg_ir::{Codec, ContentScope, DecodeOptions};
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Owner'><Properties Count='2'><Property name='Bad' type='App::PropertyInteger'><Integer value='x'/></Property><Property name='Peer' type='App::PropertyString'><String value='independent'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("GuiDocument.xml", gui),
    ]);
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(&bytes),
            &DecodeOptions {
                depth: DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(decoded.ir().scope, ContentScope::container());
    assert_eq!(decoded.ir().model.entity_count(), 0);
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    assert_eq!(
        native
            .arena_as::<crate::native::GuiDocumentRecord>("gui_documents")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        native
            .arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")
            .unwrap()
            .len(),
        1
    );
    let properties = native
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    assert_eq!(properties.len(), 2);
    let peer = properties
        .iter()
        .find(|property| property.name == "Peer")
        .unwrap();
    assert_eq!(peer.values[0].attributes["value"], "independent");
    assert!(properties
        .iter()
        .find(|property| property.name == "Bad")
        .unwrap()
        .values
        .is_empty());
    let layout = &decoded.source_fidelity().record_layout()[&peer.id];
    assert_eq!(
        layout.stream(),
        decoded
            .source_fidelity()
            .parts()
            .values()
            .find(|part| part.level().as_str() == "freecad.gui_document_xml")
            .unwrap()
            .stream()
    );
    let failures = decoded
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect::<Vec<_>>();
    let [failure] = failures.as_slice() else {
        panic!("one genuine GUI leaf refusal")
    };
    assert_eq!(failure.level().as_str(), "freecad.gui_property_value");
    assert_eq!(
        failure.offset(),
        u64::try_from(
            gui.windows(b"<Integer".len())
                .position(|bytes| bytes == b"<Integer")
                .unwrap()
        )
        .unwrap()
    );
    assert!(!decoded
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.opaque_side_entry"));
    crate::test_support::framing::assert_xml_parts_agree(&bytes, &decoded);
    let findings = crate::test_support::validate_native(decoded.ir());
    assert!(findings.is_empty(), "{findings:?}");
    crate::test_support::test_archive::assert_valid_document(decoded.ir());
}

#[test]
fn independent_xml_framing_matches_all_frozen_golden_inputs() {
    use cadmpeg_ir::Codec;
    use cadmpeg_test_support::golden::Harness;
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures");
    let inputs = Harness::new(env!("CARGO_MANIFEST_DIR"), "FCStd", "")
        .with_fixture_dir(fixtures)
        .fixture_inputs();
    assert!(!inputs.is_empty());
    for (name, bytes) in inputs {
        let decoded = crate::FcstdCodec
            .decode(
                &mut std::io::Cursor::new(&bytes),
                &cadmpeg_ir::DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        crate::test_support::framing::assert_xml_parts_agree(&bytes, &decoded);
    }
}

#[test]
fn inspect_runs_the_same_whole_source_schedule_and_reports_the_same_failures() {
    use cadmpeg_ir::Codec;
    let bytes = archive();
    let inspected = crate::FcstdCodec
        .inspect(
            &mut std::io::Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    let decoded = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(&bytes),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(inspected.losses, decoded.report().losses);
    assert_eq!(inspected.notes, decoded.report().notes);
    assert_eq!(inspected.entries.len(), 3);
    assert_eq!(
        inspected
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(decoded
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.opaque_side_entry"));
}

#[test]
fn a_failed_document_member_preserves_independent_gui_grammar_and_presentation() {
    use cadmpeg_core::decode::DecodeDepth;
    use cadmpeg_ir::{Codec, ContentScope};
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='0'/></Document>";
    let bytes = zip_fixture::archive(&[
        RawEntry::compressed("Document.xml", zip_fixture::DEFLATE, b"\x07", b"unexpanded"),
        RawEntry::stored("GuiDocument.xml", gui),
    ]);
    for depth in [
        DecodeDepth::Full,
        DecodeDepth::Geometry,
        DecodeDepth::Container,
    ] {
        let decoded = crate::FcstdCodec
            .decode(
                &mut std::io::Cursor::new(&bytes),
                &cadmpeg_ir::DecodeOptions {
                    depth,
                    ..cadmpeg_ir::DecodeOptions::default()
                },
            )
            .unwrap();
        let expected = if depth == DecodeDepth::Container {
            ContentScope::container()
        } else {
            ContentScope::geometry()
        };
        assert_eq!(decoded.ir().scope, expected);
        assert_eq!(
            decoded.ir().model.presentation_documents.len(),
            usize::from(depth != DecodeDepth::Container)
        );
        assert_eq!(
            decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::GuiDocumentRecord>("gui_documents")
                .unwrap()
                .len(),
            1
        );
        let parts = decoded.source_fidelity().parts();
        assert!(!parts
            .values()
            .any(|part| part.level().as_str() == "freecad.document_xml"));
        assert!(!parts
            .values()
            .any(|part| part.level().as_str() == "freecad.opaque_side_entry"));
        let gui_part = parts
            .values()
            .find(|part| part.level().as_str() == "freecad.gui_document_xml")
            .unwrap();
        assert_eq!(
            gui_part.state(),
            &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
        );
        assert_eq!(
            parts[gui_part.parent().unwrap()].level().as_str(),
            "zip.entry"
        );
        let failures = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect::<Vec<_>>();
        let [loss] = failures.as_slice() else {
            panic!("one failed Document member")
        };
        assert_eq!(loss.provenance.as_ref().unwrap().byte_len(), Some(1));
        crate::test_support::test_archive::assert_valid_document(decoded.ir());
    }
}
