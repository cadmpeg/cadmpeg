// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::super::*;
use crate::container;
use crate::test_support::build_prt;
use crate::CreoCodec;

fn contour_payload() -> Vec<u8> {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0xe4; 4]);
    payload.push(0xe3);
    payload.extend_from_slice(&[0xe4; 4]);
    payload.push(0xe3);
    payload.extend_from_slice(&[0x82, 0x10, 0x01]);
    payload.extend_from_slice(&[0xe4, 0xe4, 0xe4, 0x34, 0xb8, 0x00]);
    payload.extend_from_slice(&[0xe3, 0xf7, 0x0f]);
    payload.extend_from_slice(&[0x82, 0x11, 0x02]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0xe4]);
    payload.push(0xe1);
    payload
}

fn inline_non_plane_contour_payload() -> Vec<u8> {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        0x0f, 0x2f, 0x00, 0x00, 0x12, 0x2f, 0x10, 0x00, 0x0f, 0x0f, 0x2f, 0x18, 0x00, 0xe4, 0xe4,
        0x2f, 0x20, 0x00, 0xe3,
    ]);
    payload.extend_from_slice(&[
        0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x10, 0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x2f, 0x10,
        0x00, 0x2f, 0x00, 0x00, 0xe3,
    ]);
    payload.extend_from_slice(&[0x82, 0x10, 0x01]);
    payload.extend_from_slice(&[0xe4, 0xe4, 0xe4, 0x34, 0xb8, 0x00]);
    payload.extend_from_slice(&[0xe3, 0xf7, 0x0f]);
    payload.extend_from_slice(&[0x82, 0x11, 0x02]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0xe4]);
    payload.push(0xe1);
    payload
}

#[test]
fn retains_complete_contour_chain_entries() {
    let payload = contour_payload();
    let records = contour_records(&payload);

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].surface_id, 7);
    assert_eq!(records[0].chain_index, 0);
    assert_eq!(records[0].curve_header_id, 0x210);
    assert_eq!(records[0].trv, 1);
    assert_eq!(
        records[0].parameter_envelope,
        [Some(1.0), Some(1.0), Some(1.0), None]
    );
    assert_eq!(records[0].separator_reference, Some(15));
    assert_eq!(
        records[0].body,
        [0x82, 0x10, 0x01, 0xe4, 0xe4, 0xe4, 0x34, 0xb8, 0x00, 0xe3]
    );
    assert_eq!(records[1].chain_index, 1);
    assert_eq!(records[1].curve_header_id, 0x211);
    assert_eq!(records[1].trv, 2);
    assert_eq!(
        records[1].parameter_envelope,
        [Some(0.0), Some(1.0), Some(0.0), Some(1.0)]
    );
    assert_eq!(records[1].separator_reference, None);
    assert_eq!(records[1].body.last(), Some(&0xe1));
}

#[test]
fn starts_an_inline_non_plane_contour_after_the_local_system_close() {
    let records = contour_records(&inline_non_plane_contour_payload());

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].surface_id, 7);
    assert_eq!(records[0].chain_index, 0);
    assert_eq!(records[1].chain_index, 1);
    assert_eq!(records[1].curve_header_id, 0x211);
}

#[test]
fn rejects_a_chain_without_its_terminal_marker() {
    let mut payload = contour_payload();
    payload.pop();

    assert!(contour_records(&payload).is_empty());
}

#[test]
fn rejects_an_undefined_traversal_byte() {
    let mut payload = contour_payload();
    let marker = payload
        .windows(3)
        .position(|bytes| bytes == [0x82, 0x10, 0x01])
        .unwrap();
    payload[marker + 2] = 0x04;

    assert!(contour_records(&payload).is_empty());
}

#[test]
fn container_and_native_arena_retain_contour_entries() {
    let data = build_prt("c", &[("VisibGeom", contour_payload())]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.surfaces.contours.len(), 2);
    assert_eq!(
        scan.surfaces.contours[0].surface_row_offset,
        scan.surfaces.rows[0].offset
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let contours = &result.ir().native.namespace("creo").unwrap().arenas["surface_contours"];
    assert_eq!(contours.len(), 2);
    assert_eq!(contours[0].fields()["surface_id"], 7);
    assert_eq!(contours[0].fields()["curve_header_id"], 0x210);
    assert_eq!(contours[0].fields()["separator_reference"], 15);
    assert_eq!(
        result.source_fidelity().annotations.provenance[contours[0].id()]
            .tag
            .as_deref(),
        Some("surface_contour_chain_entry")
    );
}
