// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports, dead_code, clippy::disallowed_methods)]

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::IR_VERSION;

use crate::chunks::{
    anonymous_version, checked_count_bytes, chunk_at, crc16, packed_version, parse_eof,
    parse_header, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
    TCODE_CRC, TCODE_ENDOFFILE, TCODE_SHORT,
};
use crate::layout::endoffile_record_v50 as eof_v50;
use crate::layout::file_header;
use crate::layout::long_chunk_header_v2 as long_v2;
use crate::layout::long_chunk_header_v50 as long_v50;
use crate::settings;
use crate::test_support::test_dump::*;
use crate::wire::Uuid;
use crate::{RhinoCodec, MAGIC};

#[test]
fn detects_existing_magic_forms() {
    assert_eq!(RhinoCodec.detect(MAGIC), Confidence::High);
    assert_eq!(RhinoCodec.detect(&MAGIC[..MAGIC.len() - 1]), Confidence::No);
    let mut incorrect = MAGIC.to_vec();
    incorrect[3] = b'X';
    assert_eq!(RhinoCodec.detect(&incorrect), Confidence::No);
    let mut prefix = vec![0x00, 0x01, 0x02, 0x03];
    prefix.extend_from_slice(MAGIC);
    prefix.extend_from_slice(&[0x04, 0x05]);
    assert_eq!(RhinoCodec.detect(&prefix), Confidence::High);
}

#[test]
fn parses_exact_header_and_scope() {
    for (text, expected) in [
        ("1", ArchiveVersion::V1),
        ("2", ArchiveVersion::V2),
        ("3", ArchiveVersion::V3),
        ("4", ArchiveVersion::V4),
        ("5", ArchiveVersion::LegacyV5),
        ("50", ArchiveVersion::V5),
        ("60", ArchiveVersion::V6),
        ("70", ArchiveVersion::V7),
        ("80", ArchiveVersion::V8),
        ("90", ArchiveVersion::V9),
    ] {
        let parsed = parse_header(&header(text)).expect("valid header");
        assert_eq!(parsed.archive_version, expected);
    }
    assert!(parse_header(&header("0")).is_err());
    let mut invalid = header("50");
    invalid[file_header::ARCHIVE_VERSION] = b'0';
    assert!(matches!(
        parse_header(&invalid),
        Err(FramingError::InvalidHeader)
    ));
    invalid = header("50");
    invalid[31] = b' ';
    assert!(matches!(
        parse_header(&invalid),
        Err(FramingError::InvalidHeader)
    ));
    assert!(parse_header(&header("1234567")).is_ok());
    assert!(parse_header(&header("12345678")).is_ok());
    let mut embedded = vec![0x5a; 127];
    embedded.extend(header("80"));
    assert_eq!(
        parse_header(&embedded)
            .expect("embedded archive")
            .start_offset,
        127
    );
}

#[test]
fn parses_widths_short_long_and_bounds() {
    let short = (TCODE_SHORT | 7).to_le_bytes();
    let mut bytes = short.to_vec();
    bytes.extend(42_i32.to_le_bytes());
    let parsed =
        chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V4, false).expect("required invariant");
    assert!(parsed.short);
    assert_eq!(parsed.value, 42);
    assert_eq!(parsed.next_offset, long_v2::LEN);

    let bytes = long_chunk(ArchiveVersion::V4, 9, &[1, 2, 3]);
    let parsed =
        chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V4, false).expect("required invariant");
    assert_eq!(parsed.body, long_v2::LEN..11);
    assert_eq!(parsed.header_start, 0);
    assert_eq!(parsed.range(), 0..11);
    assert_eq!(parsed.next_offset, 11);

    let bytes = long_chunk(ArchiveVersion::V5, 9, &[1, 2, 3]);
    let parsed =
        chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V5, false).expect("required invariant");
    assert_eq!(parsed.body, long_v50::LEN..15);
    assert_eq!(parsed.header_start, 0);
    assert_eq!(parsed.range(), 0..15);
    assert_eq!(parsed.next_offset, 15);

    let mut bad = 9_u32.to_le_bytes().to_vec();
    bad.extend((-1_i64).to_le_bytes());
    let bodyless = chunk_at(&bad, 0, bad.len(), ArchiveVersion::V5, false)
        .expect("negative long value is bodyless");
    assert!(bodyless.short);
    assert_eq!(bodyless.body.len(), 0);
    let mut overflow = 9_u32.to_le_bytes().to_vec();
    overflow.extend(i32::MAX.to_le_bytes());
    assert!(matches!(
        chunk_at(&overflow, 0, overflow.len(), ArchiveVersion::V4, false),
        Err(FramingError::OutOfBounds { .. })
    ));
    assert!(chunk_at(&[9, 0, 0], 0, 3, ArchiveVersion::V4, false).is_err());
}

