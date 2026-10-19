// SPDX-License-Identifier: Apache-2.0
//! Catalog parser tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::PREFIX;
use crate::test_support::test_object_graph::catalog_stream;

use super::parse;

#[test]
fn catalog_accepts_utf8_and_expression_line_feeds() {
    let entries = [
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "angle\n°",
    ];
    let mut body = vec![0x86];
    for entry in entries {
        body.push(u8::try_from(entry.len() + 1).expect("fixture entry fits in u8"));
        body.extend_from_slice(entry.as_bytes());
    }
    let total_len = 6 + body.len();
    let mut bytes = vec![0x7c, 0x02];
    bytes.extend_from_slice(
        &u32::try_from(total_len)
            .expect("fixture catalog length fits in u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&body);
    let catalogs = parse(&bytes).catalogs;
    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].entries[4].value, "angle\n°");
}

#[test]
fn catalog_rejects_a_count_larger_than_the_framed_entry_bytes() {
    let bytes = [0x7c, 0x02, 8, 0, 0, 0, 0xe4, 0xff];
    assert!(parse(&bytes).catalogs.is_empty());
}

#[test]
fn catalog_accepts_zero_tagged_u32_entry_lengths() {
    let long = "x".repeat(300);
    let entries = ["CATCatalogManager", "catalogManager", "catalogLinks", ""];
    let mut body = vec![0x86];
    for entry in entries {
        body.push(u8::try_from(entry.len() + 1).expect("fixture entry fits in u8"));
        body.extend_from_slice(entry.as_bytes());
    }
    body.push(0);
    body.extend_from_slice(
        &u32::try_from(long.len())
            .expect("fixture entry length fits in u32")
            .to_le_bytes(),
    );
    body.extend_from_slice(long.as_bytes());
    let total_len = 6 + body.len();
    let mut bytes = vec![0x7c, 0x02];
    bytes.extend_from_slice(
        &u32::try_from(total_len)
            .expect("fixture catalog length fits in u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&body);
    let catalogs = parse(&bytes).catalogs;
    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].entries[4].value, long);
}

#[test]
fn catalog_owns_catalog_shaped_bytes_inside_an_entry() {
    let mut nested = vec![0x7c, 0x02, 0, 0, 0, 0, 0xd1, 0x80];
    for entry in PREFIX {
        nested.push(u8::try_from(entry.len() + 1).expect("fixture entry length"));
        nested.extend_from_slice(entry.as_bytes());
    }
    nested.extend(std::iter::repeat_n(1, 123));
    nested.push(79);
    nested.extend(std::iter::repeat_n(b'x', 78));
    assert_eq!(nested.len(), 257);
    nested[2..6].copy_from_slice(&257u32.to_le_bytes());
    assert!(std::str::from_utf8(&nested).is_ok());

    let mut outer = vec![0x7c, 0x02, 0, 0, 0, 0, 0x86];
    for entry in PREFIX {
        outer.push(u8::try_from(entry.len() + 1).expect("fixture entry length"));
        outer.extend_from_slice(entry.as_bytes());
    }
    outer.push(0);
    outer.extend_from_slice(&257u32.to_le_bytes());
    outer.extend_from_slice(&nested);
    let outer_len = u32::try_from(outer.len()).expect("fixture catalog length");
    outer[2..6].copy_from_slice(&outer_len.to_le_bytes());

    let catalogs = parse(&outer).catalogs;
    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].pos, 0);
    assert_eq!(catalogs[0].entries.len(), 5);
}

#[test]
fn catalog_parser_reads_exact_inclusive_length_dictionary() {
    let entries = [
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
        "Pad",
    ];
    let catalogs = parse(&catalog_stream(&entries)).catalogs;

    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].entries.len() + 1, 7);
    assert_eq!(catalogs[0].entries.len(), entries.len());
    assert_eq!(catalogs[0].entries[4].ordinal, 4);
    assert_eq!(catalogs[0].entries[4].value, "Sketch");
    assert_eq!(catalogs[0].entries[5].value, "Pad");
}

