// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};

use crate::framing::{FrameParent, FramedPart, PartTable};

use super::frame_parameters;

fn card(marker: u8, sequence: usize, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 72);
    let mut bytes = [b' '; 80].to_vec();
    bytes[..payload.len()].copy_from_slice(payload);
    bytes[72] = marker;
    bytes[73..80].copy_from_slice(format!("{sequence:>7}").as_bytes());
    bytes.push(b'\n');
    bytes
}

fn directory(owner: usize, entity: usize, start: usize, count: usize) -> Vec<u8> {
    let mut first = [b' '; 72];
    first[..8].copy_from_slice(format!("{entity:>8}").as_bytes());
    first[8..16].copy_from_slice(format!("{start:>8}").as_bytes());
    let mut second = [b' '; 72];
    second[..8].copy_from_slice(format!("{entity:>8}").as_bytes());
    second[24..32].copy_from_slice(format!("{count:>8}").as_bytes());
    let mut bytes = card(b'D', owner, &first);
    bytes.extend(card(b'D', owner + 1, &second));
    bytes
}

fn parameter(sequence: usize, owner: usize, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 64);
    let mut bytes = [b' '; 72];
    bytes[..payload.len()].copy_from_slice(payload);
    bytes[64..72].copy_from_slice(format!(" {owner:>7}").as_bytes());
    card(b'P', sequence, &bytes)
}

fn selected<'a, 'b>(parts: &'b [FramedPart<'a>], name: &str) -> Vec<&'b FramedPart<'a>> {
    parts
        .iter()
        .filter(|part| part.level().as_str() == name)
        .collect()
}

#[test]
fn declared_card_count_includes_comments_and_reuses_the_physical_section() {
    let mut bytes = card(b'G', 1, b",,;");
    bytes.extend(directory(1, 116, 1, 2));
    bytes.extend(parameter(1, 1, b"116,1.,2.,3.; comment; P D G"));
    bytes.extend(parameter(2, 1, b"comments,not,fields;"));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    crate::framing::iges::frame(&ctx, root, &mut parts).unwrap();
    frame_parameters(&ctx, root, &mut parts).unwrap();
    let table = PartTable::load().unwrap();
    table.validate(&ctx, &parts).unwrap();
    assert_eq!(selected(&parts, "iges.parameter_section").len(), 1);
    let records = selected(&parts, "iges.parameter_record");
    assert_eq!(records.len(), 1);
    let section = parts
        .iter()
        .position(|part| part.level().as_str() == "iges.parameter_section")
        .unwrap();
    assert_eq!(records[0].parent(), FrameParent::Part(section));
    assert_eq!(
        (records[0].view().start(), records[0].view().end()),
        (243, 405)
    );
    assert_eq!(records[0].framing(), [255..256, 307..324, 388..405]);
    assert_eq!(records[0].dispatch(), [243..246]);
    let index = parts
        .iter()
        .position(|part| part.level().as_str() == "iges.parameter_record")
        .unwrap();
    assert_eq!(
        table.owned_ranges(&ctx, &parts, index).unwrap(),
        [256..307, 324..388]
    );
    let fields = selected(&parts, "iges.parameter_field");
    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].view().window(), b"116,");
    assert_eq!(fields[0].dispatch(), [0..3]);
    assert_eq!(fields[3].view().window(), b"3.;");
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn custom_delimiters_and_cross_card_strings_keep_exact_source_seam_reads() {
    let mut logical = b"212!70H".to_vec();
    let mut payload = [b'x'; 70];
    payload[55..59].copy_from_slice(b"@!PG");
    logical.extend_from_slice(&payload);
    logical.extend_from_slice(b"!1@");
    let mut bytes = b"pre".to_vec();
    bytes.extend(card(b'G', 1, b"1H!!1H@!@"));
    bytes.extend(directory(1, 212, 1, 2));
    bytes.extend(parameter(77, 1, &logical[..64]));
    bytes.extend(parameter(900, 1, &logical[64..]));
    bytes.extend_from_slice(b"post");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, bytes.len() - 4).unwrap();
    let mut parts = Vec::new();
    ctx.record("framing", source, || {
        crate::framing::iges::frame_global(&ctx, source, &mut parts)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        frame_parameters(&ctx, source, &mut parts).map_err(cadmpeg_core::CodecError::malformed)
    })
    .unwrap();
    PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
    let fields = selected(&parts, "iges.parameter_field");
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[1].content_ranges(), [7..77]);
    assert_eq!(&fields[1].view().window()[3..73], payload);
    ctx.record("parameter-string", fields[1].view(), || {
        assert_eq!(
            fields[1].view().read_slice(62..66),
            Some(b"@!PG".as_slice())
        );
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    let (framing, captures) = captures.split_first().unwrap();
    assert!(framing.reads().is_empty());
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].reads()["file"], [308..310, 327..329]);
}

