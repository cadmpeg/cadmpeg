// SPDX-License-Identifier: Apache-2.0
//! Compare production naming parts with the independent Appendix C framer.

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, View};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_test_support::framing::{freecad, zip, PartTable};
use freecad::naming::{self, StringRecords};
use std::collections::BTreeMap;
use std::ops::Range;

mod cut_short;

const MAP: &str = "42 PostfixCount 3 a b c\nMapCount 3\nElementMap 1 42 1\nFace ChildCount 3\n1 0 1 0 0 suffix 0\n2 0 1 0 0 suffix 0\n3 0 1 0 0 suffix 0\nNameCount 3\n0\n;a.0 0\n;b.0 0\nEndMap\nElementMap 2 42 1\nEdge ChildCount 1\n1 0 1 0 1 suffix 0\nNameCount 1\n;edge.0 0\nEndMap\nElementMap 3 42 1\nVertex ChildCount 1\n1 0 1 0 2 suffix 0\nNameCount 1\n;vertex.0 0\nEndMap\n";
const LEGACY: &str = "Face1 stableFace 0\nEdge1 stableEdge 0\nVertex1 stableVertex 0\n";
const NAMING_LEVELS: &[&str] = &[
    "freecad.string_hasher_record",
    "freecad.shape_string_hasher_record",
    "freecad.inline_map_postfix_dictionary",
    "freecad.inline_map_postfix",
    "freecad.inline_map_node",
    "freecad.inline_map_group",
    "freecad.inline_map_child",
    "freecad.inline_map_name_chain",
    "freecad.legacy_map_inline_record",
    "freecad.string_table_entry",
    "freecad.string_table_header",
    "freecad.string_table_record",
    "freecad.element_map_entry",
    "freecad.element_map_header",
    "freecad.element_map_postfix_dictionary",
    "freecad.element_map_postfix",
    "freecad.element_map_node",
    "freecad.element_map_group",
    "freecad.element_map_child",
    "freecad.element_map_name_chain",
    "freecad.legacy_map_record",
];

fn fixture() -> Vec<u8> {
    let document = format!(
        "<Document SchemaVersion='4' FileVersion='2'><StringHasher saveall='0' threshold='0' count='3'>1.c first\n2.0 1:two\nlines\n3.c last\n</StringHasher><Objects Count='1'><Object name='Shape' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='5'><Property name='Inline' type='Part::PropertyPartShape'><Part ElementMap='1'/><StringHasher saveall='1' threshold='0' count='3'>4.c shapeFirst\n5.c shapeMiddle\n6.c shapeLast\n</StringHasher><ElementMap new='1'/><ElementMap2>{MAP}</ElementMap2></Property><Property name='LegacyInline' type='Part::PropertyPartShape'><Part ElementMap='1'/><ElementMap count='3'>{LEGACY}</ElementMap></Property><Property name='SideTable' type='Part::PropertyPartShape'><Part/><StringHasher new='1' saveall='0' threshold='0'/><StringHasher2 file='strings.txt'/></Property><Property name='Side' type='Part::PropertyPartShape'><Part ElementMap='1'/><ElementMap file='map.txt'/></Property><Property name='LegacySide' type='Part::PropertyPartShape'><Part ElementMap='1'/><ElementMap file='legacy.txt'/></Property></Properties></Object></ObjectData></Document>"
    );
    let side = format!("BeginElementMap v1\n{MAP}");
    let legacy = format!("3\n{LEGACY}");
    archive_entries(&[
        ("Document.xml", document.as_bytes()),
        (
            "strings.txt",
            b"StringTableStart v1 3\n11.c sideFirst\n12.c sideMiddle\n13.c sideLast\n",
        ),
        ("map.txt", side.as_bytes()),
        ("legacy.txt", legacy.as_bytes()),
    ])
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    level: String,
    member: String,
    offset: u64,
    len: u64,
    framing: Vec<Range<u64>>,
    content: Vec<Range<u64>>,
}

fn attribute<'a>(
    element: cadmpeg_core::decode::XmlElement<'a>,
    name: &str,
) -> Option<(View<'a>, View<'a>)> {
    let extent = element.extent();
    let opening_end = element.framing().nth(1)?.end();
    let opening = extent.child(extent.start(), opening_end)?;
    let pattern = format!("{name}=");
    let start = opening
        .window()
        .windows(pattern.len())
        .position(|window| window == pattern.as_bytes())?;
    let value_start = start + pattern.len() + 1;
    let quote = opening.window()[value_start - 1];
    let end = value_start
        + opening.window()[value_start..]
            .iter()
            .position(|byte| *byte == quote)?;
    Some((
        opening.child(opening.start() + start, opening.start() + end + 1)?,
        opening.child(opening.start() + value_start, opening.start() + end)?,
    ))
}

