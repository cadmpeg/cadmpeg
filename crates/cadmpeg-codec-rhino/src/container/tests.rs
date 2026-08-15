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
use crate::settings;
use crate::test_support::test_dump::*;
use crate::wire::Uuid;
use crate::{RhinoCodec, MAGIC};

#[test]
fn document_table_record_budget_rejects_compact_record_amplification() {
    let archive = ArchiveVersion::V5;
    let records = [
        long_chunk(archive, 0x7000_0001, &[]),
        long_chunk(archive, 0x7000_0002, &[]),
    ];
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
            table(archive, 0x1000_0017, &records),
        ],
    );
    let error = crate::container::scan_with_test_record_limit(bytes, 1)
        .expect_err("record budget must fail before descriptor amplification");
    assert!(error.to_string().contains("table record budget"));
}

#[test]
fn near_budget_user_table_keeps_count_without_record_descriptors() {
    let archive = ArchiveVersion::V5;
    let records = (0..127)
        .map(|index| long_chunk(archive, 0x7000_0000 + index, &[]))
        .collect::<Vec<_>>();
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
            table(archive, 0x1000_0017, &records),
        ],
    );
    let scan =
        crate::container::scan_with_test_record_limit(bytes, 128).expect("near-budget user table");
    let user = scan.tables.last().expect("user table");
    assert_eq!(user.record_count, 127);
    assert!(user.records.is_empty());
    assert_eq!(scan.opaque_records.len(), 127);
}

#[test]
pub(crate) fn scans_metadata_tables_and_reports_offsets() {
    let archive = ArchiveVersion::V5;
    let object = object_record(archive, 0x20, [0; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert_eq!(summary.container_kind, "3dm-chunks");
    assert_eq!(summary.entries.len(), 4);
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
fn aggregates_object_classes_after_table_entries() {
    let archive = ArchiveVersion::V5;
    let first = object_record(archive, 1, [0; 16]);
    let second = object_record(archive, 2, [1; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[first, second]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert_eq!(summary.entries.len(), 5);
    assert_eq!(summary.entries[3].role, "object-class");
    assert_eq!(
        summary.entries[3].attributes.get("count"),
        Some(&"1".to_string())
    );
    assert_eq!(
        summary.entries[4].attributes.get("count"),
        Some(&"1".to_string())
    );
}

#[test]
fn container_only_returns_empty_current_ir_for_full_bands() {
    for version in ["50", "60", "70", "80"] {
        let archive = parse_header(&header(version))
            .expect("required invariant")
            .archive_version;
        let bytes = minimal_document(
            version,
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0013, &[]),
            ],
        );
        let result = RhinoCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    container_only: true,
                    ..Default::default()
                },
            )
            .expect("required invariant");
        assert_eq!(result.ir().ir_version(), IR_VERSION);
        assert!(result.ir().model.bodies.is_empty());
        assert!(result.ir().model.subds.is_empty());
        assert!(result.report().container_only);
        assert_eq!(result.report().format, "rhino");
    }
}

#[test]
fn container_only_returns_empty_current_ir_for_v3_and_v4() {
    for version in ["3", "4"] {
        let archive = parse_header(&header(version))
            .expect("required invariant")
            .archive_version;
        let bytes = minimal_document(
            version,
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0013, &[]),
            ],
        );
        let result = RhinoCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    container_only: true,
                    ..Default::default()
                },
            )
            .expect("required invariant");
        assert_eq!(result.ir().ir_version(), IR_VERSION);
        assert!(result.ir().model.bodies.is_empty());
        assert!(result.ir().model.subds.is_empty());
        assert!(result.report().container_only);
    }
}

