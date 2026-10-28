// SPDX-License-Identifier: Apache-2.0
//! Carrier-bounded auxiliary XT stream extraction.
#![allow(clippy::unwrap_used)]

use crate::container;
use crate::parasolid::ExtractedStream;
use crate::test_support::container::{make_block, outer_header};
use crate::test_support::parasolid::parasolid_with_body;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn stream() -> Vec<u8> {
    parasolid_with_body("auxiliary XT", "SCH_2900195_28101_13006", &[0, 1, 0, 1])
}

fn entry(stream: &[u8]) -> Vec<u8> {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend(u32::try_from(stream.len()).unwrap().to_le_bytes());
    bytes.extend(stream);
    bytes
}

fn scan(section: &str, payload: &[u8]) -> Result<Vec<ExtractedStream>, cadmpeg_core::CodecError> {
    let mut source = outer_header();
    source.extend(make_block(0x43, section, payload));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    Ok(container::scan(&ctx, root)?.blocks[0].ps_streams.clone())
}

#[test]
fn weldment_direct_entries_stop_at_declared_extents_before_outer_records() {
    let first = stream();
    let second = stream();
    let mut payload = entry(&first);
    payload.extend([0x5a; 11]);
    let second_start = payload.len() + 8;
    payload.extend(entry(&second));
    payload.extend([0x5a; 19]);
    let streams = scan("ThirdPty/WELDMENT_STORAGE", &payload).unwrap();
    assert_eq!(streams.len(), 2);
    assert_eq!(streams[0].offset, 8);
    assert_eq!(streams[1].offset, second_start);
    assert_eq!(streams[0].payload, first);
    assert_eq!(streams[1].payload, second);
}

#[test]
fn unrecognized_direct_carrier_keeps_its_existing_extraction_policy() {
    let first = stream();
    let mut payload = entry(&first);
    payload.extend([0x5a; 11]);
    let streams = scan("ThirdPty/Other", &payload).unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].payload, payload[8..]);
}

#[test]
fn weldment_entry_keeps_a_complete_header_inside_its_counted_value() {
    let inner = stream();
    let mut body = vec![0, 0x4f, 0xff];
    body.extend(u32::try_from(inner.len()).unwrap().to_be_bytes());
    body.extend([0, 2]);
    body.extend(inner);
    body.extend([0, 1, 0, 1]);
    let outer = parasolid_with_body("auxiliary XT", "SCH_2900195_28101_13006", &body);
    let streams = scan("ThirdPty/WELDMENT_STORAGE", &entry(&outer)).unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].payload, outer);
}

#[test]
fn weldment_entry_rejects_invalid_declared_extents_and_endpoint_wire() {
    let valid = entry(&stream());
    let mut zero = valid.clone();
    zero[4..8].copy_from_slice(&0_u32.to_le_bytes());
    let mut overflow = valid.clone();
    overflow[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut short = valid.clone();
    short[4..8].copy_from_slice(&8_u32.to_le_bytes());
    let mut bad_header = valid.clone();
    bad_header[12..14].copy_from_slice(&u16::MAX.to_be_bytes());
    let mut bad_terminal = valid.clone();
    *bad_terminal.last_mut().unwrap() = 2;
    for payload in [zero, overflow, short, bad_header, bad_terminal] {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
        assert!(crate::parasolid::extract_streams_in_section(
            &payload,
            Some("ThirdPty/WELDMENT_STORAGE"),
            &ctx
        )
        .is_err());
    }
}

#[test]
fn weldment_unknown_prefix_does_not_resurrect_the_generic_header_splitter() {
    let mut payload = entry(&stream());
    payload[..4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(scan("ThirdPty/WELDMENT_STORAGE", &payload)
        .unwrap()
        .is_empty());
    payload[..4].copy_from_slice(&1_u32.to_le_bytes());
    payload[8] = b'Q';
    assert!(scan("ThirdPty/WELDMENT_STORAGE", &payload)
        .unwrap()
        .is_empty());
}

#[test]
fn weldment_wrapped_sections_keep_the_existing_wrapper_extractor() {
    let xt = parasolid_with_body(
        "auxiliary XT",
        "SCH_2900195_28101_13006",
        &crate::test_support::parasolid::triangle_body(),
    );
    let (chained, _) = super::chained_payload(&[vec![xt.clone()]]);
    let nested = crate::test_support::parasolid::legacy_probe_payload(&xt, 0);
    for payload in [chained, nested] {
        let streams = scan("ThirdPty/WELDMENT_STORAGE", &payload).unwrap();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].payload, xt);
    }
}
