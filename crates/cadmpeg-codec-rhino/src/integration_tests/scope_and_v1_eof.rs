// SPDX-License-Identifier: Apache-2.0
//! Public decoding derives class delivery from reached Rhino content parts.

use std::io::Cursor;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::{Codec, ContentScope, DecodeOptions, DecodeResult};

use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    class_wrapper, crc_chunk, eof, header, long_chunk, minimal_document, nested_crc_chunk,
    point_payload, short_chunk, table, POINT_CLASS,
};
use crate::RhinoCodec;

fn decode(bytes: &[u8], depth: DecodeDepth) -> DecodeResult {
    RhinoCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth,
                ..DecodeOptions::default()
            },
        )
        .expect("bounded source parts decode through the public entry")
}

fn modern(objects: &[Vec<u8>], history: Option<&[Vec<u8>]>) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut tables = vec![
        table(archive, 0x1000_0014, &[]),
        table(archive, 0x1000_0015, &[]),
        table(archive, 0x1000_0013, objects),
    ];
    if let Some(records) = history {
        tables.push(table(archive, 0x1000_0026, records));
    }
    minimal_document("80", &tables)
}

fn assert_scope(result: &DecodeResult, expected: ContentScope) {
    assert_eq!(result.ir().scope, expected);
    assert_eq!(result.report().scope(), expected);
    result
        .ir()
        .check_scope()
        .expect("populated arena invariant");
}

#[test]
fn an_empty_reached_object_table_holds_model_without_claiming_absent_design() {
    let bytes = modern(&[], None);
    for depth in [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Full,
    ] {
        let result = decode(&bytes, depth);
        assert_scope(&result, ContentScope::geometry());
        assert!(result.ir().model.points.is_empty());
        assert!(result.ir().model.features.is_empty());
        assert!(!result.report().geometry_transferred());
    }
}

#[test]
fn an_empty_reached_history_table_holds_design_even_on_a_container_request() {
    let bytes = modern(&[], Some(&[]));
    for depth in [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Full,
    ] {
        let result = decode(&bytes, depth);
        assert_scope(&result, ContentScope::full());
        assert!(result.ir().model.features.is_empty());
        assert!(!result.report().geometry_transferred());
    }
}

#[test]
fn a_failed_bounded_history_record_does_not_erase_the_reached_design_part() {
    let failed = crc_chunk(ArchiveVersion::V8, 0x2000_807b, b"invalid history class");
    let bytes = modern(&[], Some(&[failed]));
    for depth in [DecodeDepth::Container, DecodeDepth::Full] {
        let result = decode(&bytes, depth);
        assert_scope(&result, ContentScope::full());
        assert!(result.ir().model.features.is_empty());
        assert!(result.report().losses.iter().any(
            |loss| loss.message.contains("history record") && loss.message.contains("degraded")
        ));
    }
}

#[test]
fn a_bounded_object_history_part_survives_its_inner_parse_failure_for_scope() {
    let archive = ArchiveVersion::V8;
    let body = [
        short_chunk(archive, 0x8200_0071, 1),
        class_wrapper(archive, POINT_CLASS, &point_payload([1.0, 2.0, 3.0])),
        crc_chunk(archive, 0x0200_8074, &[]),
        short_chunk(archive, 0x8200_007f, 0),
    ]
    .concat();
    let object = nested_crc_chunk(archive, 0x2000_8070, &body);
    let bytes = modern(&[object], None);
    for depth in [DecodeDepth::Container, DecodeDepth::Full] {
        let result = decode(&bytes, depth);
        assert_scope(&result, ContentScope::full());
        assert!(result.ir().model.points.is_empty());
        assert!(result.ir().model.features.is_empty());
        assert!(!result.report().geometry_transferred());
    }
}

fn v1(points: &[[f64; 3]]) -> Vec<u8> {
    let mut bytes = header("1");
    bytes.extend(long_chunk(ArchiveVersion::V1, 1, b"legacy"));
    for point in points {
        let payload = point
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        bytes.extend(long_chunk(ArchiveVersion::V1, 0x0010_0001, &payload));
    }
    bytes
}

#[test]
fn v1_reached_geometry_holds_model_even_when_projection_is_not_requested() {
    let bytes = v1(&[[1.0, 2.0, 3.0]]);
    for depth in [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Full,
    ] {
        let result = decode(&bytes, depth);
        assert_scope(&result, ContentScope::geometry());
        if depth == DecodeDepth::Container {
            assert!(result.ir().model.points.is_empty());
            assert!(!result.report().geometry_transferred());
        } else {
            assert_eq!(result.ir().model.points.len(), 1);
            assert_eq!(
                result.ir().model.points[0].position().get(),
                cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            );
            assert!(result.report().geometry_transferred());
        }
    }
}

#[test]
fn v1_header_comment_and_terminator_alone_do_not_claim_a_content_class() {
    let mut bytes = v1(&[]);
    bytes.extend(short_chunk(ArchiveVersion::V1, 0x8000_7fff, 0));
    for depth in [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Full,
    ] {
        let result = decode(&bytes, depth);
        assert_scope(&result, ContentScope::container());
        assert!(!result.report().geometry_transferred());
    }
}

#[test]
fn both_v1_eof_forms_stop_before_later_geometry_and_unbounded_trailing_bytes() {
    for short in [false, true] {
        let mut bytes = v1(&[[1.0, 2.0, 3.0]]);
        if short {
            bytes.extend(short_chunk(ArchiveVersion::V1, 0x8000_7fff, 0));
        } else {
            let marker_len = eof(ArchiveVersion::V1, 0).len();
            bytes.extend(eof(ArchiveVersion::V1, bytes.len() + marker_len));
        }
        let trailing_point = [7.0_f64, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        bytes.extend(long_chunk(ArchiveVersion::V1, 0x0010_0001, &trailing_point));
        bytes.push(0xff);
        for depth in [
            DecodeDepth::Container,
            DecodeDepth::Geometry,
            DecodeDepth::Full,
        ] {
            let result = decode(&bytes, depth);
            assert_scope(&result, ContentScope::geometry());
            assert_eq!(result.source_fidelity().retained_records().len(), 1);
            if depth == DecodeDepth::Container {
                assert!(result.ir().model.points.is_empty());
            } else {
                assert_eq!(result.ir().model.points.len(), 1);
                assert_eq!(
                    result.ir().model.points[0].position().get(),
                    cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                );
            }
        }
    }
}
