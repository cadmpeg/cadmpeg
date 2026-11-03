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

/// A base-schema `BLEND_SURF` (56) whose `surface` array holds `surfaces`.
fn blend_surf(index: u16, surfaces: [u16; 2]) -> Vec<u8> {
    let mut bytes = vec![0, 56, 255];
    bytes.extend_from_slice(&index.to_be_bytes());
    bytes.extend_from_slice(&7u32.to_be_bytes());
    for pointer in [1u16; 5] {
        bytes.extend_from_slice(&pointer.to_be_bytes());
    }
    bytes.extend_from_slice(b"+R");
    for pointer in [surfaces[0], surfaces[1], 1] {
        bytes.extend_from_slice(&pointer.to_be_bytes());
    }
    for value in [0.002, -0.5, 1.0, 2.0] {
        bytes.extend_from_slice(&f64::to_be_bytes(value));
    }
    for _ in 0..4 {
        bytes.extend_from_slice(&1u16.to_be_bytes());
    }
    bytes
}

#[test]
fn fixed_arrays_read_exactly_their_declared_extent() {
    let mut bytes = header();
    bytes.extend(blend_surf(3, [20, 21]));
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    with_decode_context(|_ctx| {
        let records = PrimaryRecords::read(&bytes).unwrap().unwrap();
        assert!(records.is_terminated());
        let nodes: Vec<_> = records.nodes().collect();
        let blend = nodes[0];
        assert_eq!(blend.pointer_array::<2>("surface"), Some([20, 21]));
        assert_eq!(blend.real_array::<2>("range"), Some([0.002, -0.5]));
        assert_eq!(blend.real_array::<2>("thumb_weight"), Some([1.0, 2.0]));
        assert_eq!(blend.pointer_array::<2>("boundary"), Some([1, 1]));
        assert_eq!(blend.pointer_array::<3>("surface"), None);
        assert_eq!(blend.pointer_array::<1>("spine"), None);
        assert_eq!(blend.real_array::<2>("surface"), None);
        assert_eq!(blend.pointer_array::<2>("range"), None);
        assert_eq!(blend.pointer("surface"), None);
    });
}

/// A base-schema `LIMIT` (41): its `type` and variable `hvec` field.
fn limit(index: u16, kind: u8, points: &[[f64; 3]]) -> Vec<u8> {
    let mut bytes = vec![0, 41, 255];
    bytes.extend_from_slice(&u32::try_from(points.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&index.to_be_bytes());
    bytes.push(kind);
    for value in points.iter().flatten() {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes
}

#[test]
fn hvec_fields_read_their_counted_points() {
    let points = [[0.001, 0.0, -2.0], [1.0, 2.0, 3.0]];
    let mut bytes = header();
    bytes.extend(limit(3, b'L', &points));
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    with_decode_context(|_ctx| {
        let records = PrimaryRecords::read(&bytes).unwrap().unwrap();
        assert!(records.is_terminated());
        let limit = records.nodes().next().unwrap();
        assert_eq!(limit.char("type"), Some(b'L'));
        assert_eq!(limit.hvec_points("hvec"), Some(points.to_vec()));
        assert_eq!(limit.reals("hvec"), None);
        assert_eq!(limit.hvec_points("type"), None);
    });
}

/// A base-schema `CHAR_VALUES` (79): its variable `string` field.
fn char_values(index: u16, text: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 79, 255];
    bytes.extend_from_slice(&u32::try_from(text.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&index.to_be_bytes());
    bytes.extend_from_slice(text);
    bytes
}

/// A base-13006 `ATTRIB_DEF` (80) with `legal_owners` and a variable
/// `fields` list.
fn attrib_def(index: u16, identifier: u16, owners: [u8; 14], fields: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 80, 255];
    bytes.extend_from_slice(&u32::try_from(fields.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&index.to_be_bytes());
    for pointer in [1, identifier] {
        bytes.extend_from_slice(&pointer.to_be_bytes());
    }
    bytes.extend_from_slice(&7u32.to_be_bytes());
    bytes.extend_from_slice(&[0, 1, 2, 3, 4, 5, 6, 0]);
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&owners);
    bytes.extend_from_slice(fields);
    bytes
}

/// A base-schema `ATTRIBUTE` (81) whose variable `fields` holds `fields`.
fn attribute(index: u16, definition: u16, owner: u16, fields: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0, 81, 255];
    bytes.extend_from_slice(&u32::try_from(fields.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&index.to_be_bytes());
    bytes.extend_from_slice(&9u32.to_be_bytes());
    for pointer in [definition, owner, 1, 1, 1, 1] {
        bytes.extend_from_slice(&pointer.to_be_bytes());
    }
    for pointer in fields {
        bytes.extend_from_slice(&pointer.to_be_bytes());
    }
    bytes
}

/// A base-schema `INT_VALUES` (82): its variable `values` field.
fn int_values(index: u16, values: &[u32]) -> Vec<u8> {
    let mut bytes = vec![0, 82, 255];
    bytes.extend_from_slice(&u32::try_from(values.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(&index.to_be_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes
}

#[test]
fn attribute_fields_read_their_declared_elements() {
    let mut owners = [0; 14];
    owners[3] = 1;
    let mut bytes = header();
    bytes.extend(char_values(3, b"FACE_ID_2001"));
    bytes.extend(attrib_def(4, 3, owners, &[9, 9, 1, 1]));
    bytes.extend(attribute(5, 4, 20, &[1, 1, 6, 1]));
    bytes.extend(int_values(6, &[36, 7, 0xffff_ffff]));
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    with_decode_context(|_ctx| {
        let records = PrimaryRecords::read(&bytes).unwrap().unwrap();
        assert!(records.is_terminated());
        let nodes: Vec<_> = records.nodes().collect();
        assert_eq!(nodes[0].chars("string"), Some(b"FACE_ID_2001".to_vec()));
        assert_eq!(nodes[0].bytes("string"), None);
        let definition = nodes[1];
        assert_eq!(definition.pointer("identifier"), Some(3));
        assert_eq!(
            definition.byte_array::<8>("actions"),
            Some([0, 1, 2, 3, 4, 5, 6, 0])
        );
        assert_eq!(definition.byte_array::<7>("actions"), None);
        let expected: Vec<bool> = owners.iter().map(|owner| *owner == 1).collect();
        assert_eq!(definition.logicals("legal_owners"), Some(expected));
        assert_eq!(definition.logicals("actions"), None);
        assert_eq!(definition.bytes("fields"), Some(vec![9, 9, 1, 1]));
        assert_eq!(nodes[2].pointer_list("fields"), Some(vec![1, 1, 6, 1]));
        assert_eq!(nodes[2].integers("fields"), None);
        assert_eq!(nodes[3].integers("values"), Some(vec![36, 7, 0xffff_ffff]));
    });
}

#[test]
fn a_logical_array_with_an_element_outside_zero_and_one_does_not_read() {
    let mut owners = [0; 14];
    owners[0] = 2;
    let mut bytes = header();
    bytes.extend(attrib_def(4, 3, owners, &[1]));
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    with_decode_context(|_ctx| {
        let records = PrimaryRecords::read(&bytes).unwrap().unwrap();
        let definition = records.nodes().next().unwrap();
        assert_eq!(definition.logicals("legal_owners"), None);
    });
}