#[test]
fn unusable_declarations_use_only_the_first_contiguous_real_owner_run() {
    for (start, count) in [(0, 0), (99, 5)] {
        let mut bytes = card(b'G', 1, b",,;");
        bytes.extend(directory(1, 116, start, count));
        bytes.extend(parameter(500, 1, b"116,1,2,3;"));
        bytes.extend(parameter(501, 99, b"unowned;"));
        bytes.extend(parameter(502, 1, b"not a second record;"));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut parts = Vec::new();
        frame_parameters(&ctx, root, &mut parts).unwrap();
        PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
        let records = selected(&parts, "iges.parameter_record");
        assert_eq!(records.len(), 1);
        assert_eq!(
            (records[0].view().start(), records[0].view().end()),
            (243, 324)
        );
        assert_eq!(selected(&parts, "iges.parameter_field").len(), 4);
    }
}

#[test]
fn usable_declared_ranges_bound_cards_even_when_the_back_pointer_is_unusable() {
    let mut bytes = card(b'G', 1, b",,;");
    bytes.extend(directory(1, 116, 1, 1));
    bytes.extend(parameter(1, 0, b"116,1,2,3;"));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    frame_parameters(&ctx, root, &mut parts).unwrap();
    PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
    assert_eq!(selected(&parts, "iges.parameter_record").len(), 1);
    assert_eq!(selected(&parts, "iges.parameter_field").len(), 4);
}

#[test]
fn overlapping_declarations_and_known_owner_conflicts_cannot_make_duplicate_records() {
    for (second_start, first_owner) in [(1, 1), (2, 3)] {
        let mut bytes = card(b'G', 1, b",,;");
        bytes.extend(directory(1, 116, 1, 1));
        bytes.extend(directory(3, 116, second_start, 1));
        bytes.extend(parameter(1, first_owner, b"116,1,2,3;"));
        bytes.extend(parameter(2, 3, b"116,4,5,6;"));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut parts = Vec::new();
        assert!(frame_parameters(&ctx, root, &mut parts).is_err());
        assert!(parts.is_empty());
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn cut_short_cards_and_unpaired_directory_owners_keep_only_real_bytes() {
    let mut bytes = card(b'G', 1, b",,;");
    bytes.extend_from_slice(&directory(1, 116, 0, 0)[..81]);
    let mut last = parameter(1, 1, b"116,1,2,3;");
    last.truncate(73);
    bytes.extend(last);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    frame_parameters(&ctx, root, &mut parts).unwrap();
    PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
    let records = selected(&parts, "iges.parameter_record");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].view().end(), bytes.len());
    assert_eq!(
        records[0].framing().last().unwrap().end,
        u64::try_from(bytes.len()).unwrap()
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes[..bytes.len() - 1], &arena, &DecodePolicy::service())
            .unwrap();
    let mut absent = Vec::new();
    frame_parameters(&ctx, root, &mut absent).unwrap();
    assert!(absent.is_empty());
}

