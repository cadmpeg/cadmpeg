// SPDX-License-Identifier: Apache-2.0
//! Exact independent XML part comparisons for public archive decodes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_test_support::framing::{freecad, zip, FrameParent, PartTable};
use std::collections::BTreeSet;

pub(crate) fn zip_forms(bytes: &[u8]) -> Vec<Vec<u8>> {
    use ::zip::write::SimpleFileOptions;
    use std::io::{Cursor, Write};

    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let entries = zip::frame(&ctx, root, &mut Vec::new()).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let members = entries
        .iter()
        .map(|entry| {
            let name = std::str::from_utf8(entry.name().window()).unwrap();
            (name, archive.open(&ctx, name).unwrap())
        })
        .collect::<Vec<_>>();
    let mut forms = Vec::new();
    for streaming in [false, true] {
        for large in [false, true] {
            for method in [
                ::zip::CompressionMethod::Stored,
                ::zip::CompressionMethod::Deflated,
            ] {
                let options = SimpleFileOptions::default()
                    .large_file(large)
                    .compression_method(method);
                let output = if streaming {
                    let mut writer = ::zip::ZipWriter::new_stream(Vec::new());
                    for (name, source) in &members {
                        writer.start_file(*name, options).unwrap();
                        writer.write_all(source.window()).unwrap();
                    }
                    writer.finish().unwrap().into_inner()
                } else {
                    let mut writer = ::zip::ZipWriter::new(Cursor::new(Vec::new()));
                    for (name, source) in &members {
                        writer.start_file(*name, options).unwrap();
                        writer.write_all(source.window()).unwrap();
                    }
                    writer.finish().unwrap().into_inner()
                };
                forms.push(output);
            }
        }
    }
    forms
}

pub(crate) fn assert_xml_parts_agree(bytes: &[u8], decoded: &DecodeResult) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut xml_frames = Vec::new();
    for entry in &entries {
        if !matches!(entry.name().window(), b"Document.xml" | b"GuiDocument.xml") {
            continue;
        }
        let name = std::str::from_utf8(entry.name().window()).unwrap();
        let member = archive.open(&ctx, name).unwrap();
        let start = frames.len();
        freecad::frame_xml_member(&ctx, member, entry.name(), entry.frame_index(), &mut frames)
            .unwrap();
        xml_frames.extend(start..frames.len());
    }
    PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
    let parts = decoded.source_fidelity().parts();
    let mut levels = BTreeSet::new();
    let mut matched = BTreeSet::new();
    for index in xml_frames {
        let frame = &frames[index];
        levels.insert(frame.level().as_str());
        let (id, part) = parts
            .iter()
            .find(|(_, part)| {
                part.level() == frame.level()
                    && part.stream() == frame.capture().stream()
                    && part.offset() == frame.capture().offset()
                    && part.byte_len() == frame.capture().byte_len()
            })
            .unwrap_or_else(|| panic!("missing independent XML frame: {frame:?}"));
        assert!(matched.insert(id.clone()), "duplicate XML frame");
        assert_eq!(
            part.framing_bytes(),
            frame.framing(),
            "{}",
            part.level().as_str()
        );
        if let FrameParent::Part(parent) = frame.parent() {
            let expected = &frames[parent];
            let actual = &parts[part.parent().expect("XML part has its physical parent")];
            assert_eq!(actual.level(), expected.level());
            assert_eq!(actual.stream(), expected.capture().stream());
            assert_eq!(actual.offset(), expected.capture().offset());
            assert_eq!(actual.byte_len(), expected.capture().byte_len());
        }
    }
    for (id, part) in parts
        .iter()
        .filter(|(_, part)| levels.contains(part.level().as_str()))
    {
        assert!(matched.contains(id), "decoder-only XML part: {part:?}");
    }
}
