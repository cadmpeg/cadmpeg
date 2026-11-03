// SPDX-License-Identifier: Apache-2.0

use super::super::PrimaryRecords;
use crate::test_support::with_decode_context;

fn header() -> Vec<u8> {
    let description = b": TRANSMIT FILE (partition) node contract";
    let schema = b"SCH_999_999_13006";
    let mut bytes = b"PS".to_vec();
    bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(description);
    bytes.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(schema);
    bytes.extend_from_slice(&250u16.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes
}

/// A base-schema POINT: `node_id`, four pointers, and `pvec`.
fn point(index: u16, node_id: u32, owner: u16, pvec: [f64; 3]) -> Vec<u8> {
    let mut bytes = vec![0, 29, 255];
    bytes.extend_from_slice(&index.to_be_bytes());
    bytes.extend_from_slice(&node_id.to_be_bytes());
    for pointer in [1, owner, 1, 1] {
        bytes.extend_from_slice(&pointer.to_be_bytes());
    }
    for value in pvec {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes
}

/// A base-schema FACE with a real tolerance and a `+` sense.
fn face(index: u16, loop_: u16, surface: u16) -> Vec<u8> {
    let mut bytes = vec![0, 14, 255];
    bytes.extend_from_slice(&index.to_be_bytes());
    bytes.extend_from_slice(&7u32.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&1.0e-8_f64.to_be_bytes());
    for pointer in [1, 1, loop_, 1, surface] {
        bytes.extend_from_slice(&pointer.to_be_bytes());
    }
    bytes.push(b'+');
    for _ in 0..5 {
        bytes.extend_from_slice(&1u16.to_be_bytes());
    }
    bytes
}

#[test]
fn nodes_expose_their_fields_by_schema_name() {
    let mut bytes = header();
    let point_at = bytes.len();
    bytes.extend(point(3, 99, 12, [0.001, -2.0, 3.5]));
    let face_at = bytes.len();
    bytes.extend(face(0x7fff, 21, 40));
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    with_decode_context(|_ctx| {
        let records = PrimaryRecords::read(&bytes).unwrap().unwrap();
        assert!(records.is_terminated());
        let nodes: Vec<_> = records.nodes().collect();
        assert_eq!(nodes.len(), 2);

        let point = nodes[0];
        assert_eq!((point.kind(), point.index()), (29, 3));
        assert_eq!(point.range().start, point_at);
        assert_eq!(point.integer("node_id"), Some(99));
        assert_eq!(point.pointer("owner"), Some(12));
        assert_eq!(point.pointer("next"), Some(1));
        assert_eq!(point.pointers(["owner", "next"]), Some([12, 1]));
        assert_eq!(point.pointers(["owner", "absent"]), None);
        assert_eq!(point.vector("pvec"), Some([0.001, -2.0, 3.5]));
        assert_eq!(point.real("pvec"), None);
        assert_eq!(point.pointer("absent"), None);

        let face = nodes[1];
        assert_eq!((face.kind(), face.index()), (14, 0x7fff));
        assert_eq!(face.range(), face_at..bytes.len() - 4);
        assert_eq!(face.real("tolerance"), Some(1.0e-8));
        assert_eq!(face.is_null("tolerance"), Some(false));
        assert_eq!(face.is_null("sense"), None);
        assert_eq!(face.pointer("loop"), Some(21));
        assert_eq!(face.pointer("surface"), Some(40));
        assert_eq!(face.char("sense"), Some(b'+'));
        let sense = face.field_range("sense").unwrap();
        assert_eq!(bytes[sense], [b'+']);
    });
}

#[test]
fn a_stopped_sequence_is_not_terminated() {
    let mut bytes = header();
    bytes.extend(point(3, 99, 12, [0.0; 3]));
    with_decode_context(|_ctx| {
        let records = PrimaryRecords::read(&bytes).unwrap().unwrap();
        assert!(!records.is_terminated());
        assert_eq!(records.nodes().count(), 1);
    });
}

/// A base-schema `KNOT_SET` (128): its variable `knots` field, counted before the index.
fn knot_set(index: u16, knots: &[f64]) -> Vec<u8> {
    let mut bytes = vec![0, 128, 255];
    bytes.extend_from_slice(&u32::try_from(knots.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&index.to_be_bytes());
    for knot in knots {
        bytes.extend_from_slice(&knot.to_be_bytes());
    }
    bytes
}

/// A base-schema `KNOT_MULT` (127): its variable `mult` field.
fn knot_mult(index: u16, mult: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0, 127, 255];
    bytes.extend_from_slice(&u32::try_from(mult.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&index.to_be_bytes());
    for value in mult {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes
}

#[test]
fn variable_fields_read_their_counted_elements() {
    let mut bytes = header();
    bytes.extend(knot_set(3, &[0.0, 0.5, 1.0]));
    bytes.extend(knot_mult(4, &[3, 1, 3]));
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    with_decode_context(|_ctx| {
        let records = PrimaryRecords::read(&bytes).unwrap().unwrap();
        assert!(records.is_terminated());
        let nodes: Vec<_> = records.nodes().collect();
        assert_eq!(nodes[0].reals("knots"), Some(vec![0.0, 0.5, 1.0]));
        assert_eq!(nodes[0].real("knots"), None);
        assert_eq!(nodes[0].shorts("knots"), None);
        assert_eq!(nodes[1].shorts("mult"), Some(vec![3, 1, 3]));
        assert_eq!(nodes[1].short("mult"), None);
    });
}
