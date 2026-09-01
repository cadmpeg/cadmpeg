// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use crate::test_support::*;
use crate::IgesCodec;

#[test]
fn overlong_preterminate_physical_line_is_malformed() {
    let mut bytes = point_file();
    let line_end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("Start line ending");
    bytes.insert(line_end, b'x');

    let error = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn inspect_rejects_unsequenced_physical_records_before_terminate() {
    let mut blank = vec![b' '; 80];
    blank.push(b'\n');
    let invalid_marker = card(b"", b'X', 1);

    for inserted in [blank, invalid_marker] {
        let mut bytes = point_file();
        let directory_card = bytes
            .chunks_exact(81)
            .position(|line| line[72] == b'D')
            .expect("Directory card");
        let offset = directory_card * 81;
        bytes.splice(offset..offset, inserted);

        let error = IgesCodec
            .inspect(
                &mut Cursor::new(bytes),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap_err();
        assert!(matches!(error, CodecError::Malformed(_)));
    }
}

#[test]
fn malformed_sequence_padding_is_rejected_without_panicking() {
    let mut bytes = point_file();
    bytes[CARD_DATA_COLUMNS + 1..CARD_COLUMNS].copy_from_slice(b"     1 ");

    assert_eq!(IgesCodec.detect(&bytes), Confidence::No);
    assert_eq!(
        IgesCodec
            .inspect(
                &mut Cursor::new(bytes),
                &cadmpeg_core::decode::InspectOptions::default()
            )
            .unwrap_err()
            .to_string(),
        "not the expected format: unrecognized IGES representation"
    );
}

#[test]
fn inspect_reports_sections_and_physical_line_endings() {
    let mut bytes = card_with_ending(b"original fixture", b'S', 1, b"\r\n");
    bytes.extend(card_with_ending(
        b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,1,1,1,1,1,,1,2,,1,1,13H240101.000000,0,0,,,11;",
        b'G',
        1,
        b"\n",
    ));
    bytes.extend(card_with_ending(
        b"S0000001G0000001D0000000P0000000",
        b'T',
        1,
        b"\r",
    ));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert_eq!(summary.format(), "iges");
    assert_eq!(summary.container_kind, "fixed-ascii");
    assert_eq!(summary.entries.len(), 3);
    assert_eq!(summary.entries[0].name, "start");
    assert_eq!(summary.entries[0].attributes["line_endings"], "crlf:1");
    assert_eq!(summary.entries[1].attributes["line_endings"], "lf:1");
    assert_eq!(summary.entries[2].attributes["line_endings"], "cr:1");
}

#[test]
fn decode_rejects_extended_physical_records_before_terminate() {
    let mut bytes = point_file();
    let mut inserted = b"short record\n".to_vec();
    inserted.extend(std::iter::repeat_n(b'x', CARD_COLUMNS + 1));
    inserted.push(b'\n');
    // The over-long record goes in at the last Global card so it precedes
    // the Directory section — the same offset this test used as a bare
    // literal before this derivation replaced it.
    let last_global = bytes
        .chunks_exact(CARD_LINE_BYTES)
        .rposition(|line| line[CARD_DATA_COLUMNS] == b'G')
        .expect("Global card");
    let offset = last_global * CARD_LINE_BYTES;
    bytes.splice(offset..offset, inserted);

    let error = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.as_slice()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(error, CodecError::Malformed(_)));
}

#[test]
fn inspect_recovers_a_terminate_count_from_the_card_census() {
    let mut bytes = card(b"original fixture", b'S', 1);
    bytes.extend(card(b"1H,,1H;,,;", b'G', 1));
    bytes.extend(card(b"S0000001G0000002D0000000P0000000", b'T', 1));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(summary.entries[1].attributes["cards"], "1");

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == crate::loss::IgesLossCode::CardFramingRecovered.kind())
            .count(),
        1,
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn inspect_accepts_space_padded_terminate_counts() {
    let mut bytes = card(b"original fixture", b'S', 1);
    bytes.extend(card(
        b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,1,1,1,1,1,,1,2,,1,1,13H240101.000000,0,0,,,11;",
        b'G',
        1,
    ));
    bytes.extend(card(b"S      1G      1D      0P      0", b'T', 1));

    IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
}

#[test]
fn decode_retains_post_terminate_physical_record() {
    let mut bytes = point_file();
    bytes.extend_from_slice(b"transport padding\r\n");

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["cards"].len(),
        8
    );
}

#[test]
fn terminate_card_remainder_is_retained_after_terminate() {
    let mut bytes = point_file();
    let line_end = bytes.len() - 1;
    bytes.insert(line_end, b'x');

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.as_slice()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    let post_terminate = summary
        .entries
        .iter()
        .find(|entry| entry.name == "post-terminate")
        .unwrap();
    assert_eq!(post_terminate.role, "retained-trailing-records");
    assert_eq!(post_terminate.attributes["records"], "1");

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["cards"].len(),
        8
    );
}

#[test]
fn decode_accepts_carriage_return_only_line_endings() {
    let bytes = point_file()
        .into_iter()
        .map(|byte| if byte == b'\n' { b'\r' } else { byte })
        .collect::<Vec<_>>();

    assert_eq!(IgesCodec.detect(&bytes), Confidence::High);

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}
