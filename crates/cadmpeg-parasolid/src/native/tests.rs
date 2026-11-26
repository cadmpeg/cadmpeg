// SPDX-License-Identifier: Apache-2.0
use super::{to_neutral, LEAD};
use crate::records::PrimaryRecords;

const DESCRIPTION: &[u8] = b": TRANSMIT FILE created by modeller version 3401176";

fn native_header(schema: &str, maximum: Option<u16>) -> Vec<u8> {
    let mut out = vec![LEAD];
    out.extend_from_slice(&u16::try_from(DESCRIPTION.len()).unwrap().to_le_bytes());
    out.extend_from_slice(DESCRIPTION);
    out.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_le_bytes());
    out.extend_from_slice(schema.as_bytes());
    if let Some(maximum) = maximum {
        out.extend_from_slice(&maximum.to_le_bytes());
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    out
}

fn neutral_header(schema: &str, maximum: Option<u16>) -> Vec<u8> {
    let mut out = b"PS".to_vec();
    out.extend_from_slice(&u32::try_from(DESCRIPTION.len()).unwrap().to_be_bytes());
    out.extend_from_slice(DESCRIPTION);
    out.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_be_bytes());
    out.extend_from_slice(schema.as_bytes());
    if let Some(maximum) = maximum {
        out.extend_from_slice(&maximum.to_be_bytes());
    }
    out.extend_from_slice(&0_u32.to_be_bytes());
    out
}

/// A POINT node (type 29) with node id 7, null pointers, and `pvec`.
fn native_point(index: u32) -> Vec<u8> {
    let mut out = 29_u16.to_le_bytes().to_vec();
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&7_u32.to_le_bytes());
    for _ in 0..4 {
        out.extend_from_slice(&0_u32.to_le_bytes());
    }
    for value in [1.0_f64, 2.0, 3.0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn neutral_point(index: &[u8]) -> Vec<u8> {
    let mut out = 29_u16.to_be_bytes().to_vec();
    out.extend_from_slice(index);
    out.extend_from_slice(&7_u32.to_be_bytes());
    for _ in 0..4 {
        out.extend_from_slice(&[0, 1]);
    }
    for value in [1.0_f64, 2.0, 3.0] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out
}

const NATIVE_TERMINATOR: [u8; 6] = [1, 0, 0, 0, 0, 0];
const NEUTRAL_TERMINATOR: [u8; 4] = [0, 1, 0, 1];

#[test]
fn a_direct_schema_node_is_rewritten_in_neutral_order() {
    let schema = "SCH_3401176_13006";
    let native = [
        native_header(schema, None),
        native_point(1),
        NATIVE_TERMINATOR.to_vec(),
    ]
    .concat();
    let neutral = [
        neutral_header(schema, None),
        neutral_point(&[0, 2]),
        NEUTRAL_TERMINATOR.to_vec(),
    ]
    .concat();
    assert_eq!(to_neutral(&native).unwrap(), neutral);
}

#[test]
fn a_wide_node_index_takes_the_two_word_form() {
    let schema = "SCH_3401176_13006";
    let native = [
        native_header(schema, None),
        native_point(40_000),
        NATIVE_TERMINATOR.to_vec(),
    ]
    .concat();
    let mut wide = Vec::new();
    crate::pointer::write(&mut wide, 40_001).unwrap();
    assert_eq!(wide.len(), 4);
    let neutral = [
        neutral_header(schema, None),
        neutral_point(&wide),
        NEUTRAL_TERMINATOR.to_vec(),
    ]
    .concat();
    assert_eq!(to_neutral(&native).unwrap(), neutral);
}

#[test]
fn an_embedded_declaration_is_rewritten_and_frames() {
    let schema = "SCH_3401176_34101_13006";
    let mut native = native_header(schema, Some(231));
    native.extend_from_slice(&29_u16.to_le_bytes());
    native.push(7);
    native.extend_from_slice(b"CCCCCCA\x05extra");
    native.extend_from_slice(&0_u16.to_le_bytes());
    native.extend_from_slice(&0_u32.to_le_bytes());
    native.extend_from_slice(b"\x01dZ");
    native.extend_from_slice(&native_point(1)[2..]);
    native.extend_from_slice(&9_u32.to_le_bytes());
    native.extend_from_slice(&NATIVE_TERMINATOR);

    let neutral = to_neutral(&native).unwrap();
    let records = PrimaryRecords::read(&neutral).unwrap().unwrap();
    assert!(records.is_terminated());
    assert_eq!(records.records().len(), 1);
    assert!(neutral.ends_with(&[0, 0, 0, 9, 0, 1, 0, 1]));
}

#[test]
fn a_non_null_terminator_index_is_refused() {
    let schema = "SCH_3401176_13006";
    let native = [native_header(schema, None), vec![1, 0, 2, 0, 0, 0]].concat();
    assert!(to_neutral(&native).is_err());
}

#[test]
fn a_neutral_transmit_is_refused() {
    let neutral = [
        neutral_header("SCH_3401176_13006", None),
        NEUTRAL_TERMINATOR.to_vec(),
    ]
    .concat();
    assert!(to_neutral(&neutral).is_err());
}