fn text(element: cadmpeg_core::decode::XmlElement<'_>) -> Option<View<'_>> {
    let markers = element.framing().collect::<Vec<_>>();
    if markers.len() != 3 {
        return None;
    }
    element.extent().child(markers[1].end(), markers[2].start())
}

fn inventory(bytes: &[u8]) -> Vec<Snapshot> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let mut members = BTreeMap::new();
    let mut member_streams = BTreeMap::new();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    for entry in &entries {
        let name = std::str::from_utf8(entry.name().window()).unwrap();
        let source = archive.open(&ctx, name).unwrap();
        member_streams.insert(
            ctx.capture_part_extent(source).unwrap().stream().to_owned(),
            name.to_owned(),
        );
        members.insert(name, (source, entry));
    }
    let (document, entry) = members["Document.xml"];
    let reached = freecad::frame_xml_member(
        &ctx,
        document,
        entry.name(),
        entry.frame_index(),
        &mut frames,
    )
    .unwrap();
    for (index, element) in reached.scan().elements().iter().copied().enumerate() {
        let name = element.name().window();
        if !matches!(
            name,
            b"StringHasher" | b"StringHasher2" | b"ElementMap" | b"ElementMap2"
        ) {
            continue;
        }
        let parent = reached.frame_for_element(index).unwrap();
        if let Some((file_attribute, value)) = attribute(element, "file") {
            let file = std::str::from_utf8(value.window()).unwrap();
            let (source, entry) = members[file];
            if matches!(name, b"StringHasher" | b"StringHasher2") {
                naming::frame_string_table_entry(
                    &ctx,
                    source,
                    file_attribute,
                    entry.frame_index(),
                    &mut frames,
                )
                .unwrap();
            } else {
                naming::frame_element_map_entry(
                    &ctx,
                    source,
                    file_attribute,
                    entry.frame_index(),
                    &mut frames,
                )
                .unwrap();
            }
        } else if let Some(body) = text(element) {
            if matches!(name, b"StringHasher" | b"StringHasher2") {
                let count = attribute(element, "count").unwrap().1;
                let count = std::str::from_utf8(count.window())
                    .unwrap()
                    .parse()
                    .unwrap();
                let layout = if frames[parent].level().as_str() == "freecad.string_hasher" {
                    StringRecords::Document { count }
                } else {
                    StringRecords::Shape { count }
                };
                naming::frame_string_records(&ctx, body, layout, parent, &mut frames).unwrap();
            } else if name == b"ElementMap2" {
                naming::frame_inline_map(&ctx, body, parent, &mut frames).unwrap();
            } else if let Some((_, count)) = attribute(element, "count") {
                let count = std::str::from_utf8(count.window())
                    .unwrap()
                    .parse()
                    .unwrap();
                naming::frame_legacy_records(&ctx, body, Some(count), parent, &mut frames).unwrap();
            }
        }
    }
    let table = PartTable::load().unwrap();
    table.validate(&ctx, &frames).unwrap();
    frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| NAMING_LEVELS.contains(&frame.level().as_str()))
        .map(|(index, frame)| {
            let mut excluded = frames
                .iter()
                .filter(|other| other.capture().stream() == frame.capture().stream())
                .flat_map(|other| other.framing().iter().chain(other.dispatch()))
                .cloned()
                .collect::<Vec<_>>();
            excluded.sort_by_key(|range| range.start);
            let mut content = table.owned_ranges(&ctx, &frames, index).unwrap();
            for dispatch in excluded {
                content = content
                    .into_iter()
                    .flat_map(|range| {
                        let mut kept = Vec::new();
                        if range.start < dispatch.start {
                            kept.push(range.start..range.end.min(dispatch.start));
                        }
                        if dispatch.end < range.end {
                            kept.push(range.start.max(dispatch.end)..range.end);
                        }
                        kept
                    })
                    .collect();
            }
            Snapshot {
                level: frame.level().as_str().into(),
                member: member_streams
                    .get(frame.capture().stream())
                    .unwrap()
                    .clone(),
                offset: frame.capture().offset(),
                len: frame.capture().byte_len(),
                framing: frame.framing().to_vec(),
                content,
            }
        })
        .collect()
}

#[test]
fn independent_naming_framer_matches_every_reached_naming_level_and_exact_framing() {
    for source in crate::test_support::framing::zip_forms(&fixture()) {
        for depth in [
            DecodeDepth::Container,
            DecodeDepth::Geometry,
            DecodeDepth::Full,
        ] {
            let expected = inventory(&source);
            for level in NAMING_LEVELS {
                assert!(expected.iter().any(|part| part.level == *level), "{level}");
            }
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(&source),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            assert!(
                !decoded
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.code.local_code() == "record.unreadable"),
                "{:?}",
                decoded.report().losses
            );
            let mut actual = decoded
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| NAMING_LEVELS.contains(&part.level().as_str()))
                .map(|part| {
                    assert_eq!(part.state(), &PartState::Decoded {});
                    (
                        part.level().as_str().to_owned(),
                        part.stream().rsplit(':').next().unwrap().to_owned(),
                        part.offset(),
                        part.byte_len(),
                        part.framing_bytes().to_vec(),
                    )
                })
                .collect::<Vec<_>>();
            let mut expected = expected
                .into_iter()
                .map(|part| (part.level, part.member, part.offset, part.len, part.framing))
                .collect::<Vec<_>>();
            actual.sort_by_key(|part| (part.0.clone(), part.1.clone(), part.2, part.3));
            expected.sort_by_key(|part| (part.0.clone(), part.1.clone(), part.2, part.3));
            assert_eq!(actual, expected);
            assert!(
                crate::test_support::validate_native(decoded.ir()).is_empty(),
                "{depth:?}"
            );
        }
    }
}

