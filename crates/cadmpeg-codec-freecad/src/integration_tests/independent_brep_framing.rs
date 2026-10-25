// SPDX-License-Identifier: Apache-2.0
//! Independent Appendix C exact-shape boundaries and bounded source damage.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::framing::{
    freecad::brep::frame_brep_member, zip, FrameParent, PartTable,
};
use std::collections::BTreeSet;
use std::io::Cursor;

mod canonical_prefix;
mod carrier_admission;
mod compressed;
mod cut_short;
mod depths;
mod empty_triangulations;
mod entrypoints;
mod fixtures;
mod recovery;

#[derive(Debug)]
struct Boundary {
    member: String,
    member_offset: u64,
    level: String,
    stream: String,
    offset: u64,
    length: u64,
    framing: Vec<std::ops::Range<u64>>,
    content: Vec<std::ops::Range<u64>>,
    marker: bool,
    parent_end: u64,
    table: Option<String>,
}

fn decode(bytes: &[u8]) -> DecodeResult {
    crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn assert_parts_agree(bytes: &[u8], decoded: &DecodeResult) -> Vec<Boundary> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut names = BTreeSet::new();
    for xml_name in ["Document.xml", "GuiDocument.xml"] {
        if !entries
            .iter()
            .any(|entry| entry.name().window() == xml_name.as_bytes())
        {
            continue;
        }
        let source = archive.open(&ctx, xml_name).unwrap();
        let document =
            roxmltree::Document::parse(std::str::from_utf8(source.window()).unwrap()).unwrap();
        for property in document
            .descendants()
            .filter(|node| node.has_tag_name("Property"))
        {
            match property.attribute("type") {
                Some("Part::PropertyPartShape") => {
                    for value in property.children().filter(|node| node.has_tag_name("Part")) {
                        if let Some(name) = value.attribute("file").filter(|name| !name.is_empty())
                        {
                            names.insert(name.to_owned());
                        }
                    }
                }
                Some("Part::PropertyTopoShapeList") => {
                    for value in property
                        .descendants()
                        .filter(|node| node.has_tag_name("TopoShape"))
                    {
                        if let Some(name) = value.attribute("file").filter(|name| !name.is_empty())
                        {
                            names.insert(name.to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut brep = Vec::new();
    for name in names {
        let entry = entries
            .iter()
            .find(|entry| entry.name().window() == name.as_bytes())
            .unwrap_or_else(|| panic!("shape reference {name} has no genuine ZIP entry"));
        let member = archive.open(&ctx, &name).unwrap();
        let member_offset = ctx.capture_part_extent(member).unwrap().offset();
        let begin = frames.len();
        frame_brep_member(&ctx, member, entry.name(), entry.frame_index(), &mut frames).unwrap();
        brep.extend((begin..frames.len()).map(|index| (index, name.to_owned(), member_offset)));
    }
    PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
    let parts = decoded.source_fidelity().parts();
    let mut matched = BTreeSet::new();
    let mut boundaries = Vec::new();
    for (index, member, member_offset) in brep {
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
            .unwrap_or_else(|| panic!("missing independent BRep frame {frame:?}"));
        assert!(
            matched.insert(id.clone()),
            "duplicate independent frame {frame:?}"
        );
        assert_eq!(part.framing_bytes(), frame.framing(), "{frame:?}");
        let FrameParent::Part(parent) = frame.parent() else {
            panic!("BRep part lacks its genuine physical parent: {frame:?}")
        };
        let expected = &frames[parent];
        let actual = &parts[part.parent().expect("BRep part retains its genuine parent")];
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
            member_offset,
            level: frame.level().as_str().to_owned(),
            stream: capture.stream().to_owned(),
            offset: capture.offset(),
            length: capture.byte_len(),
            framing: frame.framing().to_vec(),
            content: frame.content_ranges_in(&frames).unwrap(),
            marker: frame.level().as_str() == "freecad.brep_binary_record"
                && frame.external_dispatch().is_empty(),
            parent_end: expected.capture().offset() + expected.capture().byte_len(),
            table: if frame.level().as_str() == "freecad.brep_binary_record" {
                frame.external_dispatch().first().map_or_else(
                    || std::str::from_utf8(frame.view().window()).ok(),
                    |view| std::str::from_utf8(view.window()).ok(),
                )
            } else if frame.level().as_str() == "freecad.brep_text_table" {
                std::str::from_utf8(frame.view().window()).ok()
            } else if expected.level().as_str() == "freecad.brep_text_table" {
                std::str::from_utf8(expected.view().window()).ok()
            } else {
                None
            }
            .and_then(|text| text.split_ascii_whitespace().next())
            .map(str::to_owned),
        });
    }
    for (id, part) in parts
        .iter()
        .filter(|(_, part)| part.level().as_str().starts_with("freecad.brep_"))
    {
        assert!(matched.contains(id), "decoder-only BRep part {part:?}");
    }
    boundaries
}

#[test]
fn independent_brep_framing_matches_every_frozen_golden_input() {
    use cadmpeg_test_support::golden::Harness;
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures");
    let inputs = Harness::new(env!("CARGO_MANIFEST_DIR"), "FCStd", "")
        .with_fixture_dir(fixtures)
        .fixture_inputs();
    assert!(!inputs.is_empty());
    for (name, bytes) in inputs {
        let result = decode(&bytes);
        let boundaries = assert_parts_agree(&bytes, &result);
        assert!(
            boundaries.iter().all(|part| !part.stream.is_empty()),
            "{name}"
        );
    }
}

#[test]
fn independent_framing_covers_all_eight_tables_and_recursive_geometry_in_both_carriers() {
    for binary in [false, true] {
        let source = fixtures::member(binary);
        let bytes = fixtures::archive(binary, &source);
        let decoded = decode(&bytes);
        let boundaries = assert_parts_agree(&bytes, &decoded);
        assert!(
            decoded.source_fidelity().parts().values().all(|part| {
                !matches!(
                    part.state(),
                    cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                )
            }),
            "{:#?}",
            decoded.report().losses
        );
        if binary {
            assert_eq!(
                boundaries
                    .iter()
                    .filter(|part| part.level == "freecad.brep_binary_record")
                    .count(),
                24 + 8 // #224 comment6376511913: every table marker is a record.
            );
        } else {
            for level in [
                "location",
                "curve2d",
                "curve",
                "polygon3d",
                "polygon_on_triangulation",
                "surface",
                "triangulation",
                "tshape",
            ] {
                assert_eq!(
                    boundaries
                        .iter()
                        .filter(|part| part.level == format!("freecad.brep_text_{level}"))
                        .count(),
                    3,
                    "{level}"
                );
            }
        }
        for part in boundaries {
            assert!(part.offset.checked_add(part.length).is_some());
            assert!(part.framing.iter().all(|range| range.start < range.end));
        }
        crate::test_support::test_archive::assert_valid_document(decoded.ir());
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    }
}

fn reseal_damage(bytes: &[u8], target: &Boundary, fill: u8) -> Vec<u8> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut member = archive
        .open(&ctx, &target.member)
        .unwrap()
        .window()
        .to_vec();
    for position in target.offset..target.offset + target.length {
        if target.content.iter().any(|range| range.contains(&position)) {
            let local = usize::try_from(position - target.member_offset).unwrap();
            member[local] = fill;
        }
    }
    zip::rebuild_member(&ctx, root, target.member.as_bytes(), &member).unwrap()
}

fn compare_damage(
    original: &[u8],
    before: &DecodeResult,
    after: &DecodeResult,
    bytes: &[u8],
    streams: &std::collections::BTreeMap<String, String>,
    target: Option<&Boundary>,
) {
    use cadmpeg_test_support::comparison::{
        assert_decodes_agree, Allowances, DecodeSnapshot, Mode,
    };
    let before_arena = DecodeArena::new();
    let (before_ctx, before_root) =
        DecodeContext::from_root_bytes(original, &before_arena, &DecodePolicy::service()).unwrap();
    let before_root = before_ctx.name_stream(before_root, "file").unwrap();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    assert_surface_directrices_keep_their_actual_native_home(before, after);
    let mut frames = Vec::new();
    let damage_boundary = target;
    let target_index = target.map(|target| {
        let entries = zip::frame(&before_ctx, before_root, &mut frames).unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.name().window() == target.member.as_bytes())
            .unwrap();
        let archive = cadmpeg_container::ArchiveSnapshot::new(&before_ctx, before_root).unwrap();
        let member = archive.open(&before_ctx, &target.member).unwrap();
        frame_brep_member(
            &before_ctx,
            member,
            entry.name(),
            entry.frame_index(),
            &mut frames,
        )
        .unwrap();
        frames
            .iter()
            .position(|frame| {
                frame.level().as_str() == target.level
                    && frame.capture().stream() == target.stream
                    && frame.capture().offset() == target.offset
                    && frame.capture().byte_len() == target.length
            })
            .unwrap()
    });
    let target = target_index.map(|index| {
        cadmpeg_test_support::comparison::given_up::damage::DamageTarget::new(
            &before_ctx,
            &frames,
            index,
        )
        .unwrap()
    });
    let before_snapshot = DecodeSnapshot::new(before.ir(), before.source_fidelity());
    let after_snapshot = DecodeSnapshot::new(after.ir(), after.source_fidelity());
    let allowances = if let Some(target) = &target {
        Allowances::given_up_zip_damaged(
            before_snapshot,
            after_snapshot,
            after.report(),
            (&before_ctx, before_root),
            (&ctx, root),
            target,
        )
    } else {
        Allowances::given_up_zip(
            before_snapshot,
            after_snapshot,
            after.report(),
            (&before_ctx, before_root),
            (&ctx, root),
        )
    }
    .unwrap();
    let inventory = assert_parts_agree(original, before);
    assert_canonical_prefix(before, after, streams, &inventory, damage_boundary);
    assert_decodes_agree(
        &serde_json::to_value(before.ir()).unwrap(),
        &serde_json::to_value(after.ir()).unwrap(),
        &allowances,
        Mode::Same,
    );
}

fn assert_surface_directrices_keep_their_actual_native_home(
    before: &DecodeResult,
    after: &DecodeResult,
) {
    let native = after.ir().native.namespace("fcstd").unwrap();
    for curve in &before.ir().model.curves {
        if let Some(current) = after
            .ir()
            .model
            .curves
            .iter()
            .find(|current| current.id == curve.id)
        {
            assert_eq!(
                current.native_ref, curve.native_ref,
                "surviving curve keeps its actual canonical native home"
            );
            if let Some(home) = current.native_ref.as_deref() {
                assert!(
                    native
                        .arenas()
                        .values()
                        .any(|rows| rows.iter().any(|row| row.id() == home)),
                    "{} has no actual native record home {home}",
                    current.id
                );
            }
        }
    }
}

fn assert_canonical_prefix(
    before: &DecodeResult,
    after: &DecodeResult,
    streams: &std::collections::BTreeMap<String, String>,
    inventory: &[Boundary],
    target: Option<&Boundary>,
) {
    use crate::brep::records::payloads_untracked;
    use cadmpeg_ir::source_fidelity::parts::PartState;
    let previous = before.ir().native.namespace("fcstd").unwrap();
    let current = after.ir().native.namespace("fcstd").unwrap();
    let old = payloads_untracked(previous).unwrap();
    let new = payloads_untracked(current).unwrap();
    let mut unavailable = recovery::sequential_suffixes(before, after, streams);
    unavailable.extend(recovery::physical_suffixes(before, after, streams));
    // This exact original target was authenticated by given_up_zip_damaged
    // before this auxiliary assertion. The amended sequential region starts
    // at X, even when a changed readable X makes the later Y fail.
    if let Some(target) = target {
        let level = cadmpeg_ir::source_fidelity::parts::PartLevel::parse(&target.level).unwrap();
        let end = if level.framing() == cadmpeg_ir::source_fidelity::parts::PartFraming::Sequential
        {
            target.parent_end
        } else {
            target.offset + target.length
        };
        unavailable.push((target.stream.clone(), target.offset..end));
    }
    let failed =
        recovery::assert_failed_headers_drop_owned_records(before, after, &old, &new, streams);
    let tables = [
        ("Locations", "locations"),
        ("Curve2ds", "curve2ds"),
        ("Curves", "curves"),
        ("Polygon3D", "polygons3d"),
        ("PolygonOnTriangulations", "polygons_on_triangulations"),
        ("Surfaces", "surfaces"),
        ("Triangulations", "triangulations"),
        ("TShapes", "tshapes"),
    ];
    for previous in &old {
        if failed.contains(&previous.id) {
            continue;
        }
        let source = &before.source_fidelity().record_layout()[&previous.id];
        let stream = &streams[source.stream()];
        for (table, (name, field)) in tables.iter().enumerate() {
            let mut rows = inventory
                .iter()
                .filter(|row| {
                    row.stream == source.stream()
                        && row.table.as_deref() == Some(*name)
                        && !row.marker
                        && row.level != "freecad.brep_text_table"
                })
                .collect::<Vec<_>>();
            rows.sort_by_key(|row| row.offset);
            for (offset, row) in rows.into_iter().enumerate() {
                if unavailable.iter().any(|(stream, range)| {
                    stream == &row.stream && range.start <= row.offset && row.offset < range.end
                }) {
                    continue;
                }
                if after.source_fidelity().parts().values().any(|part| {
                    matches!(part.state(), PartState::GivenUp {})
                        && part.stream() == stream
                        && part.offset() <= row.offset
                        && row.offset < part.offset() + part.byte_len()
                }) {
                    continue;
                }
                let current = new
                    .iter()
                    .find(|current| current.id == previous.id)
                    .unwrap_or_else(|| {
                        panic!("accepted prefix keeps actual payload {}", previous.id)
                    });
                let index = previous.source_index(u8::try_from(table).unwrap(), offset + 1);
                let label = format!("{}:{name}:{index}", previous.id);
                if table == 7 {
                    canonical_prefix::assert_shape(previous, current, index, &label);
                    continue;
                }
                if table == 0 {
                    canonical_prefix::assert_location(previous, current, index, &label);
                    continue;
                }
                if table == 3 || table == 4 {
                    let id = previous
                        .row_identity(u8::try_from(table).unwrap(), offset + 1)
                        .unwrap();
                    let arena = if table == 3 {
                        "polygons3d"
                    } else {
                        "polygons_on_triangulations"
                    };
                    let a = previous_polygon(previous, previous_native(before), arena, id);
                    let b = previous_polygon(current, current_native(after), arena, id);
                    assert_eq!(a, b, "accepted canonical polygon prefix {label}");
                    continue;
                }
                let a = serde_json::to_value(previous).unwrap();
                let b = serde_json::to_value(current).unwrap();
                let carrier = if a["text"].is_object() {
                    "text"
                } else {
                    "binary"
                };
                let values = a[carrier][field].as_array().unwrap();
                let surviving = b[carrier][field].as_array().unwrap();
                let current_index = (1..=surviving.len())
                    .find(|dense| {
                        current.source_index(u8::try_from(table).unwrap(), *dense) == index
                    })
                    .unwrap_or_else(|| panic!("accepted source slot {label} survives"));
                assert_eq!(
                    values[offset],
                    surviving[current_index - 1],
                    "accepted canonical prefix {label}"
                );
            }
        }
    }
}
fn previous_native(result: &DecodeResult) -> &cadmpeg_ir::native::NativeNamespace {
    result.ir().native.namespace("fcstd").unwrap()
}
fn current_native(result: &DecodeResult) -> &cadmpeg_ir::native::NativeNamespace {
    previous_native(result)
}
fn previous_polygon(
    _payload: &crate::brep::ShapePayloadRecord,
    native: &cadmpeg_ir::native::NativeNamespace,
    arena: &str,
    id: &str,
) -> serde_json::Value {
    serde_json::to_value(
        native.arenas()[arena]
            .iter()
            .find(|row| row.id() == id)
            .unwrap_or_else(|| panic!("accepted canonical polygon {id} survives")),
    )
    .unwrap()
}

fn damage_level(binary: bool, level: &str) {
    let original = fixtures::archive(binary, &fixtures::member(binary));
    let before = decode(&original);
    let boundaries = assert_parts_agree(&original, &before);
    damage_candidates(&original, &before, &boundaries, level);
}

fn damage_candidates(original: &[u8], before: &DecodeResult, boundaries: &[Boundary], level: &str) {
    let candidates = boundaries
        .iter()
        .filter(|part| part.level == level)
        .collect::<Vec<_>>();
    assert!(!candidates.is_empty(), "missing representative {level}");
    let choices = recovery::choices(&candidates);
    for index in choices {
        let target = candidates[index];
        for fill in [0_u8, 255] {
            let bytes = reseal_damage(original, target, fill);
            eprintln!(
                "BRep damage member={} level={} offset={} length={} fill={fill:#04x}",
                target.member, target.level, target.offset, target.length
            );
            let after = decode(&bytes);
            compressed::diagnose_changed_geometry(before, &after, target, fill);
            let damaged_boundaries = assert_parts_agree(&bytes, &after);
            let streams = recovery::stream_pairs(boundaries, &damaged_boundaries);
            let sequential = matches!(
                level,
                "freecad.brep_binary_record"
                    | "freecad.brep_text_location"
                    | "freecad.brep_text_curve2d"
                    | "freecad.brep_text_curve"
                    | "freecad.brep_text_polygon3d"
                    | "freecad.brep_text_polygon_on_triangulation"
                    | "freecad.brep_text_surface"
                    | "freecad.brep_text_triangulation"
            );
            let end = if sequential {
                target.parent_end
            } else {
                target.offset + target.length
            };
            recovery::assert_recovery_boundaries(target, fill, end, &damaged_boundaries, &after);
            assert!(
                after.report().losses.iter().all(|loss| {
                    loss.code.to_string() == "decode/record.unreadable"
                        || before
                            .report()
                            .losses
                            .iter()
                            .any(|prior| prior.code == loss.code)
                }),
                "damage {target:?} fill={fill:#x}: {:#?}",
                after.report().losses
            );
            compare_damage(original, before, &after, &bytes, &streams, Some(target));
            crate::fidelity::b1::assert_classified_spans(&after);
            assert!(
                crate::test_support::validate_native(after.ir()).is_empty(),
                "damage {target:?} fill={fill:#x}"
            );
        }
    }
}

macro_rules! damage_controls {
    ($($name:ident: $binary:literal, $level:literal;)*) => {$(
        #[test]
        fn $name() { damage_level($binary, $level); }
    )*};
}
damage_controls! {
    text_member_content_damage: false, "freecad.brep_text";
    text_header_content_damage: false, "freecad.brep_text_header";
    text_table_content_damage: false, "freecad.brep_text_table";
    text_location_content_damage: false, "freecad.brep_text_location";
    text_curve2d_content_damage: false, "freecad.brep_text_curve2d";
    text_curve_content_damage: false, "freecad.brep_text_curve";
    text_polygon3d_content_damage: false, "freecad.brep_text_polygon3d";
    text_indexed_polygon_content_damage: false, "freecad.brep_text_polygon_on_triangulation";
    text_surface_content_damage: false, "freecad.brep_text_surface";
    text_triangulation_content_damage: false, "freecad.brep_text_triangulation";
    text_tshape_content_damage: false, "freecad.brep_text_tshape";
    text_root_content_damage: false, "freecad.brep_text_root";
    binary_member_content_damage: true, "freecad.brep_binary";
    binary_header_content_damage: true, "freecad.brep_binary_header";
    binary_record_content_damage: true, "freecad.brep_binary_record";
    binary_root_content_damage: true, "freecad.brep_binary_root";
}

#[test]
fn zeroed_binary_polygon_count_exposes_the_unlisted_selector_boundary() {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    let member = fixtures::member(true);
    assert_eq!(member.len(), 2166);
    let bytes = fixtures::archive(true, &member);
    let before = decode(&bytes);
    let boundaries = assert_parts_agree(&bytes, &before);
    let target = boundaries
        .iter()
        .find(|part| part.level == "freecad.brep_binary_record" && part.offset == 1109)
        .expect("first PolygonOnTriangulations row");
    assert_eq!(target.length, 37);
    let damaged = reseal_damage(&bytes, target, 0);
    let after = decode(&damaged);
    let failures = after
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp {}))
        .collect::<Vec<_>>();
    // #224 comment6376511913: the zero count is readable; the first
    // subsequent failed row/marker gives up only its sequential suffix.
    let [failed] = failures.as_slice() else {
        panic!("{target:?}: {failures:?}")
    };
    assert_eq!(failed.level().as_str(), "freecad.brep_binary_record");
    assert!(failed.offset() >= target.offset);
    assert_eq!(failed.offset() + failed.byte_len(), target.parent_end);
    for part in before.source_fidelity().parts().values().filter(|part| {
        part.stream() == target.stream && part.offset() + part.byte_len() <= target.offset
    }) {
        assert!(after
            .source_fidelity()
            .parts()
            .values()
            .any(|current| current.level() == part.level()
                && current.offset() == part.offset()
                && current.byte_len() == part.byte_len()
                && current.state() == part.state()));
    }
    let current = assert_parts_agree(&damaged, &after);
    let streams = recovery::stream_pairs(&boundaries, &current);
    compare_damage(&bytes, &before, &after, &damaged, &streams, Some(target));
}
