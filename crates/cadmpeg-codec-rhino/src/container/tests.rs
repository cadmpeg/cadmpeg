// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::IR_VERSION;

use crate::chunks::{parse_header, ArchiveVersion, FramingError, TCODE_ENDOFTABLE};
use crate::test_support::test_dump::*;
use crate::RhinoCodec;

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
    for version in ["50", "60", "70", "80", "90"] {
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
        assert!(result.report().container_only());
        assert_eq!(result.report().format(), "rhino");
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
        assert!(result.report().container_only());
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
    assert!(container_only.report().container_only());
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
fn archive_word_5_uses_the_four_byte_chunk_scan() {
    let archive = ArchiveVersion::LegacyV5;
    let bytes = minimal_document(
        "5",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[]),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .expect("archive word 5 uses the chunked scan");
    assert!(!summary.entries.is_empty());
    assert_eq!(
        summary
            .dialects()
            .as_ref()
            .expect("Rhino inspection reports dialect layers")
            .primary()
            .admission(),
        cadmpeg_core::dialect::Admission::Admitted
    );

    let decoded = RhinoCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..Default::default()
            },
        )
        .expect("archive word 5 reaches chunked container decode");
    assert!(decoded.report().container_only());
    assert_eq!(
        decoded
            .report()
            .dialects()
            .as_ref()
            .expect("Rhino decode reports dialect layers")
            .primary()
            .admission(),
        cadmpeg_core::dialect::Admission::Admitted
    );
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == crate::loss::RhinoLossCode::SourceDialectUnverified.kind() }));
}

#[test]
fn an_undeclared_archive_word_scans_and_reports_an_unverified_admission() {
    // The residual row runs the chunked route, not a header-only stop: the
    // scan reaches the tables and the report names the row whose strategy was
    // substituted.
    let archive = ArchiveVersion::from_word(100);
    let bytes = minimal_document(
        "100",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(
                archive,
                0x1000_0013,
                &[object_record(archive, 0x20, [0; 16])],
            ),
        ],
    );
    let summary = RhinoCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("an undeclared word scans like the band it shares a grammar with");
    assert!(
        !summary.entries.is_empty(),
        "the scan reached the table sequence"
    );
    let matched = summary
        .dialects()
        .as_ref()
        .expect("Rhino inspection reports dialect layers")
        .primary();
    assert_eq!(
        matched.admission(),
        cadmpeg_core::dialect::Admission::AdmittedUnverified {
            using: Some(cadmpeg_core::dialect::DialectId::pinned("rhino:archive-90",)),
        }
    );
    assert_eq!(matched.declared()["archive_version"], "100");
}

#[test]
fn an_undeclared_word_over_broken_framing_fails_structurally() {
    // The attempt is self-limiting: an undeclared word buys the chunked scan,
    // not a recovery of bytes the scan cannot frame.
    let error = RhinoCodec
        .decode(&mut Cursor::new(header("100")), &DecodeOptions::default())
        .expect_err("a header with no chunk sequence cannot be framed");
    assert!(
        !matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(CodecError::NotImplemented(_))
        ),
        "the residual row is attempted, so its failure is structural: {error}"
    );
}

