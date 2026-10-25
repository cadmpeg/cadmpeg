// SPDX-License-Identifier: Apache-2.0
//! An actual containing EOF never invents a later naming row or node.

use super::*;
use cadmpeg_test_support::comparison::{assert_decodes_agree, Allowances, DecodeSnapshot, Mode};

fn members(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let mut archive = ::zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut entry = archive.by_index(index).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            (entry.name().unwrap().into_owned(), data)
        })
        .collect()
}

fn archive(members: &[(String, Vec<u8>)]) -> Vec<u8> {
    archive_entries(
        &members
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>(),
    )
}

fn source() -> Vec<u8> {
    let mut data = members(&fixture());
    for (_, bytes) in &mut data {
        *bytes = String::from_utf8(std::mem::take(bytes))
            .unwrap()
            .replace(".c ", ".0 0:")
            .into_bytes();
    }
    archive(&data)
}

fn truncate(source: &[u8], target: &Snapshot) -> Vec<u8> {
    let mut data = members(source);
    let member = data
        .iter_mut()
        .find(|(name, _)| name == &target.member)
        .unwrap();
    let start = usize::try_from(target.offset).unwrap();
    let length = usize::try_from(target.len).unwrap();
    let cut = if target.level.ends_with("_record") {
        start + length - 2
    } else if target.level.ends_with("_entry") || target.level.ends_with("_header") {
        start + 1
    } else {
        start + length / 2
    };
    member.1.truncate(cut);
    archive(&data)
}

fn home(target: &Snapshot) -> &str {
    // An authentic XML-member prefix cut removes the enclosing Document end
    // tag; its Appendix C callback is the nearest provable XML failure home.
    if target.member == "Document.xml" {
        return "freecad.document";
    }
    let level = target.level.as_str();
    match level {
        "freecad.string_hasher_record"
        | "freecad.shape_string_hasher_record"
        | "freecad.string_table_record" => level,
        "freecad.inline_map_postfix_dictionary"
        | "freecad.inline_map_postfix"
        | "freecad.legacy_map_inline_record" => "freecad.property_value",
        "freecad.inline_map_node"
        | "freecad.inline_map_group"
        | "freecad.inline_map_child"
        | "freecad.inline_map_name_chain" => "freecad.inline_map_node",
        "freecad.element_map_node"
        | "freecad.element_map_group"
        | "freecad.element_map_child"
        | "freecad.element_map_name_chain" => "freecad.element_map_node",
        "freecad.string_table_entry" | "freecad.string_table_header" => {
            "freecad.string_table_entry"
        }
        _ => "freecad.element_map_entry",
    }
}

#[test]
fn every_naming_level_cut_short_has_its_proved_nearest_home_and_conserves_independent_records() {
    let bytes = source();
    let baseline = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let old_arena = DecodeArena::new();
    let (old_ctx, old_root) =
        DecodeContext::from_root_bytes(&bytes, &old_arena, &DecodePolicy::service()).unwrap();
    let old_root = old_ctx.name_stream(old_root, "file").unwrap();
    let frames = inventory(&bytes);
    for level in NAMING_LEVELS {
        let target = frames
            .iter()
            .rev()
            .find(|frame| frame.level == *level)
            .unwrap();
        let damaged = truncate(&bytes, target);
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(&damaged), &DecodeOptions::default())
            .unwrap();
        let failed = decoded
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1, "{level}: {:?}", decoded.report().losses);
        assert_eq!(
            failed[0].level().as_str(),
            home(target),
            "{level}: {:?}",
            decoded.report().losses
        );
        let new_arena = DecodeArena::new();
        let (new_ctx, new_root) =
            DecodeContext::from_root_bytes(&damaged, &new_arena, &DecodePolicy::service()).unwrap();
        let new_root = new_ctx.name_stream(new_root, "file").unwrap();
        let allowances = Allowances::given_up_zip(
            DecodeSnapshot::new(baseline.ir(), baseline.source_fidelity()),
            DecodeSnapshot::new(decoded.ir(), decoded.source_fidelity()),
            decoded.report(),
            (&old_ctx, old_root),
            (&new_ctx, new_root),
        )
        .unwrap_or_else(|error| panic!("{level}: {error}"));
        eprintln!("strong cut-short comparison for {level}");
        assert_decodes_agree(
            &serde_json::to_value(baseline.ir()).unwrap(),
            &serde_json::to_value(decoded.ir()).unwrap(),
            &allowances,
            Mode::Same,
        );
        assert!(
            crate::test_support::validate_native(decoded.ir()).is_empty(),
            "{level}"
        );
        for peer in baseline
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap()
        {
            if peer.name() == target.member {
                continue;
            }
            let actual = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::EntryRecord>("entries")
                .unwrap()
                .into_iter()
                .find(|entry| entry.name() == peer.name())
                .unwrap();
            assert_eq!(actual.id(), peer.id(), "{level}");
            assert_eq!(actual.role, peer.role, "{level}");
        }
        for part in decoded
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.level().as_str() == "zip.entry")
        {
            assert_eq!(part.state(), &PartState::Decoded {}, "{level}");
        }
    }
}
