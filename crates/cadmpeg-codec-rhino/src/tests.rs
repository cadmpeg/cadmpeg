// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecError, Confidence, DecodeOptions};
use cadmpeg_ir::IR_VERSION;

use super::chunks::{
    anonymous_version, checked_count_bytes, chunk_at, crc16, packed_version, parse_eof,
    parse_header, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
    TCODE_CLASS_UUID, TCODE_CRC, TCODE_ENDOFFILE, TCODE_SHORT,
};
use super::{RhinoCodec, MAGIC};

fn header(version: &str) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    let mut field = [b' '; 8];
    let start = 8 - version.len();
    field[start..].copy_from_slice(version.as_bytes());
    bytes.extend(field);
    bytes
}

fn long_chunk(archive: ArchiveVersion, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    if archive.uses_eight_byte_values() {
        bytes.extend((body.len() as i64).to_le_bytes());
    } else {
        bytes.extend((body.len() as i32).to_le_bytes());
    }
    bytes.extend(body);
    bytes
}

fn crc_chunk(archive: ArchiveVersion, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(body).to_le_bytes());
    long_chunk(archive, typecode, &payload)
}

fn eof(archive: ArchiveVersion, file_size: usize) -> Vec<u8> {
    long_chunk(
        archive,
        TCODE_ENDOFFILE,
        &if archive.uses_eight_byte_values() {
            (file_size as u64).to_le_bytes().to_vec()
        } else {
            (file_size as u32).to_le_bytes().to_vec()
        },
    )
}

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
    ] {
        let parsed = parse_header(&header(text)).expect("valid header");
        assert_eq!(parsed.archive_version, expected);
    }
    assert!(parse_header(&header("0")).is_err());
    let mut invalid = header("50");
    invalid[24] = b'0';
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
    assert!(matches!(parse_header(&header("12345678")), Ok(_)));
}

#[test]
fn parses_widths_short_long_and_bounds() {
    let short = (TCODE_SHORT | 7).to_le_bytes();
    let mut bytes = short.to_vec();
    bytes.extend(42_i32.to_le_bytes());
    let parsed = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V4, false).unwrap();
    assert!(parsed.short);
    assert_eq!(parsed.value, 42);
    assert_eq!(parsed.next_offset, 8);

    let bytes = long_chunk(ArchiveVersion::V4, 9, &[1, 2, 3]);
    let parsed = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V4, false).unwrap();
    assert_eq!(parsed.body, 8..11);
    assert_eq!(parsed.next_offset, 11);

    let bytes = long_chunk(ArchiveVersion::V5, 9, &[1, 2, 3]);
    let parsed = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V5, false).unwrap();
    assert_eq!(parsed.body, 12..15);
    assert_eq!(parsed.next_offset, 15);

    let mut bad = 9_u32.to_le_bytes().to_vec();
    bad.extend((-1_i32).to_le_bytes());
    assert!(matches!(
        chunk_at(&bad, 0, bad.len(), ArchiveVersion::V4, false),
        Err(FramingError::InvalidLength { .. })
    ));
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
    assert_eq!(crc16(0, b"123456789"), 0x31c3);
    assert_eq!(crc32fast::hash(b""), 0);
    assert_eq!(crc32fast::hash(b"123456789"), 0xcbf4_3926);

    let body = b"body";
    let mut bytes = (TCODE_CRC | 9).to_le_bytes().to_vec();
    bytes.extend(((body.len() + 4) as i32).to_le_bytes());
    bytes.extend(body);
    bytes.extend(crc32fast::hash(body).to_le_bytes());
    let chunk = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V2, false).unwrap();
    assert_eq!(verify_checksum(&bytes, &chunk), Ok(ChecksumStatus::Valid));
    *bytes.last_mut().unwrap() ^= 1;
    assert!(matches!(
        verify_checksum(&bytes, &chunk),
        Ok(ChecksumStatus::Mismatch { .. })
    ));

    assert_eq!(
        super::chunks::checksum_kind(ArchiveVersion::V1, 0x0001_0000, false),
        super::chunks::ChecksumKind::Crc16
    );
    assert_eq!(
        super::chunks::checksum_kind(ArchiveVersion::V1, TCODE_CLASS_UUID, true),
        super::chunks::ChecksumKind::Crc16
    );
}

