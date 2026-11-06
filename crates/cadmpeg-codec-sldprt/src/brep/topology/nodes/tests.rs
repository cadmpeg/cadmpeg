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
    let agreement = super::DeltasAgreement::new([(&records, stream.as_slice())]);
    let tables = super::deltas_tables(&records, &stream, 0, &agreement).unwrap();
    assert!((tables.points()[&40].xyz_m[0] - 0.001).abs() < 1.0e-15);
    assert!(!tables.points().contains_key(&41));
    assert!(tables.point_is_ambiguous(41));
}

/// A Deltas EDGE: index, node ID, status-paired pointers, and `curve`.
fn deltas_edge(first: bool, index: u16, curve: u16) -> Vec<u8> {
    let mut bytes = vec![0, 16];
    if first {
        bytes.push(255);
    }
    bytes.extend(index.to_be_bytes());
    bytes.extend(7u32.to_be_bytes());
    bytes.extend(1u16.to_be_bytes());
    bytes.push(1);
    bytes.extend(crate::test_support::parasolid::sequence::NULL_REAL.to_be_bytes());
    for target in [1, 1, 1, curve, 1, 1, 1] {
        bytes.extend(u16::to_be_bytes(target));
        bytes.push(1);
    }
    bytes
}

#[test]
fn deltas_edges_whose_versions_disagree_across_streams_are_not_read() {
    use cadmpeg_parasolid::records::DeltasRecords;
    use cadmpeg_parasolid::test_support::test_pmarks;
    let stream = |curve| {
        let mut bytes = test_pmarks::prefix(1);
        bytes.extend(deltas_edge(true, 50, curve));
        bytes.extend(deltas_edge(false, 51, 31));
        test_pmarks::complete(bytes)
    };
    let (first, second) = (stream(30), stream(32));
    let first_records = DeltasRecords::read(&first).unwrap();
    let second_records = DeltasRecords::read(&second).unwrap();
    let agreement = super::DeltasAgreement::new([
        (&first_records, first.as_slice()),
        (&second_records, second.as_slice()),
    ]);
    for (records, payload) in [(&first_records, &first), (&second_records, &second)] {
        let tables = super::deltas_tables(records, payload, 0, &agreement).unwrap();
        assert!(!tables.edge_uses().contains_key(&50));
        assert!(tables.edge_uses().contains_key(&51));
    }
}

#[test]
fn ownership_keeps_the_ambiguity_marker_of_a_conflicting_face() {
    let mut tables = crate::brep::topology::Tables::default();
    tables.bridge_frames.insert(
        9,
        crate::brep::topology::RecordFrame {
            partition: false,
            bytes: None,
        },
    );
    own(&mut tables, &HashSet::new(), &HashSet::new());
    assert!(tables
        .bridge_frames
        .get(&9)
        .is_some_and(|frame| frame.bytes.is_none()));
}

#[test]
fn edges_with_a_tolerance_outside_the_stored_bounds_are_not_read() {
    use cadmpeg_parasolid::records::DeltasRecords;
    use cadmpeg_parasolid::test_support::test_pmarks;
    let edge = |first: bool, index: u16, tolerance: f64| {
        let mut bytes = deltas_edge(first, index, 30);
        let at = if first { 12 } else { 11 };
        bytes[at..at + 8].copy_from_slice(&tolerance.to_be_bytes());
        bytes
    };
    let mut bytes = test_pmarks::prefix(1);
    bytes.extend(edge(true, 50, -1.0));
    bytes.extend(edge(false, 51, f64::NAN));
    bytes.extend(edge(false, 52, 1.0e-6));
    let bytes = test_pmarks::complete(bytes);
    let records = DeltasRecords::read(&bytes).unwrap();
    let agreement = super::DeltasAgreement::new([(&records, bytes.as_slice())]);
    let tables = super::deltas_tables(&records, &bytes, 0, &agreement).unwrap();
    assert!(!tables.edge_uses().contains_key(&50));
    assert!(!tables.edge_uses().contains_key(&51));
    assert_eq!(tables.edge_uses()[&52].tolerance_m, Some(1.0e-6));
}

#[test]
fn deltas_versions_agree_across_the_streams_of_a_site() {
    use cadmpeg_parasolid::records::DeltasRecords;
    use cadmpeg_parasolid::test_support::test_pmarks;
    let stream = |points: &[(bool, u16, f64)]| {
        let mut bytes = test_pmarks::prefix(1);
        for (first, index, x) in points {
            bytes.extend(deltas_point(*first, *index, *x));
        }
        test_pmarks::complete(bytes)
    };
    let first = stream(&[(true, 40, 0.001), (false, 41, 0.001), (false, 41, 0.002)]);
    let second = stream(&[(true, 40, 0.003), (false, 41, 0.001), (false, 42, 0.001)]);
    let first_records = DeltasRecords::read(&first).unwrap();
    let second_records = DeltasRecords::read(&second).unwrap();
    let agreement = super::DeltasAgreement::new([
        (&first_records, first.as_slice()),
        (&second_records, second.as_slice()),
    ]);
    for records in [&first_records, &second_records] {
        let indices: Vec<u32> = super::consistent_deltas_nodes(records, &agreement)
            .iter()
            .map(cadmpeg_parasolid::records::deltas::node::DeltasNode::index)
            .collect();
        assert!(!indices.contains(&40));
        assert!(!indices.contains(&41));
    }
    let second_indices: Vec<u32> = super::consistent_deltas_nodes(&second_records, &agreement)
        .iter()
        .map(cadmpeg_parasolid::records::deltas::node::DeltasNode::index)
        .collect();
    assert_eq!(second_indices, vec![42]);
}