#[test]
fn malformed_field_boundaries_cannot_consume_envelopes_or_other_sections() {
    let mut split_header = b"116,".to_vec();
    split_header.resize(63, b' ');
    split_header.extend_from_slice(b"12Hpayload;");
    let mut split_atom = b"116,".to_vec();
    split_atom.resize(63, b' ');
    split_atom.extend_from_slice(b"12;");
    for logical in [
        b"116,999Hshort;".to_vec(),
        b"116,0H;".to_vec(),
        b"110,1,2,3;".to_vec(),
        split_header,
        split_atom,
        b"116,1,2,3".to_vec(),
    ] {
        let count = logical.len().div_ceil(64);
        let mut bytes = card(b'G', 1, b",,;");
        bytes.extend(directory(1, 116, 1, count));
        for (index, payload) in logical.chunks(64).enumerate() {
            bytes.extend(parameter(index + 1, 1, payload));
        }
        bytes.extend(card(b'T', 1, b";"));
        bytes.extend(parameter(count + 1, 1, b";"));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut parts = Vec::new();
        assert!(
            frame_parameters(&ctx, root, &mut parts).is_err(),
            "{logical:?}"
        );
        assert!(selected(&parts, "iges.parameter_record").is_empty());
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn detached_sources_and_retained_budget_refusals_are_not_suppressed() {
    let mut bytes = card(b'G', 1, b",,;");
    bytes.extend(directory(1, 116, 1, 1));
    bytes.extend(parameter(1, 1, b"116,1,2,3;"));
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(frame_parameters(&ctx, View::over_retained(&bytes), &mut Vec::new()).is_err());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (bounded, source) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(frame_parameters(&bounded, source, &mut Vec::new()).is_err());
    assert_eq!(
        bounded.resource_refusal().unwrap().dimension,
        ResourceDimension::RetainedBytes
    );
}

#[test]
fn macro_records_keep_their_declared_extent_without_guessing_statement_children() {
    let mut bytes = card(b'G', 1, b",,;");
    bytes.extend(directory(1, 306, 1, 2));
    bytes.extend(parameter(1, 1, b"306,MACRO,110,1H;;ENDM; comment"));
    bytes.extend(parameter(2, 1, b"additional comment card"));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    frame_parameters(&ctx, root, &mut parts).unwrap();
    let table = PartTable::load().unwrap();
    table.validate(&ctx, &parts).unwrap();
    let records = selected(&parts, "iges.parameter_record");
    assert_eq!(records.len(), 1);
    assert_eq!(
        (records[0].view().start(), records[0].view().end()),
        (243, 405)
    );
    assert_eq!(records[0].framing(), [307..324, 388..405]);
    assert_eq!(records[0].dispatch(), [243..246]);
    assert!(selected(&parts, "iges.parameter_field").is_empty());
    assert!(selected(&parts, "iges.macro_statement").is_empty());
    assert_eq!(
        table.owned_ranges(&ctx, &parts, 1).unwrap(),
        [243..307, 324..388]
    );
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn fragmented_runs_return_an_error_without_removing_existing_physical_siblings() {
    let mut bytes = card(b'G', 1, b",,;");
    bytes.extend(directory(1, 116, 1, 1));
    bytes.extend(parameter(1, 1, b"116,1,2,3;"));
    bytes.extend_from_slice(b"unmarked\n");
    bytes.extend(parameter(2, 1, b"comment"));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    crate::framing::iges::frame(&ctx, root, &mut parts).unwrap();
    let count = parts.len();
    assert!(frame_parameters(&ctx, root, &mut parts).is_err());
    assert_eq!(parts.len(), count);
    assert_eq!(selected(&parts, "iges.parameter_section").len(), 2);
    assert!(selected(&parts, "iges.parameter_record").is_empty());
    PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
}
