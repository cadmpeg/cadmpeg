// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{
    ByteRange, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, SourceOrigin, View,
};

use crate::framing::{FrameParent, FramedPart, PartTable};

use super::frame_global;

fn card(section: u8, sequence: u32, payload: &[u8], ending: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 72);
    let mut bytes = [b' '; 80].to_vec();
    bytes[..payload.len()].copy_from_slice(payload);
    bytes[72] = section;
    bytes[73..80].copy_from_slice(format!("{sequence:>7}").as_bytes());
    bytes.extend_from_slice(ending);
    bytes
}

fn selected<'a, 'b>(parts: &'b [FramedPart<'a>], level: &str) -> Vec<&'b FramedPart<'a>> {
    parts
        .iter()
        .filter(|part| part.level().as_str() == level)
        .collect()
}

#[test]
fn declared_delimiters_and_counted_payload_shield_field_and_record_markers() {
    let bytes = card(b'G', 1, b"1H!!1H@!5ha!@G;!12@", b"\r\n");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    frame_global(&ctx, root, &mut parts).unwrap();
    PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].view().window(), bytes);
    assert_eq!(parts[0].framing(), [0..8, 18..19, 72..82]);
    assert_eq!(parts[0].dispatch(), [72..73]);
    let fields = selected(&parts, "iges.global_field");
    assert_eq!(fields[0].view().window(), b"5ha!@G;!");
    assert_eq!(fields[0].framing(), [8..10, 15..16]);
    assert_eq!(fields[0].content_ranges(), [10..15]);
    assert_eq!(fields[1].view().window(), b"12@");
    assert_eq!(fields[1].framing(), [18..19]);
    assert_eq!(fields[1].parent(), FrameParent::Part(0));
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn cross_card_hollerith_payload_has_real_composite_segments_and_exact_seam_reads() {
    let mut logical = b",,80H".to_vec();
    let mut payload = [b'a'; 80];
    payload[65..69].copy_from_slice(b";G,!");
    logical.extend_from_slice(&payload);
    logical.extend_from_slice(b",7;");
    let mut bytes = b"pre".to_vec();
    bytes.extend(card(b'G', 1, &logical[..72], b"\r\n"));
    bytes.extend(card(b'G', 2, &logical[72..], b"\r"));
    bytes.extend_from_slice(b"post");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, bytes.len() - 4).unwrap();
    let mut parts = Vec::new();
    ctx.record("framing", source, || {
        frame_global(&ctx, source, &mut parts).map_err(cadmpeg_core::CodecError::malformed)
    })
    .unwrap();
    let table = PartTable::load().unwrap();
    table.validate(&ctx, &parts).unwrap();
    assert_eq!((parts[0].view().start(), parts[0].view().end()), (3, 166));
    let fields = selected(&parts, "iges.global_field");
    assert_eq!(fields.len(), 2);
    assert_eq!(&fields[0].view().window()[3..83], payload);
    assert_eq!(fields[0].content_ranges(), [5..85]);
    let streams = ctx.source_streams().unwrap();
    let composite = streams
        .iter()
        .find(|stream| matches!(stream.origin(), SourceOrigin::Composite { .. }))
        .unwrap();
    let SourceOrigin::Composite { segments } = composite.origin() else {
        unreachable!()
    };
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].child(), ByteRange { start: 0, end: 72 });
    assert_eq!(segments[0].parent(), "file");
    assert_eq!(segments[0].extent(), ByteRange { start: 3, end: 75 });
    assert_eq!(
        segments[1].child(),
        ByteRange {
            start: 72,
            end: 144
        }
    );
    assert_eq!(
        segments[1].extent(),
        ByteRange {
            start: 85,
            end: 157
        }
    );
    ctx.record("cross-card-value", fields[0].view(), || {
        assert_eq!(
            fields[0].view().read_slice(70..74),
            Some(b";G,!".as_slice())
        );
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    let framing = captures
        .iter()
        .find(|capture| capture.id() == "framing")
        .unwrap();
    assert!(framing.reads().is_empty());
    let captures: Vec<_> = captures
        .iter()
        .filter(|capture| capture.id() == "cross-card-value")
        .collect();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].reads()["file"], [73..75, 85..87]);
    assert_eq!(captures[0].reads()[composite.name()], [70..74]);
}