#[test]
fn a_vertex_loop_fin_reads_its_loop_and_vertex_and_keeps_its_frame() {
    let (mut payload, body_offset) = topology_sequence();
    patch(&mut payload, 17, "sense", b"?");
    assert!(read(&payload, body_offset).is_none());
    for field in ["edge", "other"] {
        patch(&mut payload, 17, field, &1u16.to_be_bytes());
    }
    let tables = read(&payload, body_offset).expect("node tables");
    assert!(!tables.coedges().contains_key(&7));
    let fin = &tables.vertex_loop_fins()[&7];
    assert_eq!((fin.loop_attr, fin.vertex), (6, 9));
    assert!(tables.coedge_frames[&7].bytes.is_some());
    assert_eq!(tables.coedge_owners[&7], HashSet::from([6]));
    assert!(tables.loops().contains_key(&6));
}

/// A Deltas FIN: index, nine status-paired pointers, and `sense`.
fn deltas_fin(first: bool, index: u16, refs: [u16; 9], sense: u8) -> Vec<u8> {
    let mut bytes = vec![0, 17];
    if first {
        bytes.push(255);
    }
    bytes.extend(index.to_be_bytes());
    for target in refs {
        bytes.extend(target.to_be_bytes());
        bytes.push(1);
    }
    bytes.push(sense);
    bytes
}

const VERTEX_LOOP_FIN: [u16; 9] = [1, 61, 60, 60, 62, 1, 1, 1, 1];
const EDGE_FIN: [u16; 9] = [1, 61, 60, 60, 62, 1, 63, 1, 1];

fn deltas_fin_tables(fins: &[(u16, [u16; 9], u8)]) -> crate::brep::topology::Tables {
    use cadmpeg_parasolid::records::DeltasRecords;
    use cadmpeg_parasolid::test_support::test_pmarks;
    let mut bytes = test_pmarks::prefix(1);
    for (position, (index, refs, sense)) in fins.iter().enumerate() {
        bytes.extend(deltas_fin(position == 0, *index, *refs, *sense));
    }
    let bytes = test_pmarks::complete(bytes);
    let records = DeltasRecords::read(&bytes).unwrap();
    let agreement = super::DeltasAgreement::new([(&records, bytes.as_slice())]);
    super::deltas_tables(&records, &bytes, 0, &agreement).unwrap()
}

#[test]
fn a_deltas_vertex_loop_fin_and_an_edge_fin_of_one_identity_leave_it_ambiguous() {
    let agreeing = deltas_fin_tables(&[(60, VERTEX_LOOP_FIN, b'?'), (60, VERTEX_LOOP_FIN, b'?')]);
    assert_eq!(agreeing.vertex_loop_fins()[&60].vertex, 62);
    assert!(!agreeing.coedge_is_ambiguous(60));

    for fins in [
        [(60, VERTEX_LOOP_FIN, b'?'), (60, EDGE_FIN, b'+')],
        [(60, EDGE_FIN, b'+'), (60, VERTEX_LOOP_FIN, b'?')],
    ] {
        let tables = deltas_fin_tables(&fins);
        assert!(tables.coedge_is_ambiguous(60));
        assert!(!tables.coedges().contains_key(&60));
        assert!(!tables.vertex_loop_fins().contains_key(&60));
    }
}

#[test]
fn a_partition_fin_of_either_form_stands_over_its_deltas_version() {
    let (mut payload, body_offset) = topology_sequence();
    for field in ["edge", "other"] {
        patch(&mut payload, 17, field, &1u16.to_be_bytes());
    }
    patch(&mut payload, 17, "sense", b"?");
    let mut partition = read(&payload, body_offset).unwrap();
    let mut edge_fin = EDGE_FIN;
    edge_fin[1] = 6;
    partition.merge_deltas(deltas_fin_tables(&[(7, edge_fin, b'+')]), None);
    assert!(!partition.coedges().contains_key(&7));
    assert_eq!(partition.vertex_loop_fins()[&7].loop_attr, 6);

    let (payload, body_offset) = topology_sequence();
    let mut partition = read(&payload, body_offset).unwrap();
    let mut vertex_fin = VERTEX_LOOP_FIN;
    vertex_fin[1..4].copy_from_slice(&[6, 7, 7]);
    partition.merge_deltas(deltas_fin_tables(&[(7, vertex_fin, b'?')]), None);
    assert!(!partition.vertex_loop_fins().contains_key(&7));
    assert_eq!(partition.coedges()[&7].refs[6], 8);
}

#[test]
fn a_rejected_partition_edge_is_not_filled_from_deltas() {
    use cadmpeg_parasolid::records::DeltasRecords;
    use cadmpeg_parasolid::test_support::test_pmarks;
    let (mut payload, body_offset) = topology_sequence();
    let edge = *read(&payload, body_offset)
        .unwrap()
        .edge_uses()
        .keys()
        .next()
        .unwrap();
    patch(&mut payload, 16, "tolerance", &(-1.0f64).to_be_bytes());
    let mut partition = read(&payload, body_offset).unwrap();
    assert!(!partition.edge_uses().contains_key(&edge));
    let mut bytes = test_pmarks::prefix(1);
    bytes.extend(deltas_edge(true, u16::try_from(edge).unwrap(), 30));
    let bytes = test_pmarks::complete(bytes);
    let records = DeltasRecords::read(&bytes).unwrap();
    let agreement = super::DeltasAgreement::new([(&records, bytes.as_slice())]);
    let deltas = super::deltas_tables(&records, &bytes, 0, &agreement).unwrap();
    assert!(deltas.edge_uses().contains_key(&edge));
    partition.merge_deltas(deltas, None);
    assert!(!partition.edge_uses().contains_key(&edge));
}