#[test]
fn v2_class_records_use_four_byte_chunks_and_container_only_stays_empty() {
    let archive = ArchiveVersion::V2;
    let point =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let mut units_payload = 100_i32.to_le_bytes().to_vec();
    units_payload.extend(2_i32.to_le_bytes());
    units_payload.extend(0.01_f64.to_le_bytes());
    units_payload.extend(0.1_f64.to_le_bytes());
    units_payload.extend(0.001_f64.to_le_bytes());
    let bytes = minimal_document(
        "2",
        &[
            table(archive, 0x1000_0014, &[]),
            table(
                archive,
                0x1000_0015,
                &[crc_chunk(archive, 0x2000_8031, &units_payload)],
            ),
            table(archive, 0x1000_0013, &[point]),
        ],
    );

    let container_only = RhinoCodec
        .decode(
            &mut Cursor::new(bytes.clone()),
            &DecodeOptions {
                container_only: true,
                ..Default::default()
            },
        )
        .expect("V2 container-only decode");
    assert!(container_only.report().container_only);
    assert!(container_only.ir().model.points.is_empty());

    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("V2 class-record decode");
    assert_eq!(decoded.ir().model.points.len(), 1, "{:?}", decoded.report());
    assert_eq!(
        decoded.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    );
}

#[test]
fn header_only_bands_inspect_without_scanning_and_do_not_decode() {
    for version in ["5", "999"] {
        let bytes = header(version);
        let summary = RhinoCodec
            .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
            .expect("required invariant");
        assert!(summary.entries.is_empty());
        assert_eq!(summary.container_kind, "3dm-chunks");
        let result = RhinoCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..Default::default()
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
        RhinoCodec.inspect(&mut Cursor::new(missing), &InspectOptions::default()),
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
        RhinoCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()),
        Err(CodecError::Malformed(_))
    ));
}

#[test]
pub(crate) fn structural_framing_errors_keep_diagnostics() {
    let error = FramingError::Structural {
        offset: 42,
        message: "object record is missing object end".to_string(),
    };
    assert_eq!(
        error.to_string(),
        "framing error at 42: object record is missing object end"
    );
}

#[test]
fn requires_properties_settings_and_object_tables() {
    let archive = ArchiveVersion::V5;
    for tables in [
        vec![
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
        vec![
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
        vec![
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
        ],
    ] {
        let bytes = minimal_document("50", &tables);
        assert!(matches!(
            RhinoCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()),
            Err(CodecError::Malformed(message))
                if message.contains("properties, settings, and object tables")
        ));
    }
}

#[test]
fn crc_mismatch_is_a_summary_warning_and_later_record_survives() {
    let archive = ArchiveVersion::V5;
    let mut bad_object = object_record(archive, 0x08, [0; 16]);
    let crc_offset = bad_object.len() - 1;
    bad_object[crc_offset] ^= 1;
    let good_object = object_record(archive, 0x08, [0; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[bad_object, good_object]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("CRC mismatch")));
    assert_eq!(
        summary.entries[2].attributes.get("record_count"),
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
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
            table(archive, 0x1000_0017, &[]),
            table(archive, 0x1000_0017, &[]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert_eq!(summary.entries.len(), 5);
}

#[test]
fn obsolete_layerset_occupies_the_layer_group_compatibility_slot() {
    let archive = ArchiveVersion::V5;
    let valid = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0011, &[]),
            table(archive, 0x1000_0024, &[]),
            table(archive, 0x1000_0018, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    assert!(RhinoCodec
        .inspect(&mut Cursor::new(valid), &InspectOptions::default())
        .is_ok());

    let invalid = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0024, &[]),
            table(archive, 0x1000_0011, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    assert!(matches!(
        RhinoCodec.inspect(&mut Cursor::new(invalid), &InspectOptions::default()),
        Err(CodecError::Malformed(_))
    ));
}

#[test]
fn accepts_table_crc_with_its_declared_bound() {
    let archive = ArchiveVersion::V5;
    let bytes = minimal_document(
        "50",
        &[
            crc_table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("required invariant");
    assert_eq!(summary.entries.len(), 3);
}

#[test]
fn skips_short_and_long_unknown_table_records() {
    let archive = ArchiveVersion::V5;
    let short_object = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(
                archive,
                0x1000_0013,
                &[short_chunk(archive, 0x2000_8070, 0)],
            ),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(short_object), &InspectOptions::default())
        .expect("unknown short record is skipped");
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("unknown bounded record")));

    let unknown = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[long_chunk(archive, 0x1234, &[1])]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(unknown), &InspectOptions::default())
        .expect("required invariant");
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("unknown bounded record")));
}
