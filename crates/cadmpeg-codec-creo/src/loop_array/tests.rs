// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::scan;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::build_prt;
use crate::CreoCodec;

fn prototype() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (name, value) in [
        (b"lo_id".as_slice(), 1),
        (b"lo_type".as_slice(), 2),
        (b"lo_subtype".as_slice(), 3),
        (b"feat_id".as_slice(), 4),
        (b"attributes".as_slice(), 5),
        (b"direction".as_slice(), 6),
        (b"next_lo_ptr".as_slice(), 7),
        (b"object_data".as_slice(), 8),
    ] {
        bytes.extend_from_slice(&[0xe0, 0x01]);
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&[0, value]);
    }
    bytes.extend_from_slice(&[0xf1, 0xf7, 0x2a, 0xe3]);
    bytes
}

fn frame(count: u8, rows: &[u8]) -> Vec<u8> {
    let mut bytes = b"lo_array\0\xf3\xf8".to_vec();
    bytes.extend_from_slice(&[count, 0xf7, 0x2a, 0xfb, 0xe3]);
    bytes.extend(prototype());
    bytes.extend_from_slice(rows);
    bytes.extend_from_slice(b"srf_array\0");
    bytes
}

fn bare_frame(count: u8, rows: &[u8]) -> Vec<u8> {
    let mut bytes = frame(count, rows);
    bytes.remove(b"lo_array\0".len());
    bytes
}

fn row(id: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id, 2, 3, 4, 5, 6, 7];
    bytes.extend_from_slice(body);
    bytes.push(0xe3);
    bytes
}

#[test]
fn retains_bounded_rows_and_frame_header() {
    let payload = frame(
        2,
        &[
            row(1, &[0xe2, 0x10]).as_slice(),
            row(8, &[0xe2, 0x11]).as_slice(),
        ]
        .concat(),
    );
    let parsed = scan(&payload);

    assert_eq!(parsed.frames.len(), 1);
    assert_eq!(parsed.frames[0].variant, Some(0xf3));
    assert_eq!(parsed.frames[0].declared_count, 2);
    assert_eq!(parsed.frames[0].class_id, 0x2a);
    assert_eq!(parsed.frames[0].materialized_count, 2);
    assert!(!parsed.frames[0].overfull);
    assert_eq!(parsed.records.len(), 2);
    assert_eq!(parsed.records[0].lo_id, 1);
    assert_eq!(parsed.records[0].feature_id, 4);
    assert_eq!(parsed.records[0].attributes, 5);
    assert_eq!(parsed.records[0].direction, 6);
    assert_eq!(parsed.records[0].next_lo_ptr, 7);
    assert_eq!(parsed.records[0].body, [0xe2, 0x10, 0xe3]);
    assert_eq!(parsed.records[1].lo_id, 8);
}

#[test]
fn retains_realistic_nested_row_body() {
    let payload = frame(
        1,
        &row(
            1,
            &[
                0xf8, 1, 0xf7, 0x34, 0xfb, 0xe2, 0xf7, 0x35, 4, 0xe2, 0x6a, 0xe1, 0, 0, 0, 0,
            ],
        ),
    );
    let parsed = scan(&payload);
    assert_eq!(parsed.records.len(), 1);
}

#[test]
fn retains_bare_frame_header_variant() {
    let parsed = scan(&bare_frame(1, &row(1, &[0xe2, 0x10])));

    assert_eq!(parsed.frames.len(), 1);
    assert_eq!(parsed.frames[0].variant, None);
    assert_eq!(parsed.records.len(), 1);
}

#[test]
fn keeps_materialized_rows_when_slots_are_sparse() {
    let payload = frame(3, &row(1, &[0xe2, 0x10]));
    let parsed = scan(&payload);

    assert_eq!(parsed.frames[0].declared_count, 3);
    assert_eq!(parsed.frames[0].materialized_count, 1);
    assert_eq!(parsed.records.len(), 1);
}

#[test]
fn withholds_an_overfull_frame() {
    let rows = [
        row(1, &[0xe2, 0x10]).as_slice(),
        row(8, &[0xe2, 0x11]).as_slice(),
    ]
    .concat();
    let parsed = scan(&frame(1, &rows));

    assert_eq!(parsed.frames.len(), 1);
    assert!(parsed.frames[0].overfull);
    assert_eq!(parsed.frames[0].materialized_count, 0);
    assert!(parsed.records.is_empty());
}

#[test]
fn rejects_truncated_prototype_and_row() {
    let mut missing_prototype = frame(1, &row(1, &[0xe2, 0x10]));
    missing_prototype.drain(20..27);
    assert!(scan(&missing_prototype).frames.is_empty());

    let mut truncated_row = frame(1, &[1, 2, 3, 4, 5, 6, 7, 0xe2]);
    truncated_row.truncate(truncated_row.len() - b"srf_array\0".len());
    assert!(scan(&truncated_row).records.is_empty());
}

#[test]
fn container_and_native_arenas_retain_loop_roster() {
    let payload = frame(1, &row(1, &[0xe2, 0x10]));
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.loop_arrays.frames.len(), 1);
    assert_eq!(scan.loop_arrays.records.len(), 1);
    assert_eq!(scan.loop_arrays.records[0].lo_id, 1);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let namespace = result.ir().native.namespace("creo").unwrap();
    assert_eq!(namespace.arenas["loop_array_frames"].len(), 1);
    let records = &namespace.arenas["loop_array_records"];
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fields()["lo_id"], 1);
    assert_eq!(records[0].fields()["next_lo_ptr"], 7);
    assert_eq!(
        result.source_fidelity().annotations.provenance[records[0].id()]
            .tag
            .as_deref(),
        Some("loop_array_record")
    );
}
