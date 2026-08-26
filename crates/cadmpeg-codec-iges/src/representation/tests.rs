// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};

use crate::test_support::*;
use crate::IgesCodec;

#[derive(Debug)]
struct ShortReader {
    inner: Cursor<Vec<u8>>,
    maximum_read: usize,
}

impl Read for ShortReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = buffer.len().min(self.maximum_read);
        self.inner.read(&mut buffer[..length])
    }
}

impl Seek for ShortReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[test]
fn fixed_ascii_detection_requires_two_consistent_cards() {
    let mut valid = card(b"generated fixture", b'S', 1);
    valid.extend(card(b"", b'G', 1));
    assert_eq!(IgesCodec.detect(&valid), Confidence::High);

    assert_eq!(IgesCodec.detect(&valid[..81]), Confidence::No);

    let mut arbitrary = vec![b'x'; 72];
    arbitrary.extend_from_slice(b"S      1\nsecond line\n");
    assert_eq!(IgesCodec.detect(&arbitrary), Confidence::No);
}

#[test]
fn fixed_ascii_detection_allows_eight_bit_data_fields() {
    let mut valid = card(b"generated fixture", b'S', 1);
    valid.extend(card(&[0x80, 0xff], b'G', 1));

    assert_eq!(IgesCodec.detect(&valid), Confidence::High);
}

#[test]
fn representation_classification_fills_its_prefix_across_short_reads() {
    let bytes = point_file();
    let mut reader = ShortReader {
        inner: Cursor::new(bytes),
        maximum_read: 7,
    };

    assert_eq!(
        crate::representation::classify(&mut reader).unwrap(),
        crate::representation::Representation::FixedAscii
    );
    assert_eq!(reader.stream_position().unwrap(), 0);
}

#[test]
fn compressed_and_binary_representations_are_detected_and_binary_is_validated() {
    let mut compressed = vec![b' '; 80];
    compressed[72] = b'C';
    assert_eq!(IgesCodec.detect(&compressed), Confidence::High);
    let inspect_error = IgesCodec
        .inspect(
            &mut Cursor::new(compressed.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();
    assert_eq!(
        inspect_error.to_string(),
        "malformed container: IGES Compressed ASCII: Start section is missing after the flag record"
    );
    assert_eq!(
        IgesCodec
            .decode(&mut Cursor::new(compressed), &DecodeOptions::default())
            .unwrap_err()
            .to_string(),
        "malformed container: IGES Compressed ASCII: Start section is missing after the flag record"
    );

    let mut binary = vec![0_u8; 80];
    binary[0] = b'B';
    binary[1..5].copy_from_slice(&75_u32.to_be_bytes());
    for (offset, identifier) in [
        (11, b'B'),
        (16, b'S'),
        (21, b'G'),
        (26, b'D'),
        (31, b'P'),
        (36, b'T'),
    ] {
        binary[offset] = identifier;
    }
    binary[72] = b'B';
    binary[73..79].fill(b'0');
    binary[79] = b'1';
    assert_eq!(IgesCodec.detect(&binary), Confidence::High);
    let inspect_error = IgesCodec
        .inspect(
            &mut Cursor::new(binary.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap_err();
    assert!(inspect_error
        .to_string()
        .contains("malformed container: IGES Binary: Binary primitive bit lengths"));
    let error = IgesCodec
        .decode(&mut Cursor::new(binary), &DecodeOptions::default())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("malformed container: IGES Binary: Binary primitive bit lengths"));
}

#[test]
fn representation_detection_rejects_malformed_flag_constants() {
    let mut compressed = vec![b' '; 80];
    compressed[72] = b'C';
    compressed[4] = b'\n';
    assert_eq!(IgesCodec.detect(&compressed), Confidence::No);

    let mut binary = vec![0_u8; 80];
    binary[0] = b'B';
    binary[1..5].copy_from_slice(&75_u32.to_be_bytes());
    for (offset, identifier) in [
        (11, b'B'),
        (16, b'S'),
        (21, b'G'),
        (26, b'D'),
        (31, b'P'),
        (36, b'T'),
    ] {
        binary[offset] = identifier;
    }
    binary[72] = b'B';
    binary[73..79].fill(b' ');
    binary[79] = b'1';

    for offset in [11, 16, 21, 26, 31, 36, 72, 79] {
        let mut malformed = binary.clone();
        malformed[offset] ^= 1;
        assert_eq!(
            IgesCodec.detect(&malformed),
            Confidence::No,
            "offset {offset}"
        );
    }

    let mut little_endian_count = binary.clone();
    little_endian_count[1..5].copy_from_slice(&75_u32.to_le_bytes());
    assert_eq!(IgesCodec.detect(&little_endian_count), Confidence::No);

    let mut malformed_tail = binary;
    malformed_tail[75] = b'X';
    assert_eq!(IgesCodec.detect(&malformed_tail), Confidence::No);
}

#[test]
fn detection_reads_the_second_card_image_from_a_fused_first_line() {
    let base = point_file();
    let mut fused = base[..CARD_COLUMNS].to_vec();
    fused.extend_from_slice(&base[CARD_LINE_BYTES..CARD_LINE_BYTES + CARD_COLUMNS]);
    fused.extend_from_slice(&base[CARD_LINE_BYTES + CARD_COLUMNS..]);

    assert_eq!(IgesCodec.detect(&fused), Confidence::High);

    let result = IgesCodec
        .decode(&mut Cursor::new(fused), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
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
fn detection_refuses_a_start_card_with_no_readable_sequence() {
    let mut bytes = point_file();
    bytes[CARD_DATA_COLUMNS + 1..CARD_COLUMNS].fill(b' ');

    assert_eq!(IgesCodec.detect(&bytes), Confidence::No);
    assert_eq!(
        IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err()
            .to_string(),
        "not the expected format: unrecognized IGES representation"
    );
}
