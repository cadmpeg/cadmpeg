// SPDX-License-Identifier: Apache-2.0
//! Bare records whose pointers take the four-byte form past `0x7ffe` (spec §4).

use super::{
    bridge_end, parse_bridge, parse_coedge_candidates, parse_edge_use_candidates,
    parse_loop_candidates, parse_point, parse_tolerant_edge_use_candidates,
    parse_vertex_candidates, EdgeReferences, MAGIC,
};
use cadmpeg_ir::topology::Sense;

/// A pointer value that takes the four-byte form and has a u16 key.
const WIDE: u32 = 0x9000;

/// A pointer value that takes the four-byte form and has no u16 key.
const PAST_KEY: u32 = 0x1_2345;

/// The §4 pointer cell of `value`.
fn pointer(value: u32) -> Vec<u8> {
    if value <= 0x7ffe {
        return u16::try_from(value).unwrap().to_be_bytes().to_vec();
    }
    let low = 0x8000 | u16::try_from(value & 0x7fff).unwrap();
    let high = u16::try_from(value >> 15).unwrap();
    [low.to_be_bytes(), high.to_be_bytes()].concat()
}

fn record(tag: u8, cells: &[&[u8]]) -> Vec<u8> {
    let mut bytes = vec![0, tag];
    for cell in cells {
        bytes.extend_from_slice(cell);
    }
    bytes
}

fn key(value: u32) -> u16 {
    u16::try_from(value).unwrap()
}

#[test]
fn a_bare_loop_reads_a_wide_attr_and_first_fin() {
    let bytes = record(
        0x0f,
        &[
            &pointer(WIDE),
            &7_u32.to_be_bytes(),
            &pointer(1),
            &pointer(WIDE + 1),
            &pointer(20),
            &pointer(1),
        ],
    );
    let readings = parse_loop_candidates(&bytes, 0);
    let (record, end) = &readings[0];
    assert_eq!(record.attr, key(WIDE));
    assert_eq!(record.refs, [1, key(WIDE + 1), 20, 1]);
    assert_eq!(*end, bytes.len());
}

