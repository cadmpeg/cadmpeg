// SPDX-License-Identifier: Apache-2.0
//! Opaque member framing and content isolation across all ZIP forms and depths.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::{PartLevel, PartState};
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::comparison::{assert_decodes_agree, Allowances, Mode};
use cadmpeg_test_support::framing::{zip, FrameParent, FramedPart, PartTable};
use std::io::Cursor;

#[test]
fn opaque_member_content_is_independent_of_document_records_at_every_depth_and_zip_form() {
    let document =
        b"<Document SchemaVersion='4'><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let original = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("first.bin", b"first opaque member"),
        ("middle.bin", b"middle opaque member"),
        ("last.bin", b"last opaque member"),
    ]);
    for bytes in crate::test_support::framing::zip_forms(&original) {
        for depth in [
            DecodeDepth::Container,
            DecodeDepth::Geometry,
            DecodeDepth::Full,
        ] {
            let options = DecodeOptions {
                depth,
                ..DecodeOptions::default()
            };
            let before = crate::FcstdCodec
                .decode(&mut Cursor::new(&bytes), &options)
                .unwrap();
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
            let root = ctx.name_stream(root, "file").unwrap();
            let mut frames = Vec::new();
            let entries = zip::frame(&ctx, root, &mut frames).unwrap();
            let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
            for name in ["first.bin", "middle.bin", "last.bin"] {
                let entry = entries
                    .iter()
                    .find(|entry| entry.name().window() == name.as_bytes())
                    .unwrap();
                let member = archive.open(&ctx, name).unwrap();
                let frame = FramedPart::new(
                    &ctx,
                    PartLevel::parse("freecad.opaque_side_entry").unwrap(),
                    member,
                    FrameParent::Part(entry.frame_index()),
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap();
                let capture = frame.capture();
                let parts = before.source_fidelity().parts();
                let matching = parts
                    .values()
                    .filter(|part| {
                        part.level() == frame.level()
                            && part.stream() == capture.stream()
                            && part.offset() == capture.offset()
                            && part.byte_len() == capture.byte_len()
                    })
                    .collect::<Vec<_>>();
                let [part] = matching.as_slice() else {
                    panic!("opaque member must have one exact part: {matching:?}")
                };
                assert_eq!(part.state(), &(PartState::Decoded {}));
                assert!(part.framing_bytes().is_empty());
                let parent = &parts[part.parent().unwrap()];
                let expected_parent = &frames[entry.frame_index()];
                assert_eq!(parent.level(), expected_parent.level());
                assert_eq!(parent.stream(), expected_parent.capture().stream());
                assert_eq!(parent.offset(), expected_parent.capture().offset());
                assert_eq!(parent.byte_len(), expected_parent.capture().byte_len());
                frames.push(frame);
                for fill in [0, 0xff] {
                    let changed = zip::rebuild_member(
                        &ctx,
                        root,
                        name.as_bytes(),
                        &vec![fill; member.window().len()],
                    )
                    .unwrap();
                    let after = crate::FcstdCodec
                        .decode(&mut Cursor::new(changed), &options)
                        .unwrap();
                    assert!(after
                        .source_fidelity()
                        .parts()
                        .values()
                        .all(|part| part.state() == &(PartState::Decoded {})));
                    let expected = serde_json::to_value(before.ir()).unwrap();
                    let actual = serde_json::to_value(after.ir()).unwrap();
                    assert_decodes_agree(&expected, &actual, &Allowances::default(), Mode::Same);
                    assert_decodes_agree(&actual, &expected, &Allowances::default(), Mode::Same);
                    assert_eq!(
                        after
                            .report()
                            .losses
                            .iter()
                            .map(|loss| loss.code.to_string())
                            .collect::<Vec<_>>(),
                        before
                            .report()
                            .losses
                            .iter()
                            .map(|loss| loss.code.to_string())
                            .collect::<Vec<_>>()
                    );
                }
            }
            PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
            assert_eq!(
                before
                    .source_fidelity()
                    .parts()
                    .values()
                    .filter(|part| part.level().as_str() == "freecad.opaque_side_entry")
                    .count(),
                3
            );
        }
    }
}
