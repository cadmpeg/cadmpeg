// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::EditableDecodeResult;

use super::contour_records;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

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

fn contour_chain_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Option<Vec<super::super::SurfaceContourRecord>>, CodecError> {
    let payload = contour_payload();
    let rows = super::with_decode_ctx(&payload, |ctx| super::super::rows(ctx, &payload));
    let start = payload
        .windows(3)
        .position(|bytes| bytes == [0x82, 0x10, 0x01])
        .expect("complete contour fixture");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &policy).expect("contour input admitted");
    super::super::parse_surface_contour_chain(
        &ctx,
        &payload,
        start,
        payload.len(),
        &rows[0],
        &crate::scalar::ScalarCache::default(),
    )
}

#[test]
fn contour_chain_refuses_first_entry_before_growth() {
    let error = contour_chain_with_limits(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo contour chain entries"), |cap| contour_chain_with_limits(cap, u64::MAX)), u64::MAX)
        .expect_err("first contour entry exceeds collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo contour chain entries"));
}

#[test]
fn contour_chain_refuses_second_entry_before_growth() {
    let payload = contour_payload();
    let rows = super::with_decode_ctx(&payload, |ctx| super::super::rows(ctx, &payload));
    let start = payload.windows(3).position(|bytes| bytes == [0x82, 0x10, 0x01]).expect("complete contour fixture");
    let error = crate::test_support::last_refusal_at(&payload, ResourceDimension::CollectionItems, "creo contour chain entries", |ctx| {
        super::super::parse_surface_contour_chain(ctx, &payload, start, payload.len(), &rows[0], &crate::scalar::ScalarCache::default())
    });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo contour chain entries"));
}

#[test]
fn contour_chain_refuses_body_before_retained_copy() {
    let error = contour_chain_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo contour chain body"),
            |cap| contour_chain_with_limits(u64::MAX, cap),
        ),
    )
    .expect_err("contour body exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo contour chain body"));
}

#[test]
fn contour_chain_refuses_aggregate_before_growth() {
    let payload = contour_payload();
    let rows = super::with_decode_ctx(&payload, |ctx| super::super::rows(ctx, &payload));
    let error = crate::test_support::last_refusal_at(&payload, cadmpeg_core::decode::ResourceDimension::CollectionItems, "creo contour record aggregation", |ctx| { super::super::contour_records_for_rows(ctx, &payload, &rows) });
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo contour record aggregation"));
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
    let scan = container::scan_bytes_ok(data.clone());

    assert_eq!(scan.surfaces.contours.len(), 2);
    assert_eq!(
        scan.surfaces.contours[0].surface_row_offset,
        scan.surfaces.rows[0].offset
    );

    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(&mut Cursor::new(data), &DecodeOptions::default())
            .expect("decode"),
    );
    let contours = &result.ir().native.namespace("creo").unwrap().arenas()["surface_contours"];
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

#[test]
fn incomplete_contour_chain_retains_no_bodies() {
    let mut payload = contour_payload();
    payload.pop();
    let rows = super::with_decode_ctx(&payload, |ctx| super::super::rows(ctx, &payload));
    let start = payload.windows(3).position(|bytes| bytes == [0x82, 0x10, 0x01])
        .expect("contour fixture");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).expect("root");
    let result = super::super::parse_surface_contour_chain(
        &ctx, &payload, start, payload.len(), &rows[0], &crate::scalar::ScalarCache::default(),
    ).expect("incomplete chain needs no retained storage");
    assert!(result.is_none());
}
