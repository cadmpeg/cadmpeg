//! Terminal relation-carrier layout tests.

use super::super::super::{CLASS_MARKER, SKETCH_MARKER};
use super::super::*;
use crate::layout::{
    compact_indexed_curve_continuation120 as continuation,
    current_terminal_relation_carrier as terminal,
};

fn append_class_declaration(payload: &mut [u8], offset: usize, name: &[u8]) {
    payload[offset..offset + CLASS_MARKER.len()].copy_from_slice(CLASS_MARKER);
    payload[offset + 4..offset + 6].copy_from_slice(&(name.len() as u16).to_le_bytes());
    payload[offset + 6..offset + 6 + name.len()].copy_from_slice(name);
}

fn current_terminal_relation_payload() -> Vec<u8> {
    const CLASS: &[u8] = b"sgCircleDim";
    let mut payload = vec![0; terminal::LEN];
    payload[terminal::MARKER..terminal::MARKER + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    payload[terminal::NATIVE_KIND..terminal::NATIVE_KIND + 4].copy_from_slice(&2u32.to_le_bytes());
    payload[terminal::GEOMETRY_LOCUS..terminal::GEOMETRY_LOCUS + 4]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[terminal::ROLE..terminal::ROLE + 2].copy_from_slice(&1u16.to_le_bytes());
    payload[terminal::STATE..terminal::STATE + 2].copy_from_slice(&1u16.to_le_bytes());
    payload[terminal::SELECTOR..terminal::SELECTOR + 8]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[terminal::STATE_VALUE..terminal::STATE_VALUE + 8]
        .copy_from_slice(&1.0f64.to_le_bytes());
    payload[terminal::TERMINAL_HEADER..terminal::TERMINAL_HEADER + 4]
        .copy_from_slice(&[1, 0, 1, 0]);
    payload[terminal::ENDPOINT_SELECTOR..terminal::ENDPOINT_SELECTOR + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    payload[terminal::SIGNED_SELECTOR..terminal::SIGNED_SELECTOR + 8]
        .copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[terminal::TERMINAL_SELECTOR..terminal::TERMINAL_SELECTOR + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    for relative in (terminal::REFERENCE_SENTINELS..terminal::REFERENCE_SENTINELS + 16).step_by(4) {
        payload[relative..relative + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[terminal::TERMINAL_TAG..terminal::TERMINAL_TAG + 2]
        .copy_from_slice(&3u16.to_le_bytes());
    let class_offset = payload.len();
    payload.resize(class_offset + 6 + CLASS.len(), 0);
    append_class_declaration(&mut payload, class_offset, CLASS);
    payload
}

fn compact_continuation_relation_payload() -> Vec<u8> {
    const CLASS: &[u8] = b"sgPntPntDist";
    let mut payload = vec![0; continuation::LEN];
    payload[continuation::MARKER..continuation::MARKER + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    payload[continuation::NATIVE_KIND..continuation::NATIVE_KIND + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    payload[continuation::LOCUS..continuation::LOCUS + 4]
        .copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[continuation::ROLE..continuation::ROLE + 2].copy_from_slice(&1u16.to_le_bytes());
    payload[continuation::STATE..continuation::STATE + 2].copy_from_slice(&1u16.to_le_bytes());
    payload[continuation::SELECTOR..continuation::SELECTOR + 8]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[continuation::STATE_VALUE..continuation::STATE_VALUE + 8]
        .copy_from_slice(&1.0f64.to_le_bytes());
    payload[continuation::ENDPOINT_FIRST..continuation::ENDPOINT_FIRST + 2]
        .copy_from_slice(&1u16.to_le_bytes());
    payload[continuation::ENDPOINT_SECOND..continuation::ENDPOINT_SECOND + 2]
        .copy_from_slice(&2u16.to_le_bytes());
    payload[continuation::ENDPOINT_SELECTOR..continuation::ENDPOINT_SELECTOR + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    payload[continuation::SIGNED_SELECTOR..continuation::SIGNED_SELECTOR + 8]
        .copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[continuation::CONTINUATION_KIND..continuation::CONTINUATION_KIND + 2]
        .copy_from_slice(&8u16.to_le_bytes());
    let class_offset = payload.len();
    payload.resize(class_offset + 6 + CLASS.len(), 0);
    append_class_declaration(&mut payload, class_offset, CLASS);
    payload
}

#[test]
fn current_terminal_relation_carrier_returns_its_class_boundary() {
    let mut payload = current_terminal_relation_payload();
    assert_eq!(
        terminal_relation_class_offset(&payload, 0),
        Some(terminal::LEN)
    );

    payload[terminal::TERMINAL_TAG..terminal::TERMINAL_TAG + 2]
        .copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(terminal_relation_class_offset(&payload, 0), None);
}

#[test]
fn compact_continuation_relation_carrier_returns_its_class_boundary() {
    let mut payload = compact_continuation_relation_payload();
    assert_eq!(
        terminal_relation_class_offset(&payload, 0),
        Some(continuation::LEN)
    );

    payload[continuation::CONTINUATION_KIND..continuation::CONTINUATION_KIND + 2].fill(0);
    assert_eq!(terminal_relation_class_offset(&payload, 0), None);
}
