// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

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
        .expect("the fixture has a directory card")
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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the prefixed fixture fits the service policy");
    let source = root
        .child(3, bytes.len() - 4)
        .expect("the source window lies inside the root");
    let scan = card::scan_source(source).expect("the source window scans into cards");
    let table = global::parse(&scan)
        .expect("the global section parses")
        .0
        .global_table();
    let expected = directory::parse(&scan, table, &ctx)
        .expect("the directory section parses")
        .0;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let actual = parse(&scan, table, &ctx, &mut output)
        .expect("the part callbacks parse the directory section")
        .entries;
    assert_eq!(actual, expected);
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(losses.is_empty());
    assert!(native
        .namespace("iges")
        .is_none_or(|namespace| namespace.arenas().is_empty()));
    assert!(fidelity.retained_records().is_empty());
}

#[test]
fn failed_real_pair_has_one_shared_home_and_preserves_the_independent_peer() {
    let original = points();
    let start = second_entry(&original);
    let mut damaged = original.clone();
    damaged[start + 48..start + 56].copy_from_slice(b"     abc");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&damaged, &arena, &DecodePolicy::service())
        .expect("the damaged fixture fits the service policy");
    let scan = card::scan_source(root).expect("the damaged fixture scans into cards");
    let table = global::parse(&scan)
        .expect("the global section parses")
        .0
        .global_table();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let parsed = parse(&scan, table, &ctx, &mut output)
        .expect("a damaged entry does not refuse the directory section");
    assert_eq!(parsed.given_up, [3]);
    let entries = parsed.entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].sequence, 1);
    let baseline = card::scan(&original).expect("the original fixture scans into cards");
    let expected = directory::parse(&baseline, table, &ctx)
        .expect("the original directory section parses")
        .0;
    assert_eq!(entries[0], expected[0]);
    let (_, native, fidelity, losses) = output.into_parts();
    // #215 rule 1: the given-up part keeps one unknowns home.
    let unknowns = &native
        .namespace("iges")
        .expect("one unknowns home")
        .arenas()["unknowns"];
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .first_key_value()
            .expect("the damaged field is retained")
            .0
            .as_str()
    );
    // #222: a Directory Entry field is a given-up `iges.directory_field`.
    // The damaged field is the required transformation field (card 1
    // columns 49-56), so the record is not formed and its entry part stays
    // decoded (#215 rule 1).
    assert_eq!(fidelity.retained_records().len(), 1);
    let (_, field) = fidelity
        .retained_records()
        .first_key_value()
        .expect("the damaged field is retained");
    assert!(!field.untyped().is_empty());
    assert_eq!(field.data(), Some(&damaged[start + 48..start + 56]));
    assert_eq!(
        field.offset(),
        u64::try_from(start + 48).expect("the field offset fits u64")
    );
    assert_eq!(field.byte_len(), 8);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0].message.starts_with("part iges.directory_field "));
}

#[test]
fn a_trailing_single_directory_card_uses_its_actual_known_extent() {
    let mut bytes = points();
    let start = second_entry(&bytes);
    bytes.drain(start + 81..start + 162);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the truncated fixture fits the service policy");
    let scan = card::scan_source(root).expect("the truncated fixture scans into cards");
    let table = global::parse(&scan)
        .expect("the global section parses")
        .0
        .global_table();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    assert_eq!(
        parse(&scan, table, &ctx, &mut output)
            .expect("a trailing single card does not refuse the directory section")
            .entries
            .len(),
        1
    );
    let (_, native, fidelity, losses) = output.into_parts();
    // #215 rule 1: the given-up part keeps one unknowns home.
    let unknowns = &native
        .namespace("iges")
        .expect("one unknowns home")
        .arenas()["unknowns"];
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .first_key_value()
            .expect("the trailing card is retained")
            .0
            .as_str()
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    let (_, entry) = fidelity
        .retained_records()
        .first_key_value()
        .expect("the trailing card is retained");
    assert_eq!(entry.data(), Some(&bytes[start..start + 81]));
    assert_eq!(entry.byte_len(), 81);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
}

#[test]
fn detached_scans_and_session_entity_refusals_do_not_become_fabricated_parts() {
    let bytes = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("the fixture fits a zero entity limit at open");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let table = global::parse(&scan)
        .expect("the global section parses")
        .0
        .global_table();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let error = parse(&scan, table, &ctx, &mut output)
        .expect_err("a zero session entity limit refuses the directory section");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::Entities)
    );
}
