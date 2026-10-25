// SPDX-License-Identifier: Apache-2.0
//! Independent framing and source mutation controls for typed property side entries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::framing::{
    freecad::binary::frame_binary_member, zip, FrameParent, PartTable,
};
use std::collections::BTreeSet;
use std::io::Cursor;

mod cuts;
mod damage;
mod fixtures;
mod oracle;

struct Boundary {
    member: String,
    level: String,
    stream: String,
    offset: u64,
    length: u64,
    framing: Vec<std::ops::Range<u64>>,
}

fn decode(bytes: &[u8]) -> DecodeResult {
    crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn assert_parts_agree(
    fixtures: &[fixtures::Fixture],
    bytes: &[u8],
    decoded: &DecodeResult,
) -> Vec<Boundary> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut selected = Vec::new();
    for (index, fixture) in fixtures.iter().enumerate() {
        for prefix in ["app", "gui", "app-copy"] {
            let name = format!("{prefix}-{index}.bin");
            let entry = entries
                .iter()
                .find(|entry| entry.name().window() == name.as_bytes())
                .unwrap();
            let member = archive.open(&ctx, &name).unwrap();
            let start = frames.len();
            frame_binary_member(&ctx, member, fixture.kind, entry.frame_index(), &mut frames)
                .unwrap();
            selected.extend((start..frames.len()).map(|index| (index, name.clone())));
        }
    }
    PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
    let parts = decoded.source_fidelity().parts();
    let mut matched = BTreeSet::new();
    let mut levels = BTreeSet::new();
    let mut boundaries = Vec::new();
    for (index, member) in selected {
        let frame = &frames[index];
        let capture = frame.capture();
        let (id, part) = parts
            .iter()
            .find(|(_, part)| {
                part.level() == frame.level()
                    && part.stream() == capture.stream()
                    && part.offset() == capture.offset()
                    && part.byte_len() == capture.byte_len()
            })
            .unwrap_or_else(|| panic!("missing independent binary frame {frame:?}"));
        assert!(matched.insert(id.clone()), "duplicate frame {frame:?}");
        levels.insert(frame.level().as_str());
        assert_eq!(part.framing_bytes(), frame.framing(), "{frame:?}");
        let FrameParent::Part(parent) = frame.parent() else {
            panic!("binary part lacks physical parent")
        };
        let expected = &frames[parent];
        let actual = &parts[part.parent().unwrap()];
        assert_eq!(actual.level(), expected.level(), "{frame:?}");
        assert_eq!(actual.stream(), expected.capture().stream(), "{frame:?}");
        assert_eq!(actual.offset(), expected.capture().offset(), "{frame:?}");
        assert_eq!(
            actual.byte_len(),
            expected.capture().byte_len(),
            "{frame:?}"
        );
        boundaries.push(Boundary {
            member,
            level: frame.level().as_str().into(),
            stream: capture.stream().into(),
            offset: capture.offset(),
            length: capture.byte_len(),
            framing: frame.framing().to_vec(),
        });
    }
    for (id, part) in parts
        .iter()
        .filter(|(_, part)| levels.contains(part.level().as_str()))
    {
        assert!(matched.contains(id), "decoder-only binary part {part:?}");
    }
    boundaries
}

#[test]
fn independent_binary_frames_match_every_property_list_and_material_version_in_both_xml_carriers() {
    let fixtures = fixtures::all();
    let original = fixtures::archive(&fixtures);
    for bytes in crate::test_support::framing::zip_forms(&original) {
        for depth in [
            cadmpeg_core::decode::DecodeDepth::Container,
            cadmpeg_core::decode::DecodeDepth::Geometry,
            cadmpeg_core::decode::DecodeDepth::Full,
        ] {
            let result = crate::FcstdCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            assert!(
                result
                    .source_fidelity()
                    .parts()
                    .values()
                    .all(|part| !matches!(
                        part.state(),
                        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    )),
                "{:#?}",
                result.report().losses
            );
            let boundaries = assert_parts_agree(&fixtures, &bytes, &result);
            assert!(boundaries
                .iter()
                .any(|part| part.level == "freecad.f32_record"));
            assert!(boundaries
                .iter()
                .any(|part| part.level == "freecad.mesh_material_value"));
            crate::test_support::framing::assert_xml_parts_agree(&bytes, &result);
            assert!(
                crate::test_support::validate_native(result.ir()).is_empty(),
                "{depth:?}: {:?}",
                crate::test_support::validate_native(result.ir())
            );
        }
    }
}

#[test]
fn first_middle_last_property_binary_content_damage_preserves_framing_and_independent_records() {
    let fixtures = fixtures::all();
    let bytes = fixtures::archive(&fixtures);
    let before = decode(&bytes);
    let boundaries = assert_parts_agree(&fixtures, &bytes, &before);
    let levels = boundaries
        .iter()
        .map(|part| part.level.as_str())
        .collect::<BTreeSet<_>>();
    for level in levels {
        let candidates = boundaries
            .iter()
            .filter(|part| part.level == level)
            .collect::<Vec<_>>();
        let choices = [0, candidates.len() / 2, candidates.len() - 1]
            .into_iter()
            .collect::<BTreeSet<_>>();
        for index in choices {
            for fill in [0_u8, 255] {
                let target = candidates[index];
                let damaged = damage::mutate(&bytes, target, fill);
                let after = decode(&damaged);

                damage::compare(&before, &after, &damaged, target);
            }
        }
    }
}
