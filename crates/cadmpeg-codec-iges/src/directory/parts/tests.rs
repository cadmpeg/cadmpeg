// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceDimension, View,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{ParserPartOutput, PartState};
use cadmpeg_ir::NativeUnknownRecord;

use super::parse;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::{card, directory, global};

fn points() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ])
}

fn second_entry(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'D')
        .unwrap()
        * 81
        + 162
}

#[test]
fn actual_directory_callbacks_match_original_values_and_independent_source_extents() {
    let original = points();
    let mut bytes = b"pre".to_vec();
    bytes.extend_from_slice(&original);
    bytes.extend_from_slice(b"post");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, bytes.len() - 4).unwrap();
    let scan = card::scan_source(source, &ctx).unwrap();
    let table = global::parse(&scan).unwrap().0.global_table();
    let expected = directory::parse(&scan, table, &ctx).unwrap().0;
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let actual = parse(&scan, table, &ctx, &mut output).unwrap().entries;
    assert_eq!(actual, expected);
    let (_, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("finalize parser dependencies")
        .into_parts();
    assert!(losses.is_empty());
    assert!(native
        .namespace("iges")
        .is_none_or(|namespace| namespace.arenas().is_empty()));
    let mut reference = Vec::new();
    cadmpeg_test_support::framing::iges::frame(&ctx, source, &mut reference).unwrap();
    let mut expected: Vec<_> = reference
        .iter()
        .filter(|part| {
            matches!(
                part.level().as_str(),
                "iges.directory_section" | "iges.directory_entry"
            )
        })
        .map(|part| {
            (
                part.level().as_str(),
                part.capture().offset(),
                part.capture().byte_len(),
                part.framing().to_vec(),
            )
        })
        .collect();
    let mut actual: Vec<_> = fidelity
        .parts()
        .values()
        .map(|part| {
            (
                part.level().as_str(),
                part.offset(),
                part.byte_len(),
                part.framing_bytes().to_vec(),
            )
        })
        .collect();
    expected.sort_unstable_by_key(|part| (part.0, part.1));
    actual.sort_unstable_by_key(|part| (part.0, part.1));
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 3);
    let (section_id, _) = fidelity
        .parts()
        .iter()
        .find(|(_, part)| part.level().as_str() == "iges.directory_section")
        .unwrap();
    for part in fidelity
        .parts()
        .values()
        .filter(|part| part.level().as_str() == "iges.directory_entry")
    {
        assert_eq!(part.parent(), Some(section_id));
        assert_eq!(part.state(), &PartState::Decoded {});
    }
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn failed_real_pair_has_one_shared_home_and_preserves_the_independent_peer() {
    let original = points();
    let start = second_entry(&original);
    let mut damaged = original.clone();
    damaged[start + 48..start + 56].copy_from_slice(b"     abc");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&damaged, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root, &ctx).unwrap();
    let table = global::parse(&scan).unwrap().0.global_table();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let parsed = parse(&scan, table, &ctx, &mut output).unwrap();
    assert_eq!(parsed.given_up, [3]);
    let entries = parsed.entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].sequence, 1);
    let baseline = card::scan(&original).unwrap();
    let expected = directory::parse(&baseline, table, &ctx).unwrap().0;
    assert_eq!(entries[0], expected[0]);
    let (_, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("finalize parser dependencies")
        .into_parts();
    let unknowns = native
        .namespace("iges")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    let id = unknowns[0].id.as_str();
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(
        fidelity.retained_record(id).unwrap().data(),
        Some(&damaged[start..start + 162])
    );
    assert_eq!(fidelity.parts()[id].offset(), u64::try_from(start).unwrap());
    assert_eq!(fidelity.parts()[id].byte_len(), 162);
    assert_eq!(fidelity.parts()[id].state(), &PartState::GivenUp {});
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::Decoded {})
            .count(),
        2
    );
    assert!(fidelity.parts()[id].parent().is_some());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
}

#[test]
fn a_trailing_single_directory_card_uses_its_actual_known_extent() {
    let mut bytes = points();
    let start = second_entry(&bytes);
    bytes.drain(start + 81..start + 162);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root, &ctx).unwrap();
    let table = global::parse(&scan).unwrap().0.global_table();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    assert_eq!(
        parse(&scan, table, &ctx, &mut output)
            .unwrap()
            .entries
            .len(),
        1
    );
    let (_, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("finalize parser dependencies")
        .into_parts();
    let unknowns = native
        .namespace("iges")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    let id = unknowns[0].id.as_str();
    assert_eq!(
        fidelity.retained_record(id).unwrap().data(),
        Some(&bytes[start..start + 81])
    );
    assert_eq!(fidelity.parts()[id].byte_len(), 81);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
}

#[test]
fn detached_scans_and_session_entity_refusals_do_not_become_fabricated_parts() {
    let bytes = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(card::scan_source(View::over_retained(&bytes), &ctx).is_err());
    let scan = card::scan_source(root, &ctx).unwrap();
    let table = global::parse(&scan).unwrap().0.global_table();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let error = parse(&scan, table, &ctx, &mut output).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::Entities)
    );
}