#[test]
fn missing_end_of_table_marker_is_recoverable_warning() {
    let archive = ArchiveVersion::V5;
    let bytes = minimal_document(
        "50",
        &[
            long_chunk(archive, 0x1000_0014, &[]),
            long_chunk(archive, 0x1000_0015, &[]),
            long_chunk(archive, 0x1000_0013, &[]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("table boundary is sufficient");
    assert_eq!(
        scan.warnings
            .iter()
            .filter(|warning| warning.contains("has no end-of-table marker"))
            .count(),
        3
    );
}

#[test]
fn missing_end_of_file_and_wrong_table_order_remain_fatal() {
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
fn counted_view_list_crc_excludes_nested_children() {
    let archive = ArchiveVersion::V5;
    let child = crc_chunk(archive, 0x2000_813b, &[1, 2, 3]);
    let mut body = 1_i32.to_le_bytes().to_vec();
    let child_range = 4..4 + child.len();
    body.extend(child);
    body.extend(short_chunk(archive, TCODE_ENDOFTABLE, 0));
    let record = crc_chunk_excluding(
        archive,
        0x2000_8035,
        &body,
        std::slice::from_ref(&child_range),
    );

    assert_eq!(
        super::checksum_warning(&record, 0x2000_8035, 0, record.len(), archive)
            .expect("view-list checksum framing"),
        None
    );
}

#[test]
fn mesh_settings_crc_excludes_nested_subd_display_chunk() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x1f];
    body.extend([0; 111]);
    let child_range_start = body.len();
    body.extend(anonymous_chunk(archive, 3, &[4, 0, 0, 0, 2, 0, 0, 0, 1, 0]));
    let child_range = child_range_start..body.len();
    body.extend([0xde, 0xad]);
    for typecode in [0x2000_8032, 0x2000_8033] {
        let record =
            crc_chunk_excluding(archive, typecode, &body, std::slice::from_ref(&child_range));
        assert_eq!(
            super::checksum_warning(&record, typecode, 0, record.len(), archive)
                .expect("mesh-settings checksum framing"),
            None
        );
    }
}

#[test]
fn modern_render_settings_crc_excludes_anonymous_body() {
    let archive = ArchiveVersion::V6;
    let mut body = anonymous_chunk(archive, 3, &[0; 16]);
    let child_range = 0..body.len();
    body.extend([0xde, 0xad]);
    let record = crc_chunk_excluding(
        archive,
        0x2000_803d,
        &body,
        std::slice::from_ref(&child_range),
    );

    assert_eq!(
        super::checksum_warning(&record, 0x2000_803d, 0, record.len(), archive)
            .expect("render-settings checksum framing"),
        None
    );
}

#[test]
fn compressed_preview_crc_excludes_deflate_child() {
    let archive = ArchiveVersion::V5;
    let mut body = Vec::new();
    body.extend(40_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i16.to_le_bytes());
    body.extend(24_i16.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(4_u32.to_le_bytes());
    body.extend(0x1122_3344_u32.to_le_bytes());
    body.push(1);
    let child_start = body.len();
    body.extend(crc_chunk(archive, 0x4000_8000, &[0xde, 0xad, 0xbe, 0xef]));
    let child_range = child_start..body.len();
    let record = crc_chunk_excluding(
        archive,
        0x2000_8025,
        &body,
        std::slice::from_ref(&child_range),
    );

    assert_eq!(
        super::checksum_warning(&record, 0x2000_8025, 0, record.len(), archive)
            .expect("compressed-preview checksum framing"),
        None
    );
}

#[test]
fn compressed_preview_crc_tracks_noncontiguous_palette_and_image_buffers() {
    let archive = ArchiveVersion::V5;
    let mut body = Vec::new();
    body.extend(40_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i16.to_le_bytes());
    body.extend(1_i16.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(8_u32.to_le_bytes());
    body.extend(0x1122_3344_u32.to_le_bytes());
    body.push(0);
    body.extend([0xaa; 8]);
    body.extend(4_u32.to_le_bytes());
    body.extend(0x5566_7788_u32.to_le_bytes());
    body.push(1);
    let child_start = body.len();
    body.extend(crc_chunk(archive, 0x4000_8000, &[0xde, 0xad, 0xbe, 0xef]));
    let child_range = child_start..body.len();
    let record = crc_chunk_excluding(
        archive,
        0x2000_8025,
        &body,
        std::slice::from_ref(&child_range),
    );

    assert_eq!(
        super::checksum_warning(&record, 0x2000_8025, 0, record.len(), archive)
            .expect("non-contiguous preview checksum framing"),
        None
    );
}

#[test]
fn legacy_render_settings_crc_covers_direct_body() {
    let archive = ArchiveVersion::V5;
    let mut body = 103_i32.to_le_bytes().to_vec();
    body.extend([0; 12]);
    let record = crc_chunk(archive, 0x2000_803d, &body);

    assert_eq!(
        super::checksum_warning(&record, 0x2000_803d, 0, record.len(), archive)
            .expect("legacy render-settings checksum framing"),
        None
    );
}

#[test]
fn settings_attributes_crc_excludes_all_nested_children() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0x17];
    body.extend(1.0_f64.to_le_bytes());
    body.extend([1, 2, 3, 4]);
    body.extend(0_i32.to_le_bytes());
    body.extend((-1_i32).to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    let mut children = Vec::new();
    let child = anonymous_chunk(archive, 0, &[]);
    let start = body.len();
    body.extend(child);
    children.push(start..body.len());
    body.extend([0; 16]);
    body.extend([0; 24]);
    let child = anonymous_chunk(archive, 2, &[]);
    let start = body.len();
    body.extend(child);
    children.push(start..body.len());
    body.push(0);
    let child = anonymous_chunk(archive, 0, &[]);
    let start = body.len();
    body.extend(child);
    children.push(start..body.len());
    body.push(0x15);
    body.extend([0; 111]);
    let child = anonymous_chunk(archive, 3, &[]);
    let start = body.len();
    body.extend(child);
    children.push(start..body.len());
    body.extend([0; 16 * 6]);
    body.extend([0xde, 0xad]);
    let record = crc_chunk_excluding(archive, 0x2000_8134, &body, &children);

    assert_eq!(
        super::checksum_warning(&record, 0x2000_8134, 0, record.len(), archive)
            .expect("settings-attributes checksum framing"),
        None
    );
}

#[test]
fn plugin_list_crc_excludes_plugin_reference_chunks() {
    let archive = ArchiveVersion::V5;
    let child = anonymous_chunk(archive, 2, &[0xde, 0xad]);
    let mut body = vec![0x10];
    body.extend(1_i32.to_le_bytes());
    let child_start = body.len();
    body.extend(child);
    let child_range = child_start..body.len();
    body.extend([0xbe, 0xef]);
    let record = crc_chunk_excluding(
        archive,
        0x2000_8135,
        &body,
        std::slice::from_ref(&child_range),
    );

    assert_eq!(
        super::checksum_warning(&record, 0x2000_8135, 0, record.len(), archive)
            .expect("plugin-list checksum framing"),
        None
    );
}

#[test]
fn render_userdata_crc_excludes_userdata_and_class_end_chunks() {
    let archive = ArchiveVersion::V8;
    let class_uuid = [1_u8; 16];
    let item_uuid = [2_u8; 16];
    let application_uuid = [3_u8; 16];
    let mut transform = Vec::new();
    for index in 0..16 {
        let value: f64 = if index % 5 == 0 { 1.0 } else { 0.0 };
        transform.extend(value.to_le_bytes());
    }
    let header_body = [
        class_uuid.to_vec(),
        item_uuid.to_vec(),
        0_i32.to_le_bytes().to_vec(),
        transform,
        application_uuid.to_vec(),
        vec![0],
        60_i32.to_le_bytes().to_vec(),
        202_400_i32.to_le_bytes().to_vec(),
    ]
    .concat();
    let header = crc_chunk(archive, 0x0002_fff9, &header_body);
    let payload = anonymous_chunk(archive, 0, &[0x51, 0x52]);
    let userdata = long_chunk(
        archive,
        0x0002_7ffd,
        &[vec![0x22], header, payload].concat(),
    );
    let class_end = short_chunk(archive, 0x8002_7fff, 0);
    let mut body = Vec::new();
    let userdata_start = body.len();
    body.extend(userdata);
    let userdata_range = userdata_start..body.len();
    let class_end_start = body.len();
    body.extend(class_end);
    let class_end_range = class_end_start..body.len();
    body.extend([0xbe, 0xef]);
    let record = crc_chunk_excluding(
        archive,
        0x2000_8136,
        &body,
        &[userdata_range, class_end_range],
    );

    assert_eq!(
        super::checksum_warning(&record, 0x2000_8136, 0, record.len(), archive)
            .expect("render-settings userdata checksum framing"),
        None
    );
}

#[test]
fn user_table_uuid_crc_excludes_record_header() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0; 16];
    let header = crc_chunk(archive, 0x2000_8082, &[1, 5, 0, 0, 0, 202, 47, 31, 120]);
    let header_range = body.len()..body.len() + header.len();
    body.extend(header);
    body.extend([0xde, 0xad]);
    let record = crc_chunk_excluding(
        archive,
        0x2000_8080,
        &body,
        std::slice::from_ref(&header_range),
    );

    assert_eq!(
        super::checksum_warning(&record, 0x2000_8080, 0, record.len(), archive)
            .expect("user-table UUID checksum framing"),
        None
    );
}

#[test]
fn user_table_uuid_crc_covers_uuid_without_record_header() {
    let archive = ArchiveVersion::V5;
    let mut body = vec![0; 16];
    body.extend([0xbe, 0xef]);
    let record = crc_chunk(archive, 0x2000_8080, &body);

    assert_eq!(
        super::checksum_warning(&record, 0x2000_8080, 0, record.len(), archive)
            .expect("user-table UUID checksum framing"),
        None
    );
}

#[test]
fn historical_settings_record_is_bounded_and_retained_as_a_setting() {
    let archive = ArchiveVersion::V5;
    let record = crc_chunk(archive, 0x2000_803e, &[0; 24]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[record]),
            table(archive, 0x1000_0013, &[]),
        ],
    );

    let scan = crate::container::scan_owned(bytes).expect("historical setting framing");
    assert!(!scan
        .warnings
        .iter()
        .any(|warning| warning.contains("unknown bounded record 0x2000803e")));
    assert_eq!(scan.metadata.settings.unsupported.len(), 1);
    assert_eq!(scan.metadata.settings.unsupported[0].typecode, 0x2000_803e);
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
