// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::ids::BodyId;

use crate::decode::build::topology_body_node_ids;
use crate::framing::node_kind::NodeKind;
use crate::test_support::test_bytes::{put_f64, put_ref, put_vec3, record};
use crate::topology::{Graph, Node};

/// The per-body scan the indexed membership join replaced.
fn pairwise_topology_body_node_ids(graph: &Graph) -> BTreeMap<BodyId, BTreeSet<u32>> {
    let scope = crate::decode::ids::IdScope::stream_from_index(0).unwrap();
    let body_xmts = graph
        .body_shape_shells()
        .filter_map(|shell| shell.shell_fields()?.body.map(u32::from))
        .collect::<BTreeSet<_>>();
    let mut bodies = BTreeMap::new();
    'body: for body_xmt in body_xmts {
        let shells = graph
            .of_kind(NodeKind::Shell)
            .filter(|shell| {
                shell
                    .shell_fields()
                    .is_some_and(|fields| fields.body.map(u32::from) == Some(body_xmt))
            })
            .map(Node::xmt)
            .collect::<BTreeSet<_>>();
        let faces = graph
            .of_kind(NodeKind::Face)
            .filter(|face| {
                face.face_fields().is_some_and(|fields| {
                    fields
                        .shell
                        .is_some_and(|target| shells.contains(&u32::from(target)))
                })
            })
            .collect::<Vec<_>>();
        let face_xmts = faces.iter().map(|face| face.xmt()).collect::<BTreeSet<_>>();
        let loops = graph
            .of_kind(NodeKind::Loop)
            .filter(|loop_| {
                loop_.loop_fields().is_some_and(|fields| {
                    fields
                        .face
                        .is_some_and(|target| face_xmts.contains(&u32::from(target)))
                })
            })
            .map(Node::xmt)
            .collect::<BTreeSet<_>>();
        let mut edge_xmts = BTreeSet::new();
        let mut vertex_xmts = BTreeSet::new();
        for fin in graph.of_kind(NodeKind::Fin) {
            let Some(fields) = fin.fin_fields() else {
                continue;
            };
            if !fields
                .loop_xmt
                .is_some_and(|target| loops.contains(&u32::from(target)))
            {
                continue;
            }
            let (Some(edge), Some(vertex)) = (fields.edge, fields.vertex) else {
                continue 'body;
            };
            edge_xmts.insert(u32::from(edge));
            vertex_xmts.insert(u32::from(vertex));
        }
        let mut ids = BTreeSet::new();
        for face in faces {
            let Some(id) = face.u32_at(4) else {
                continue 'body;
            };
            ids.insert(id);
        }
        for (kind, xmts) in [(NodeKind::Edge, edge_xmts), (NodeKind::Vertex, vertex_xmts)] {
            let mut count = 0;
            for node in graph
                .of_kind(kind)
                .filter(|node| xmts.contains(&node.xmt()))
            {
                count += 1;
                let Some(id) = node.u32_at(4) else {
                    continue 'body;
                };
                ids.insert(id);
            }
            if count != xmts.len() {
                continue 'body;
            }
        }
        bodies.insert(
            scope
                .id_from_display::<BodyId>(&cadmpeg_ir::identity_component!("body"), body_xmt)
                .unwrap(),
            ids,
        );
    }
    bodies
}

