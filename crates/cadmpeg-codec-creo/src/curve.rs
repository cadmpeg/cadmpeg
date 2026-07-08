// SPDX-License-Identifier: Apache-2.0
//! Typed discovery of labeled `crv_array` prototype rows.

use crate::psb::{compact_int, reference_id};

/// A labeled curve prototype. `type_byte` is retained verbatim: its geometric
/// interpretation belongs to the curve-body evaluator, not the namespace
/// grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurvePrototype {
    pub id: u32,
    pub type_byte: u8,
    pub feature_id: Option<u32>,
    pub offset: usize,
}

/// A positional curve row whose terminal topology suffix was uniquely decoded.
/// `faces` and `next_edges` retain the two native half-edge sides in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveTopologyRow {
    pub id: u32,
    pub type_byte: u8,
    pub feature_id: u32,
    pub directions: [u8; 2],
    pub faces: [u32; 2],
    pub next_edges: [u32; 2],
    pub offset: usize,
}

/// Discover every labeled `crv_array` prototype. A label range ends at the
/// following `crv_array` label, so DEPDB-concatenated namespaces remain
/// independent.
pub fn prototypes(payload: &[u8]) -> Vec<CurvePrototype> {
    let mut result = Vec::new();
    let mut start = 0;
    while let Some(relative) = find(payload, b"crv_array\0", start) {
        let section_start = relative;
        start = relative + b"crv_array\0".len();
        let section_end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        let Some(id_label) = find_in(payload, b"crv_id\0", start, section_end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (id, id_end) = compact_int(payload, id_start);
        if id_end == id_start {
            continue;
        }
        let Some(type_label) = find_in(payload, b"type\0", id_end, section_end) else {
            continue;
        };
        let Some(&type_byte) = payload.get(type_label + b"type\0".len()) else {
            continue;
        };
        let feature_id = find_in(payload, b"feat_id\0", id_end, section_end).and_then(|label| {
            let value_start = label + b"feat_id\0".len();
            let (value, end) = compact_int(payload, value_start);
            (end != value_start).then_some(value)
        });
        result.push(CurvePrototype {
            id,
            type_byte,
            feature_id,
            offset: section_start,
        });
    }
    result
}

/// Decode positional `crv_array` rows whose terminal
/// `<four canonical reference IDs> 00 00 e3 e1 e3` suffix has exactly one
/// possible boundary. Rows with ambiguous or malformed suffixes are not
/// returned; callers must preserve their enclosing section as unknown data.
pub fn topology_rows(payload: &[u8]) -> Vec<CurveTopologyRow> {
    let Some(label) = find(payload, b"topol_ref_data\0", 0) else {
        return Vec::new();
    };
    let mut cursor = label + b"topol_ref_data\0".len();
    let mut rows = Vec::new();
    while cursor < payload.len() {
        let Some(relative_term) = find(payload, b"\xe1\xe3", cursor) else {
            break;
        };
        let term = relative_term;
        let next = term + 2;
        let Some(row) = parse_topology_row(&payload[cursor..term], cursor) else {
            cursor = next;
            continue;
        };
        rows.push(row);
        cursor = next;
    }
    rows
}

fn parse_topology_row(row: &[u8], absolute_offset: usize) -> Option<CurveTopologyRow> {
    let (id, after_id) = compact_int(row, 0);
    let type_byte = *row.get(after_id)?;
    let (feature_id, after_feature) = compact_int(row, after_id + 1);
    let directions = [*row.get(after_feature)?, *row.get(after_feature + 1)?];
    let close = row.len().checked_sub(3)?;
    (row.get(close..)? == [0, 0, 0xe3]).then_some(())?;
    let mut candidates = Vec::new();
    for length in 4..=11 {
        let Some(start) = close.checked_sub(length) else {
            continue;
        };
        let Ok((f0, p1)) = reference_id(row, start) else {
            continue;
        };
        let Ok((f1, p2)) = reference_id(row, p1) else {
            continue;
        };
        let Ok((e0, p3)) = reference_id(row, p2) else {
            continue;
        };
        let Ok((e1, end)) = reference_id(row, p3) else {
            continue;
        };
        if end == close && start >= after_feature + 2 {
            candidates.push([f0, f1, e0, e1]);
        }
    }
    (candidates.len() == 1).then_some(())?;
    let [f0, f1, e0, e1] = candidates[0];
    Some(CurveTopologyRow {
        id,
        type_byte,
        feature_id,
        directions,
        faces: [f0, f1],
        next_edges: [e0, e1],
        offset: absolute_offset,
    })
}

fn find(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

fn find_in(data: &[u8], needle: &[u8], from: usize, end: usize) -> Option<usize> {
    data.get(from..end)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_labeled_prototypes_in_concatenated_namespaces() {
        let payload = b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04\
                       crv_array\0crv_id\0\x80\x80type\0\x01";
        assert_eq!(
            prototypes(payload),
            vec![
                CurvePrototype {
                    id: 7,
                    type_byte: 8,
                    feature_id: Some(4),
                    offset: 0,
                },
                CurvePrototype {
                    id: 128,
                    type_byte: 1,
                    feature_id: None,
                    offset: 33,
                },
            ]
        );
    }

    #[test]
    fn ignores_incomplete_labeled_rows() {
        assert!(prototypes(b"crv_array\0crv_id\0\x07").is_empty());
    }

    #[test]
    fn decodes_a_uniquely_delimited_topology_suffix() {
        let payload = [
            b't', b'o', b'p', b'o', b'l', b'_', b'r', b'e', b'f', b'_', b'd', b'a', b't', b'a', 0,
            7, 8, 4, 1, 0xf6, 0x29, 0x43, 0, // opaque row body
            10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3,
        ];
        assert_eq!(
            topology_rows(&payload),
            vec![CurveTopologyRow {
                id: 7,
                type_byte: 8,
                feature_id: 4,
                directions: [1, 0xf6],
                faces: [10, 11],
                next_edges: [7, 7],
                offset: 15,
            }]
        );
    }
}
