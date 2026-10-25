// SPDX-License-Identifier: Apache-2.0
//! Source scheduling is independent of ZIP encoding and requested semantic depth.

use cadmpeg_core::decode::{DecodeDepth, InspectOptions};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

const DOCUMENT: &[u8] = b"<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='Bad' type='App::PropertyInteger'><Integer value='x'/></Property></Properties><Objects Count='1'><Object name='Owner' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='1'><Property name='Peer' type='App::PropertyString'><String value='independent'/></Property></Properties></Object></ObjectData></Document>";
const GUI: &[u8] = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Owner'><Properties Count='2'><Property name='Bad' type='App::PropertyInteger'><Integer value='x'/></Property><Property name='Peer' type='App::PropertyString'><String value='GUI peer'/></Property></Properties></ViewProvider></ViewProviderData></Document>";

fn fixture(streaming: bool, large: bool, method: zip::CompressionMethod) -> Vec<u8> {
    let options = SimpleFileOptions::default()
        .large_file(large)
        .compression_method(method);
    let entries = [("Document.xml", DOCUMENT), ("GuiDocument.xml", GUI)];
    if streaming {
        let mut writer = zip::ZipWriter::new_stream(Vec::new());
        for (name, bytes) in entries {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    } else {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
}

#[test]
fn xml_schedule_and_failure_homes_agree_across_zip_forms_and_depths() {
    for streaming in [false, true] {
        for large in [false, true] {
            for method in [
                zip::CompressionMethod::Stored,
                zip::CompressionMethod::Deflated,
            ] {
                let bytes = fixture(streaming, large, method);
                for depth in [
                    DecodeDepth::Container,
                    DecodeDepth::Geometry,
                    DecodeDepth::Full,
                ] {
                    let decoded = crate::FcstdCodec
                        .decode(
                            &mut Cursor::new(&bytes),
                            &DecodeOptions {
                                depth,
                                ..DecodeOptions::default()
                            },
                        )
                        .unwrap();
                    crate::test_support::framing::assert_xml_parts_agree(&bytes, &decoded);
                    let failures = decoded
                        .source_fidelity()
                        .parts()
                        .values()
                        .filter(|part| part.state() == &(PartState::GivenUp {}))
                        .collect::<Vec<_>>();
                    assert_eq!(
                        failures.len(),
                        2,
                        "{streaming} {large} {method:?} {depth:?}"
                    );
                    for (level, text) in [
                        ("freecad.property_value", DOCUMENT),
                        ("freecad.gui_property_value", GUI),
                    ] {
                        let part = failures
                            .iter()
                            .find(|part| part.level().as_str() == level)
                            .unwrap();
                        let literal = b"<Integer value='x'/>";
                        let offset = text
                            .windows(literal.len())
                            .position(|bytes| bytes == literal)
                            .unwrap();
                        assert_eq!(part.offset(), u64::try_from(offset).unwrap());
                        assert_eq!(part.byte_len(), u64::try_from(literal.len()).unwrap());
                        let start = u64::try_from(offset).unwrap();
                        let end = start + u64::try_from(literal.len()).unwrap();
                        assert_eq!(part.framing_bytes(), &[start..start + 8, end - 2..end]);
                    }
                    assert_eq!(
                        decoded
                            .report()
                            .losses
                            .iter()
                            .filter(|loss| loss.code.local_code() == "record.unreadable")
                            .count(),
                        2
                    );
                    assert_eq!(
                        crate::source_archive::image(decoded.source_fidelity()).unwrap(),
                        bytes
                    );
                    assert_eq!(decoded.ir().scope.model, depth != DecodeDepth::Container);
                    assert_eq!(decoded.ir().scope.design, depth == DecodeDepth::Full);
                    crate::test_support::test_archive::assert_valid_document(decoded.ir());
                    let findings = crate::test_support::validate_native(decoded.ir());
                    assert!(findings.is_empty(), "{depth:?}: {findings:?}");
                    if depth == DecodeDepth::Full {
                        let inspected = crate::FcstdCodec
                            .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
                            .unwrap();
                        assert_eq!(inspected.losses, decoded.report().losses);
                        assert_eq!(inspected.notes, decoded.report().notes);
                        assert_eq!(inspected.entries.len(), 2);
                    }
                }
            }
        }
    }
}