#[test]
fn verifies_crc_vectors_and_recoverable_mismatch() {
    assert_eq!(crc16(0, b""), 0);
    assert_eq!(crc16(1, b""), 1);
    assert_eq!(crc16(0, b"123456789"), 0xbeef);
    assert_eq!(crc32fast::hash(b""), 0);
    assert_eq!(crc32fast::hash(b"123456789"), 0xcbf4_3926);

    let body = b"body";
    let mut bytes = (TCODE_CRC | 9).to_le_bytes().to_vec();
    bytes.extend(((body.len() + 4) as i32).to_le_bytes());
    bytes.extend(body);
    bytes.extend(crc32fast::hash(body).to_le_bytes());
    let chunk =
        chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V2, false).expect("required invariant");
    assert_eq!(verify_checksum(&bytes, &chunk), Ok(ChecksumStatus::Valid));
    *bytes.last_mut().expect("required invariant") ^= 1;
    assert!(matches!(
        verify_checksum(&bytes, &chunk),
        Ok(ChecksumStatus::Mismatch { .. })
    ));

    assert_eq!(
        crate::chunks::checksum_kind(ArchiveVersion::V1, 0x0001_0000, false),
        crate::chunks::ChecksumKind::Crc16
    );
    assert_eq!(
        crate::chunks::checksum_kind(ArchiveVersion::V1, 0x0002_fffd, true),
        crate::chunks::ChecksumKind::Crc16
    );
}

#[test]
fn keeps_packed_and_anonymous_versions_distinct() {
    assert_eq!(packed_version(0x21), (2, 1));
    let bytes = [2, 0, 0, 0, 1, 0, 0, 0];
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    assert_eq!(
        anonymous_version(&mut reader).expect("required invariant"),
        (2, 1)
    );
}

#[test]
fn validates_eof_width_size_and_truncation() {
    for archive in [ArchiveVersion::V4, ArchiveVersion::V5] {
        let mut bytes = vec![0; file_header::LEN];
        let marker = eof(
            archive,
            file_header::LEN
                + 12
                + if archive.uses_eight_byte_values() {
                    16
                } else {
                    8
                },
        );
        bytes.extend(marker);
        let size = bytes.len();
        let marker_start = file_header::LEN;
        let replacement = eof(archive, size);
        bytes[marker_start..].copy_from_slice(&replacement);
        assert_eq!(
            parse_eof(&bytes, marker_start, archive)
                .expect("required invariant")
                .expect("required invariant")
                .file_size,
            size as u64
        );
        let mut mismatch = bytes.clone();
        let size_offset = marker_start
            + if archive.uses_eight_byte_values() {
                eof_v50::FILE_SIZE
            } else {
                8
            };
        mismatch[size_offset] ^= 1;
        assert_ne!(
            parse_eof(&mismatch, marker_start, archive)
                .expect("size is informational")
                .expect("EOF marker")
                .file_size,
            size as u64
        );
        assert!(parse_eof(&bytes[..bytes.len() - 1], marker_start, archive).is_err());
    }
    let bytes = vec![0; file_header::LEN];
    assert_eq!(
        parse_eof(&bytes, file_header::LEN, ArchiveVersion::V1).expect("required invariant"),
        None
    );
    assert!(matches!(
        parse_eof(&bytes, file_header::LEN, ArchiveVersion::V2),
        Err(FramingError::MissingEof)
    ));
}