fn damaged(source: &[u8], target: &Snapshot) -> Option<Vec<u8>> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let entries = zip::frame(&ctx, root, &mut Vec::new()).unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.name().window() == target.member.as_bytes())
        .unwrap();
    let payload = entry.payload();
    let offset = target
        .content
        .iter()
        .flat_map(|range| range.clone())
        .find(|offset| {
            !payload.window()[usize::try_from(*offset).unwrap()].is_ascii_whitespace()
        })?;
    let at = u64::try_from(payload.start()).unwrap() + offset;
    Some(
        zip::rebuild(
            &ctx,
            root,
            &[zip::Edit {
                range: at..at + 1,
                replacement: &[0xff],
            }],
        )
        .unwrap(),
    )
}

#[test]
fn independent_first_middle_last_naming_content_damage_has_one_exact_home_and_keeps_peers() {
    let source = fixture();
    let frames = inventory(&source);
    let mut exercised = std::collections::BTreeSet::new();
    for level in NAMING_LEVELS {
        let parts = frames
            .iter()
            .filter(|part| part.level == *level)
            .collect::<Vec<_>>();
        assert!(!parts.is_empty(), "missing representative {level}");
        let selected = [0, parts.len() / 2, parts.len() - 1]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        for selected in selected {
            let target = parts[selected];
            let Some(bytes) = damaged(&source, target) else {
                continue;
            };
            exercised.insert(*level);
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
                .unwrap();
            let fidelity = decoded.source_fidelity();
            let given_up = fidelity
                .parts()
                .iter()
                .filter(|(_, part)| matches!(part.state(), PartState::GivenUp {}))
                .collect::<Vec<_>>();
            assert_eq!(
                given_up.len(),
                1,
                "{target:?}: {:?}",
                decoded.report().losses
            );
            let (id, part) = given_up[0];
            assert_eq!(
                part.level().as_str(),
                target.level,
                "{target:?}: {:?}",
                decoded.report().losses
            );
            assert_eq!(part.offset(), target.offset);
            assert_eq!(part.byte_len(), target.len);
            let losses = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "record.unreadable")
                .collect::<Vec<_>>();
            assert_eq!(losses.len(), 1, "{target:?}");
            let unknowns = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
                .unwrap();
            assert_eq!(unknowns.len(), 1, "{target:?}");
            assert_eq!(unknowns[0].id.as_str(), id.as_str());
            let retained = fidelity.retained_record(id.as_str()).unwrap();
            assert_eq!(
                retained.data().unwrap().len(),
                usize::try_from(target.len).unwrap()
            );
            for peer in &frames {
                if peer.member == target.member
                    && target.offset <= peer.offset
                    && peer.offset + peer.len <= target.offset + target.len
                {
                    continue;
                }
                let actual = fidelity
                    .parts()
                    .values()
                    .find(|part| {
                        part.level().as_str() == peer.level
                            && part.stream().rsplit(':').next() == Some(peer.member.as_str())
                            && part.offset() == peer.offset
                            && part.byte_len() == peer.len
                    })
                    .unwrap_or_else(|| panic!("lost independent peer {peer:?} after {target:?}"));
                assert_eq!(
                    actual.state(),
                    &PartState::Decoded {},
                    "{peer:?} after {target:?}"
                );
            }
            assert!(
                crate::test_support::validate_native(decoded.ir()).is_empty(),
                "{target:?}"
            );
        }
    }
    for level in [
        "freecad.string_hasher_record",
        "freecad.shape_string_hasher_record",
        "freecad.string_table_record",
        "freecad.string_table_header",
        "freecad.inline_map_postfix_dictionary",
        "freecad.element_map_postfix_dictionary",
        "freecad.inline_map_postfix",
        "freecad.element_map_postfix",
        "freecad.inline_map_child",
        "freecad.element_map_child",
        "freecad.inline_map_name_chain",
        "freecad.element_map_name_chain",
        "freecad.legacy_map_inline_record",
        "freecad.legacy_map_record",
    ] {
        assert!(
            exercised.contains(level),
            "unexercised semantic content in {level}"
        );
    }
}