#[test]
fn a_bare_fin_reads_wide_pointers_before_its_sense() {
    let refs = [1, WIDE, 31, WIDE + 2, 40, WIDE + 3, 50, 1, 1];
    let cells: Vec<Vec<u8>> = std::iter::once(pointer(30))
        .chain(refs.iter().map(|value| pointer(*value)))
        .chain(std::iter::once(vec![0x2b]))
        .collect();
    let bytes = record(0x11, &cells.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let readings = parse_coedge_candidates(&bytes, 0);
    let (record, end) = &readings[0];
    assert_eq!(record.attr, 30);
    assert_eq!(record.refs, refs.map(key));
    assert_eq!(record.sense, Sense::Forward);
    assert_eq!(*end, bytes.len());
}

#[test]
fn a_bare_face_moves_its_tolerance_refs_and_sense_by_each_wide_pointer() {
    let refs = [WIDE, 3, WIDE + 4, 5, 6];
    let mut cells = vec![
        pointer(WIDE + 5),
        9_u32.to_be_bytes().to_vec(),
        pointer(WIDE + 6),
        MAGIC.to_vec(),
    ];
    cells.extend(refs.iter().map(|value| pointer(*value)));
    cells.push(vec![0x2d]);
    let mut bytes = record(0x0e, &cells.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let end = bytes.len();
    bytes.extend_from_slice(&[0; 10]);
    let bridge = parse_bridge(&bytes, 0, false).unwrap();
    assert_eq!(bridge.attr, key(WIDE + 5));
    assert_eq!(bridge.sequence, 9);
    assert_eq!(bridge.attributes_head, Some(key(WIDE + 6)));
    assert_eq!(bridge.refs, refs.map(key));
    assert_eq!(bridge.sense, Sense::Reversed);
    assert_eq!(bridge_end(&bytes, 0, false), Some(end));
}

fn edge(owner: u32) -> Vec<u8> {
    let mut cells = vec![
        pointer(WIDE + 7),
        11_u32.to_be_bytes().to_vec(),
        pointer(1),
        MAGIC.to_vec(),
    ];
    cells.extend(EDGE_REFS.iter().map(|value| pointer(*value)));
    cells.push(pointer(owner));
    record(0x10, &cells.iter().map(Vec::as_slice).collect::<Vec<_>>())
}

const EDGE_REFS: [u32; 6] = [41, 1, 1, WIDE, 1, 1];

fn bare_edges(bytes: &[u8]) -> Vec<(super::EdgeUse, usize)> {
    parse_edge_use_candidates(bytes, 0)
        .into_iter()
        .filter(|(record, _)| matches!(record.references, EdgeReferences::Bare(_)))
        .collect()
}

#[test]
fn a_bare_edge_ends_after_a_wide_owner() {
    let bytes = edge(WIDE + 20);
    let readings = bare_edges(&bytes);
    let (record, end) = &readings[0];
    assert_eq!(record.attr, key(WIDE + 7));
    assert_eq!(record.sequence, 11);
    assert_eq!(record.references, EdgeReferences::Bare(EDGE_REFS.map(key)));
    assert_eq!(*end, bytes.len());
}

#[test]
fn a_bare_edge_whose_owner_has_no_key_does_not_frame() {
    assert!(bare_edges(&edge(PAST_KEY)).is_empty());
}

#[test]
fn a_bare_tolerant_edge_reads_its_tolerance_after_a_wide_head() {
    let refs = [41, 1, 1, 1, WIDE, 1];
    let tolerance = 2.0e-5_f64;
    let mut cells = vec![
        pointer(42),
        12_u32.to_be_bytes().to_vec(),
        pointer(WIDE + 8),
        tolerance.to_be_bytes().to_vec(),
    ];
    cells.extend(refs.iter().map(|value| pointer(*value)));
    cells.push(pointer(2));
    let bytes = record(0x10, &cells.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let readings = parse_tolerant_edge_use_candidates(&bytes, 0);
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].attr, 42);
    assert_eq!(readings[0].tolerance_m, Some(tolerance));
    assert_eq!(readings[0].references, EdgeReferences::Bare(refs.map(key)));
}

#[test]
fn a_bare_vertex_reads_a_wide_attr_point_and_owner() {
    let refs = [1, 2, 3, 4, WIDE];
    let mut cells = vec![pointer(WIDE + 9), 13_u32.to_be_bytes().to_vec()];
    cells.extend(refs.iter().map(|value| pointer(*value)));
    cells.push(MAGIC.to_vec());
    cells.push(pointer(WIDE + 10));
    let bytes = record(0x12, &cells.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let readings = parse_vertex_candidates(&bytes, 0);
    assert_eq!(readings.len(), 1);
    let reading = &readings[0];
    assert_eq!(reading.record.attr, key(WIDE + 9));
    assert_eq!(reading.record.sequence, 13);
    assert_eq!(reading.record.refs, refs.map(key));
    assert_eq!(reading.record.owner, key(WIDE + 10));
    assert_eq!(reading.record.tolerance_m, None);
    assert_eq!(reading.end, bytes.len());
}

#[test]
fn a_bare_point_moves_its_coordinates_by_each_wide_pointer() {
    let xyz = [0.25, -0.5, 1.5_f64];
    let mut cells = vec![pointer(WIDE + 11), 14_u32.to_be_bytes().to_vec()];
    for value in [1, WIDE, 1, WIDE + 12] {
        cells.push(pointer(value));
    }
    for value in xyz {
        cells.push(value.to_be_bytes().to_vec());
    }
    let bytes = record(0x1d, &cells.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let point = parse_point(&bytes, 0, false).unwrap();
    assert_eq!(point.attr, key(WIDE + 11));
    assert_eq!(point.refs, vec![1, key(WIDE), 1, key(WIDE + 12)]);
    assert_eq!(point.xyz_m, xyz);
    assert_eq!(point.xyz_offset + 24, bytes.len());
}

#[test]
fn a_bare_record_with_a_pointer_past_the_key_space_does_not_frame() {
    let fin: Vec<Vec<u8>> = std::iter::once(pointer(30))
        .chain([1, 31, 32, 33, 40, 34, PAST_KEY, 1, 1].map(pointer))
        .chain(std::iter::once(vec![0x2b]))
        .collect();
    let bytes = record(0x11, &fin.iter().map(Vec::as_slice).collect::<Vec<_>>());
    assert!(parse_coedge_candidates(&bytes, 0).is_empty());
    let head = record(
        0x0f,
        &[
            &pointer(PAST_KEY),
            &7_u32.to_be_bytes(),
            &pointer(1),
            &pointer(21),
            &pointer(20),
            &pointer(1),
        ],
    );
    assert!(parse_loop_candidates(&head, 0).is_empty());
}

#[test]
fn a_four_byte_cell_frames_only_in_its_canonical_form() {
    // `80 05 00 00` holds 5, and `7f ff 00 01` lacks the high bit.
    for cell in [[0x80, 0x05, 0x00, 0x00], [0x7f, 0xff, 0x00, 0x01]] {
        let mut bytes = vec![0, 0x0f, 0x00, 0x2a];
        bytes.extend_from_slice(&7_u32.to_be_bytes());
        bytes.extend_from_slice(&pointer(1));
        bytes.extend_from_slice(&cell);
        bytes.extend_from_slice(&pointer(20));
        bytes.extend_from_slice(&pointer(1));
        assert!(parse_loop_candidates(&bytes, 0).is_empty());
    }
}