#[test]
fn nested_bounds_and_unknown_skip_are_exact() {
    let child = long_chunk(ArchiveVersion::V5, 0x1234, &[9, 8, 7]);
    let sibling = long_chunk(ArchiveVersion::V5, 0x2345, &[1]);
    let mut parent = long_chunk(ArchiveVersion::V5, 0x1000, &child);
    parent.extend(sibling);
    let first =
        chunk_at(&parent, 0, parent.len(), ArchiveVersion::V5, false).expect("required invariant");
    let nested = chunk_at(
        &parent,
        first.body.start,
        first.body.end,
        ArchiveVersion::V5,
        false,
    )
    .expect("required invariant");
    assert_eq!(nested.next_offset, first.body.start + child.len());
    let next = chunk_at(
        &parent,
        first.next_offset,
        parent.len(),
        ArchiveVersion::V5,
        false,
    )
    .expect("required invariant");
    assert_eq!(next.typecode, 0x2345);
    assert!(matches!(
        chunk_at(
            &parent,
            first.body.start,
            first.body.start + child.len() - 1,
            ArchiveVersion::V5,
            false
        ),
        Err(FramingError::OutOfBounds { .. })
    ));
}

#[test]
fn checked_counts_never_allocate_from_invalid_values() {
    assert_eq!(
        checked_count_bytes(3, 4, 12, 100, 0).expect("required invariant"),
        12
    );
    assert!(checked_count_bytes(-1, 4, 12, 100, 0).is_err());
    assert!(checked_count_bytes(4, 4, 12, 100, 0).is_err());
    assert!(checked_count_bytes(3, 4, 12, 2, 0).is_err());
}

#[test]
fn bounded_reader_fixed_arrays_preserve_absolute_cursor_bounds() {
    let bytes = [9, 8, 7, 6, 5];
    let mut reader = BoundedReader::new(&bytes, 1, 5).expect("required invariant");
    assert_eq!(reader.array::<3>().expect("required invariant"), [8, 7, 6]);
    assert_eq!(reader.position(), 4);
    assert!(reader.array::<2>().is_err());
    assert_eq!(reader.position(), 4);
}

#[test]
fn bounded_reader_skips_a_valid_future_suffix() {
    let bytes = [9, 8, 7, 6, 5];
    let mut reader = BoundedReader::new(&bytes, 1, 5).expect("required invariant");
    assert_eq!(reader.u16().expect("required invariant"), 0x0708);
    assert_eq!(reader.skip_remaining().expect("required invariant"), 2);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn archive_boolean_strictness_uses_writer_version_encoding() {
    for writer_version in [Some(200_206_180), None] {
        let bytes = [2_u8];
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert!(reader
            .bool_with_writer_version(writer_version)
            .expect("legacy boolean is normalized"));
    }

    for writer_version in [Some(201_708_240), Some(2_348_836_140)] {
        let bytes = [2_u8];
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert!(matches!(
            reader.bool_with_writer_version(writer_version),
            Err(FramingError::Structural { .. })
        ));
    }

    let bytes = [2_u8];
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    assert_eq!(reader.u8().expect("raw character"), 2);
}

#[test]
fn top_level_framing_preserves_truncation_classification() {
    let bytes = header("50");
    let truncated = &bytes[..bytes.len() - 1];
    assert!(matches!(
        RhinoCodec.inspect(
            &mut Cursor::new(truncated),
            &InspectOptions::default()
        ),
        Err(CodecError::Truncated { location, context })
            if location.offset == 31 && context.operation == "rhino chunk framing"
    ));
}