#[test]
fn defaults_empty_fields_and_early_record_end_have_no_invented_fields() {
    for (payload, expected) in [(b",,,;".as_slice(), 2), (b"  ;".as_slice(), 0)] {
        let bytes = card(b'G', 1, payload, b"");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut parts = Vec::new();
        frame_global(&ctx, root, &mut parts).unwrap();
        PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
        let fields = selected(&parts, "iges.global_field");
        assert_eq!(fields.len(), expected);
        assert!(fields.iter().all(|field| field.content_ranges().is_empty()));
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn global_frames_compose_with_the_unchanged_physical_card_framer() {
    let mut bytes = card(b'S', 1, b"start", b"\n");
    bytes.extend(card(b'G', 1, b",,value;", b"\r\n"));
    bytes.extend(card(b'P', 1, b"116,0,0,0;", b"\n"));
    bytes.extend(card(b'T', 1, b"S      1G      1D      0P      1", b"\n"));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    crate::framing::iges::frame(&ctx, root, &mut parts).unwrap();
    let physical = parts.len();
    frame_global(&ctx, root, &mut parts).unwrap();
    PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
    assert_eq!(physical, 4);
    assert_eq!(parts.len(), 6);
    assert_eq!(parts[physical].level().as_str(), "iges.global_section");
    assert_eq!(parts[physical + 1].parent(), FrameParent::Part(physical));
    assert_eq!(parts[physical + 1].view().window(), b"value;");
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn global_dispatch_uses_field_positions_in_the_real_source() {
    let bytes = card(
        b'G',
        1,
        b",,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23;",
        b"\n",
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    frame_global(&ctx, root, &mut parts).unwrap();
    PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
    let actual: Vec<_> = parts[0]
        .dispatch()
        .iter()
        .map(|range| {
            root.child(
                usize::try_from(range.start).unwrap(),
                usize::try_from(range.end).unwrap(),
            )
            .unwrap()
            .window()
        })
        .collect();
    assert_eq!(
        actual,
        [b"7".as_slice(), b"8", b"9", b"10", b"11", b"23", b"G"]
    );
}

#[test]
fn malformed_declarations_and_cut_short_counts_do_not_guess_later_boundaries() {
    for payload in [
        b"1H!!1H@!999999999999999999999999999999999999Hshort@".as_slice(),
        b",,999Hshort;",
        b",,3Hfoo x;",
        b"1H!!1H@!123",
        b"2H!!,;",
        b"1HD D1H;;",
    ] {
        let bytes = card(b'G', 1, payload, b"\n");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut parts = Vec::new();
        assert!(frame_global(&ctx, root, &mut parts).is_err(), "{payload:?}");
        assert!(parts.is_empty());
        assert!(ctx.take_record_captures().unwrap().is_empty());
    }
}

#[test]
fn hollerith_header_and_numeric_delimiter_must_stay_on_one_card() {
    for suffix in [b"12Hpayload;".as_slice(), b"123;"] {
        let mut logical = b",,".to_vec();
        logical.resize(71, b' ');
        logical.extend_from_slice(suffix);
        let mut bytes = card(b'G', 1, &logical[..72], b"\n");
        bytes.extend(card(b'G', 2, &logical[72..], b"\n"));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut parts = Vec::new();
        assert!(frame_global(&ctx, root, &mut parts).is_err());
        assert!(parts.is_empty());
    }
}

#[test]
fn only_actual_global_card_payloads_before_terminate_can_supply_bytes() {
    for suffix in [b"\nD;\n".as_slice(), b"\n"] {
        let mut bytes = card(b'G', 1, b",,80Hshort", b"");
        bytes.truncate(73);
        bytes.extend_from_slice(suffix);
        bytes.extend(card(b'T', 1, b"", b"\n"));
        bytes.extend(card(b'G', 2, &[b'x'; 72], b"\n"));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut parts = Vec::new();
        assert!(frame_global(&ctx, root, &mut parts).is_err());
        assert!(parts.is_empty());
    }
    let bytes = card(b'G', 1, b",,ok;", b"");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes[..73], &arena, &DecodePolicy::service()).unwrap();
    let mut parts = Vec::new();
    frame_global(&ctx, root, &mut parts).unwrap();
    PartTable::load().unwrap().validate(&ctx, &parts).unwrap();
    assert_eq!(parts[0].framing(), [0..2, 4..5, 72..73]);
    assert_eq!(parts[1].view().window(), b"ok;");
}

#[test]
fn detached_foreign_sources_and_budget_refusals_remain_errors() {
    let bytes = card(b'G', 1, b",,ok;", b"\n");
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(frame_global(&ctx, View::over_retained(&bytes), &mut Vec::new()).is_err());
    let (_, foreign) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(frame_global(&ctx, foreign, &mut Vec::new()).is_err());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (bounded, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(frame_global(&bounded, root, &mut Vec::new()).is_err());
    assert_eq!(
        bounded.resource_refusal().unwrap().dimension,
        ResourceDimension::RetainedBytes
    );
}