#[test]
fn catalog_prefix_stream_parses_as_one_catalog() {
    let bytes = catalog_stream(&PREFIX);

    assert_eq!(parse(&bytes).catalogs.len(), 1);
}

/// Nested `7C02` candidates with the exact prefix, each declaring one entry
/// more than it frames, so every candidate walks to the end of the image and
/// then fails. Every byte is UTF-8, so each outer candidate reads the inner
/// candidate as one entry value.
fn failing_candidate_tower(levels: usize) -> Vec<u8> {
    const EMPTY_ENTRIES: usize = 121;
    let ascii_bytes = |value: usize| {
        u32::try_from(value)
            .ok()
            .filter(|value| value.to_le_bytes().iter().all(|byte| *byte < 0x80))
    };
    let mut inner = b"x".repeat(16);
    for _ in 0..levels {
        let inner_len = ascii_bytes(inner.len()).expect("inner candidate length is ASCII");
        let prefix_len = PREFIX.iter().map(|entry| entry.len() + 1).sum::<usize>();
        let (pad, total_len) = (0..127)
            .find_map(|pad| {
                let total = 6 + 2 + prefix_len + EMPTY_ENTRIES + 1 + pad + 5 + inner.len();
                ascii_bytes(total).map(|total| (pad, total))
            })
            .expect("a pad keeps the frame length ASCII");
        let mut candidate = vec![0x7c, 0x02];
        candidate.extend_from_slice(&total_len.to_le_bytes());
        // 128 declared entries; the frame holds 127.
        candidate.extend_from_slice(&[0xd1, 0x80]);
        for entry in PREFIX {
            candidate.push(u8::try_from(entry.len() + 1).expect("fixture entry length"));
            candidate.extend_from_slice(entry.as_bytes());
        }
        candidate.extend(std::iter::repeat_n(1, EMPTY_ENTRIES));
        candidate.push(u8::try_from(pad + 1).expect("fixture pad length"));
        candidate.extend(std::iter::repeat_n(b'y', pad));
        candidate.push(0);
        candidate.extend_from_slice(&inner_len.to_le_bytes());
        candidate.extend_from_slice(&inner);
        inner = candidate;
    }
    assert!(std::str::from_utf8(&inner).is_ok());
    inner
}

#[test]
fn failing_nested_candidates_stop_at_the_walk_allowance() {
    const LEVELS: usize = 64;
    let bytes = failing_candidate_tower(LEVELS);

    let scan = parse(&bytes);
    assert!(scan.catalogs.is_empty());
    assert!(scan.bounded_candidates > 0);
    assert!(scan.bounded_candidates < LEVELS);

    let mut refusal = crate::nurbs::LaneRefusals::new();
    refusal.push_catalog_scan_bounded(scan.bounded_candidates);
    let notes = refusal.take_notes();
    let [note] = notes.as_slice() else {
        panic!("one bounded catalog scan");
    };
    assert_eq!(
        note.code,
        crate::loss::CatiaLossCode::SourceCatalogScanBounded.kind()
    );
    assert!(note
        .message
        .contains("bound of 4 walked entry bytes per input byte"));
}

#[test]
fn a_few_failing_nested_candidates_fit_the_walk_allowance() {
    let scan = parse(&failing_candidate_tower(4));
    assert!(scan.catalogs.is_empty());
    assert_eq!(scan.bounded_candidates, 0);

    let mut refusal = crate::nurbs::LaneRefusals::new();
    refusal.push_catalog_scan_bounded(scan.bounded_candidates);
    assert_eq!(refusal.note_count(), 0);
}

#[test]
fn a_wrong_first_entry_rejects_before_the_walk() {
    let mut bytes = catalog_stream(&PREFIX);
    let first = bytes
        .windows(PREFIX[0].len())
        .position(|window| window == PREFIX[0].as_bytes())
        .expect("first prefix entry");
    bytes[first] = b'X';
    let scan = parse(&bytes);
    assert!(scan.catalogs.is_empty());
    assert_eq!(scan.bounded_candidates, 0);
}