/// One partition stream with twelve bodies of one or two shells, two faces per
/// shell and one fin per face. Bodies 3 and 8 have a fin with no vertex, and
/// bodies 4 and 9 a fin whose vertex record is absent.
fn many_body_topology_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(
        b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );
    let with_id = |mut record: Vec<u8>, id: u32| {
        record[4..8].copy_from_slice(&id.to_be_bytes());
        record
    };
    for body_index in 0..12_u16 {
        let base = 20 + body_index * 40;
        let id = |offset: u16| u32::from(body_index) * 1000 + u32::from(offset);
        let mut body = record(12, 24);
        put_ref(&mut body, 2, base);
        s.extend(with_id(body, id(0)));
        let shell_count = 1 + body_index % 2;
        for shell_index in 0..shell_count {
            let shell_xmt = base + 1 + shell_index;
            let mut shell = record(13, 24);
            put_ref(&mut shell, 2, shell_xmt);
            put_ref(&mut shell, 8, 1);
            put_ref(&mut shell, 10, base);
            put_ref(&mut shell, 12, 1);
            put_ref(&mut shell, 14, base + 3 + shell_index * 2);
            put_ref(&mut shell, 16, 1);
            put_ref(&mut shell, 18, 1);
            put_ref(&mut shell, 20, base + 39);
            put_ref(&mut shell, 22, 1);
            s.extend(with_id(shell, id(1 + shell_index)));
        }
        for face_index in 0..shell_count * 2 {
            let shell_index = face_index / 2;
            let face_xmt = base + 3 + face_index;
            let mut face = record(14, 39);
            put_ref(&mut face, 2, face_xmt);
            put_f64(&mut face, 10, 0.000_2);
            put_ref(
                &mut face,
                18,
                if face_index % 2 == 0 { face_xmt + 1 } else { 1 },
            );
            put_ref(
                &mut face,
                20,
                if face_index % 2 == 1 { face_xmt - 1 } else { 1 },
            );
            put_ref(&mut face, 22, base + 10 + face_index);
            put_ref(&mut face, 24, base + 1 + shell_index);
            put_ref(&mut face, 26, base + 30);
            face[28] = b'+';
            s.extend(with_id(face, id(3 + face_index)));

            let fin_xmt = base + 16 + face_index;
            let mut loop_ = record(15, 16);
            put_ref(&mut loop_, 2, base + 10 + face_index);
            put_ref(&mut loop_, 10, fin_xmt);
            put_ref(&mut loop_, 12, face_xmt);
            put_ref(&mut loop_, 14, 1);
            s.extend(with_id(loop_, id(10 + face_index)));

            let vertex = match (body_index % 5, face_index) {
                (3, 1) => 1,
                (4, 1) => base + 29,
                _ => base + 22 + face_index % 2,
            };
            let mut fin = record(17, 23);
            put_ref(&mut fin, 2, fin_xmt);
            put_ref(&mut fin, 6, base + 10 + face_index);
            put_ref(&mut fin, 8, fin_xmt);
            put_ref(&mut fin, 10, fin_xmt);
            put_ref(&mut fin, 12, vertex);
            put_ref(&mut fin, 14, 1);
            put_ref(&mut fin, 16, base + 26 + face_index % 2);
            put_ref(&mut fin, 18, base + 31);
            fin[22] = b'+';
            s.extend(fin);
        }
        for end in 0..2 {
            let mut edge = record(16, 32);
            put_ref(&mut edge, 2, base + 26 + end);
            put_f64(&mut edge, 10, 0.000_3);
            put_ref(&mut edge, 18, base + 16 + end);
            put_ref(&mut edge, 24, base + 31);
            s.extend(with_id(edge, id(26 + end)));

            let mut vertex = record(18, 28);
            put_ref(&mut vertex, 2, base + 22 + end);
            put_ref(&mut vertex, 16, base + 32);
            put_f64(&mut vertex, 18, 0.000_1);
            s.extend(with_id(vertex, id(22 + end)));
        }
        let mut plane = record(50, 91);
        put_ref(&mut plane, 2, base + 30);
        plane[18] = b'+';
        put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
        put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
        put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
        s.extend(plane);
        let mut line = record(30, 67);
        put_ref(&mut line, 2, base + 31);
        line[18] = b'+';
        put_vec3(&mut line, 19, [0.0, 0.0, 0.0]);
        put_vec3(&mut line, 43, [1.0, 0.0, 0.0]);
        s.extend(line);
        let mut point = record(29, 40);
        put_ref(&mut point, 2, base + 32);
        put_vec3(&mut point, 16, [0.01, 0.02, 0.03]);
        s.extend(point);
        let mut region = record(19, 16);
        put_ref(&mut region, 2, base + 39);
        s.extend(region);
    }
    s
}

#[test]
fn topology_body_node_ids_match_the_per_body_scan_over_many_bodies() {
    let graph = Graph::parse(&many_body_topology_stream());

    let expected = pairwise_topology_body_node_ids(&graph);

    assert_eq!(expected.len(), 8);
    assert!(expected.values().all(|ids| ids.len() >= 6));
    assert_eq!(topology_body_node_ids(0, &graph).unwrap(), expected);
}