#[test]
fn keeps_packed_and_anonymous_versions_distinct() {
    assert_eq!(packed_version(0x21), (2, 1));
    let bytes = [2, 0, 0, 0, 1, 0, 0, 0];
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert_eq!(anonymous_version(&mut reader).unwrap(), (2, 1));
}

#[test]
fn validates_eof_width_size_and_truncation() {
    for archive in [ArchiveVersion::V4, ArchiveVersion::V5] {
        let mut bytes = vec![0; 32];
        let marker = eof(
            archive,
            32 + 12
                + if archive.uses_eight_byte_values() {
                    16
                } else {
                    8
                },
        );
        bytes.extend(marker);
        let size = bytes.len();
        let marker_start = 32;
        let replacement = eof(archive, size);
        bytes[marker_start..].copy_from_slice(&replacement);
        assert_eq!(
            parse_eof(&bytes, marker_start, archive)
                .unwrap()
                .unwrap()
                .file_size,
            size as u64
        );
        let mut mismatch = bytes.clone();
        let size_offset = marker_start
            + if archive.uses_eight_byte_values() {
                12
            } else {
                8
            };
        mismatch[size_offset] ^= 1;
        assert!(matches!(
            parse_eof(&mismatch, marker_start, archive),
            Err(FramingError::FileSizeMismatch { .. })
        ));
        assert!(parse_eof(&bytes[..bytes.len() - 1], marker_start, archive).is_err());
    }
    let bytes = vec![0; 32];
    assert_eq!(parse_eof(&bytes, 32, ArchiveVersion::V1).unwrap(), None);
    assert!(matches!(
        parse_eof(&bytes, 32, ArchiveVersion::V2),
        Err(FramingError::MissingEof)
    ));
}

#[test]
fn nested_bounds_and_unknown_skip_are_exact() {
    let child = long_chunk(ArchiveVersion::V5, 0x1234, &[9, 8, 7]);
    let sibling = long_chunk(ArchiveVersion::V5, 0x2345, &[1]);
    let mut parent = long_chunk(ArchiveVersion::V5, 0x1000, &child);
    parent.extend(sibling);
    let first = chunk_at(&parent, 0, parent.len(), ArchiveVersion::V5, false).unwrap();
    let nested = chunk_at(
        &parent,
        first.body.start,
        first.body.end,
        ArchiveVersion::V5,
        false,
    )
    .unwrap();
    assert_eq!(nested.next_offset, first.body.start + child.len());
    let next = chunk_at(
        &parent,
        first.next_offset,
        parent.len(),
        ArchiveVersion::V5,
        false,
    )
    .unwrap();
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
    assert_eq!(checked_count_bytes(3, 4, 12, 100, 0).unwrap(), 12);
    assert!(checked_count_bytes(-1, 4, 12, 100, 0).is_err());
    assert!(checked_count_bytes(4, 4, 12, 100, 0).is_err());
    assert!(checked_count_bytes(3, 4, 12, 2, 0).is_err());
}

fn short_chunk(archive: ArchiveVersion, typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | TCODE_SHORT).to_le_bytes().to_vec();
    if archive.uses_eight_byte_values() {
        bytes.extend(value.to_le_bytes());
    } else {
        bytes.extend((value as i32).to_le_bytes());
    }
    bytes
}

fn table(archive: ArchiveVersion, typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = records.concat();
    body.extend(short_chunk(archive, super::chunks::TCODE_ENDOFTABLE, 0));
    long_chunk(archive, typecode, &body)
}

