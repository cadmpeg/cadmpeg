// SPDX-License-Identifier: Apache-2.0

use super::{own, sequence, tables};
use crate::brep::topology::EdgeReferences;
use crate::test_support::parasolid::sequence::{
    face_fields, topology_sequence, topology_sequence_with_face, F,
};
use cadmpeg_ir::topology::Sense;
use std::collections::HashSet;

fn read(payload: &[u8], body_offset: usize) -> Option<crate::brep::topology::Tables> {
    tables(&sequence(payload)?, payload, body_offset)
}

/// Overwrite the named field of the first node of `kind`.
fn patch(payload: &mut [u8], kind: u16, field: &str, bytes: &[u8]) {
    let records = sequence(payload).unwrap();
    let node = records.nodes().find(|node| node.kind() == kind).unwrap();
    let at = node.field_range(field).unwrap().start;
    drop(records);
    payload[at..at + bytes.len()].copy_from_slice(bytes);
}

#[test]
fn topology_records_take_their_fields_from_the_node_sequence() {
    let (payload, body_offset) = topology_sequence();
    let records = sequence(&payload).expect("the sequence reaches its terminator");
    let mut t = tables(&records, &payload, body_offset).unwrap();
    own(&mut t, &HashSet::new(), &HashSet::from([2]));
    let start = |kind: u16| {
        records
            .nodes()
            .find(|node| node.kind() == kind)
            .unwrap()
            .range()
            .start
            - body_offset
    };

    let face = &t.bridges()[&5];
    assert_eq!(face.refs, [1, 1, 6, 4, 20]);
    assert_eq!(
        (face.sequence, face.sense, face.attributes_head),
        (4, Sense::Forward, None)
    );
    assert_eq!(face.offset, start(14));

    let loop_ = &t.loops()[&6];
    assert_eq!((loop_.refs, loop_.offset), ([1, 7, 5, 1], start(15)));
    assert!(!t.loop_is_ambiguous(6));

    let fin = &t.coedges()[&7];
    assert_eq!(fin.refs, [1, 6, 7, 7, 9, 1, 8, 1, 1]);
    assert_eq!(fin.sense, Sense::Reversed);
    assert_eq!(t.coedge_owners[&7], HashSet::from([6]));

    let edge = &t.edge_uses()[&8];
    assert_eq!(edge.references, EdgeReferences::Bare([7, 1, 1, 21, 1, 1]));
    assert_eq!((edge.sequence, edge.tolerance_m), (6, None));

    let vertex = &t.vertex_uses()[&9];
    assert_eq!((vertex.refs, vertex.owner), ([1, 7, 1, 1, 10], 2));
    assert_eq!(vertex.tolerance_m, Some(2.0e-8));
    assert!(t.vertex_classes[&9]);

    let point = &t.points()[&10];
    assert_eq!(point.refs, [1, 9, 1, 1]);
    assert_eq!(point.xyz_m, [0.001, 0.002, 0.003]);
    assert_eq!(
        &payload[body_offset + point.xyz_offset..][..8],
        &0.001_f64.to_be_bytes()
    );
    assert!(t.owner_classes.contains(&2));
}

#[test]
fn an_ownership_only_face_at_an_excluded_offset_is_not_a_bridge() {
    let (mut payload, body_offset) = topology_sequence();
    patch(&mut payload, 14, "loop", &1u16.to_be_bytes());
    let mut t = read(&payload, body_offset).unwrap();
    let offset = t.bridges()[&5].offset;
    own(&mut t, &HashSet::new(), &HashSet::new());
    assert!(t.bridges().contains_key(&5));
    assert!(t.bridge_frames.contains_key(&5));
    own(&mut t, &HashSet::from([offset]), &HashSet::new());
    assert!(!t.bridges().contains_key(&5));
    assert!(!t.bridge_frames.contains_key(&5));
}

#[test]
fn a_vertex_with_an_unsupported_tolerance_keeps_its_frame_unresolved() {
    for tolerance in [0.0_f64, -1.0e-8, f64::NAN] {
        let (mut payload, body_offset) = topology_sequence();
        patch(&mut payload, 18, "tolerance", &tolerance.to_be_bytes());
        let t = read(&payload, body_offset).unwrap();
        assert!(!t.vertex_uses().contains_key(&9));
        assert!(t.vertex_is_unresolved(9));
        assert!(t.vertex_classes[&9]);
    }
}

#[test]
fn duplicate_indices_and_non_finite_points_leave_the_stream_to_the_scanners() {
    let (mut payload, body_offset) = topology_sequence();
    let records = sequence(&payload).unwrap();
    let point = records.nodes().find(|node| node.kind() == 29).unwrap();
    let index_at = point.field_range("node_id").unwrap().start - 2;
    drop(records);
    payload[index_at..index_at + 2].copy_from_slice(&9u16.to_be_bytes());
    assert!(read(&payload, body_offset).is_none());

    let (mut payload, body_offset) = topology_sequence();
    patch(&mut payload, 29, "pvec", &f64::NAN.to_be_bytes());
    assert!(read(&payload, body_offset).is_none());
}

#[test]
fn an_edited_face_reads_its_fields_by_name() {
    // Insert integer `extra` after `node_id`: count 15, C, I, then 13 copies.
    let mut program = vec![15, b'C', b'I', 5];
    program.extend_from_slice(b"extra");
    program.extend_from_slice(&[0, 0, 0, 1, 1, b'd']);
    program.extend(std::iter::repeat_n(b'C', 13));
    program.push(b'Z');
    let mut fields = face_fields();
    fields.insert(1, F::I(0xdead));
    let (payload, body_offset) = topology_sequence_with_face(Some((&program, &fields)));
    let t = read(&payload, body_offset).expect("an inserted field keeps the base names");
    assert_eq!(t.bridges()[&5].refs, [1, 1, 6, 4, 20]);
    assert_eq!(t.bridges()[&5].sequence, 4);
}

#[test]
fn a_face_without_its_surface_field_leaves_the_stream_to_the_scanners() {
    // Delete `surface`, the eighth base field: count 13, 7 copies, D, 6 copies.
    let mut program = vec![13];
    program.extend(std::iter::repeat_n(b'C', 7));
    program.push(b'D');
    program.extend(std::iter::repeat_n(b'C', 6));
    program.push(b'Z');
    let mut fields = face_fields();
    fields.remove(7);
    let (payload, body_offset) = topology_sequence_with_face(Some((&program, &fields)));
    assert!(sequence(&payload).is_some());
    assert!(read(&payload, body_offset).is_none());
}

#[test]
fn a_stopped_sequence_has_no_node_tables() {
    let (mut payload, _) = topology_sequence();
    payload.truncate(payload.len() - 4);
    assert!(sequence(&payload).is_none());
}

/// A Deltas POINT: index, node ID, four status-paired pointers, and `x`.
fn deltas_point(first: bool, index: u16, x: f64) -> Vec<u8> {
    let mut bytes = vec![0, 29];
    if first {
        bytes.push(255);
    }
    bytes.extend(index.to_be_bytes());
    bytes.extend(7u32.to_be_bytes());
    for _ in 0..4 {
        bytes.extend(1u16.to_be_bytes());
        bytes.push(1);
    }
    for value in [x, 0.0, 0.0] {
        bytes.extend(value.to_be_bytes());
    }
    bytes
}

#[test]
fn deltas_versions_keep_equal_records_and_leave_differing_ones_ambiguous() {
    use cadmpeg_parasolid::records::DeltasRecords;
    use cadmpeg_parasolid::test_support::test_pmarks;
    let mut stream = test_pmarks::prefix(1);
    stream.extend(deltas_point(true, 40, 0.001));
    stream.extend(deltas_point(false, 40, 0.001));
    stream.extend(deltas_point(false, 41, 0.001));
    stream.extend(deltas_point(false, 41, 0.002));
    let stream = test_pmarks::complete(stream);
    let records = DeltasRecords::read(&stream).unwrap();
    let tables = super::deltas_tables(&records, &stream, 0).unwrap();
    assert!((tables.points()[&40].xyz_m[0] - 0.001).abs() < 1.0e-15);
    assert!(!tables.points().contains_key(&41));
    assert!(tables.point_is_ambiguous(41));
}