fn minimal_document(version: &str, tables: &[Vec<u8>]) -> Vec<u8> {
    let archive = parse_header(&header(version)).unwrap().archive_version;
    let mut bytes = header(version);
    bytes.extend(long_chunk(archive, 1, b"comment"));
    for table in tables {
        bytes.extend(table);
    }
    let eof_offset = bytes.len();
    bytes.extend(eof(archive, 0));
    let marker = eof(archive, bytes.len());
    bytes[eof_offset..].copy_from_slice(&marker);
    bytes
}

#[test]
fn scans_metadata_tables_and_reports_offsets() {
    let archive = ArchiveVersion::V5;
    let object_type = short_chunk(archive, 0x82a0_0071, 0x20);
    let object = crc_chunk(archive, 0x2000_8070, &object_type);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let summary = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(summary.container_kind, "3dm-chunks");
    assert_eq!(summary.entries.len(), 3);
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "archive version 50"));
    assert_eq!(
        summary.entries[2].attributes.get("record_count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        summary.entries[2].attributes.get("object_typecode_0x20"),
        Some(&"1".to_string())
    );
}

#[test]
fn container_only_returns_empty_current_ir_for_full_bands() {
    for version in ["50", "60", "70", "80"] {
        let bytes = minimal_document(version, &[]);
        let result = RhinoCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    container_only: true,
                },
            )
            .unwrap();
        assert_eq!(result.ir.ir_version, IR_VERSION);
        assert!(result.ir.model.bodies.is_empty());
        assert!(result.ir.model.subds.is_empty());
        assert!(result.report.container_only);
        assert_eq!(result.report.format, "rhino");
    }
}

#[test]
fn header_only_bands_inspect_without_scanning_and_do_not_decode() {
    for version in ["1", "2", "5", "999"] {
        let bytes = header(version);
        let summary = RhinoCodec.inspect(&mut Cursor::new(bytes.clone())).unwrap();
        assert!(summary.entries.is_empty());
        assert_eq!(summary.container_kind, "3dm-chunks");
        let result = RhinoCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
            },
        );
        assert!(matches!(result, Err(CodecError::NotImplemented(_))));
    }
}

#[test]
fn requires_end_of_table_and_rejects_wrong_order() {
    let archive = ArchiveVersion::V5;
    let mut missing = header("50");
    missing.extend(long_chunk(archive, 1, b"comment"));
    missing.extend(long_chunk(archive, 0x1000_0014, &[]));
    assert!(matches!(
        RhinoCodec.inspect(&mut Cursor::new(missing)),
        Err(CodecError::Malformed(_))
    ));

    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0014, &[]),
        ],
    );
    assert!(matches!(
        RhinoCodec.inspect(&mut Cursor::new(bytes)),
        Err(CodecError::Malformed(_))
    ));
}

#[test]
fn crc_mismatch_is_a_summary_warning_and_later_record_survives() {
    let archive = ArchiveVersion::V5;
    let object_type = short_chunk(archive, 0x82a0_0071, 0x08);
    let mut bad_object = crc_chunk(archive, 0x2000_8070, &object_type);
    let crc_offset = bad_object.len() - 1;
    bad_object[crc_offset] ^= 1;
    let good_object = crc_chunk(archive, 0x2000_8070, &object_type);
    let bytes = minimal_document(
        "50",
        &[table(archive, 0x1000_0013, &[bad_object, good_object])],
    );
    let summary = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap();
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("CRC mismatch")));
    assert_eq!(
        summary.entries[0].attributes.get("record_count"),
        Some(&"2".to_string())
    );
}

#[test]
fn repeated_consecutive_user_tables_are_allowed() {
    let archive = ArchiveVersion::V5;
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0017, &[]),
            table(archive, 0x1000_0017, &[]),
        ],
    );
    let summary = RhinoCodec.inspect(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(summary.entries.len(), 3);
}
