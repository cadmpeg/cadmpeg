// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::{LossCategory, LossKind, LossTaxonomy};
use cadmpeg_ir::Exactness;

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;
use crate::NxCodec;

use super::*;

#[test]
fn topology_rejects_shell_with_broken_face_ownership_chain() {
    let valid = topology_partition_stream();
    let graph = crate::topology::Graph::parse(&valid);
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut broken = valid;
    let face = broken
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut broken, face + 24, 99);
    assert!(crate::topology::Graph::parse(&broken)
        .body_shape_shells()
        .is_empty());

    let mut independent_previous = topology_partition_stream();
    let face = independent_previous
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut independent_previous, face + 20, 99);
    assert_eq!(
        crate::topology::Graph::parse(&independent_previous)
            .body_shape_shells()
            .len(),
        1
    );
}

#[test]
fn topology_retains_shell_body_identity_without_body_record() {
    let mut stream = topology_partition_stream();
    let body = stream
        .windows(4)
        .position(|window| window == [0, 12, 0, 2])
        .expect("body record");
    stream[body..body + 24].fill(0xff);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(12, 2).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].id.0, "nx:s0:body#2");
    assert_eq!(result.ir().model.faces.len(), 1);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_accepts_complete_fixed_nodes_across_the_u32_identifier_domain() {
    let mut stream = topology_partition_stream();
    let fixed_nodes = crate::topology::Graph::parse(&stream)
        .nodes
        .values()
        .filter(|node| node.kind != 17)
        .map(|node| (node.pos, node.shift))
        .collect::<Vec<_>>();
    for (ordinal, (pos, shift)) in fixed_nodes.into_iter().enumerate() {
        let node_id = u32::MAX - u32::try_from(ordinal).unwrap();
        stream[pos + 4 + shift..pos + 8 + shift].copy_from_slice(&node_id.to_be_bytes());
    }

    let graph = crate::topology::Graph::parse(&stream);

    assert_eq!(graph.body_shape_shells().len(), 1);
    assert_eq!(graph.body_shape_face_count(), 1);
    assert!(graph.has_complete_body_topology());
    assert!(graph
        .nodes
        .values()
        .filter(|node| node.kind != 17)
        .all(|node| View::u32_be_at(&node.bytes, 4).is_some_and(|id| id > 1_000_000)));

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_accepts_high_node_identity_among_low_identity_neighbors() {
    let mut stream = topology_partition_stream();
    let initial_graph = crate::topology::Graph::parse(&stream);
    let face = initial_graph.get(14, 4).unwrap();
    let node_id_offset = face.pos + 4 + face.shift;
    stream[node_id_offset..node_id_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());

    let graph = crate::topology::Graph::parse(&stream);

    assert_eq!(
        graph.get(14, 4).and_then(crate::topology::Node::node_id),
        Some(u32::MAX)
    );
    assert_eq!(graph.body_shape_face_count(), 1);
    assert!(graph.has_complete_body_topology());
}

#[test]
fn topology_admits_high_identity_carriers_from_typed_topology_slots() {
    let mut stream = topology_partition_stream();
    let initial_graph = Graph::parse(&stream);
    for (kind, xmt) in [(50, 6), (30, 9), (29, 11)] {
        let node = initial_graph.get(kind, xmt).unwrap();
        let node_id_offset = node.pos + 4 + node.shift;
        stream[node_id_offset..node_id_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    }

    let graph = Graph::parse(&stream);

    for (kind, xmt) in [(50, 6), (30, 9), (29, 11)] {
        assert_eq!(
            graph.get(kind, xmt).and_then(|node| node.u32_at(4)),
            Some(u32::MAX)
        );
    }
    assert!(graph.has_complete_body_topology());

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(result.ir().model.curves.len(), 1);
    assert_eq!(result.ir().model.points.len(), 1);
}

#[test]
fn topology_rejects_unreferenced_high_identity_carrier() {
    let mut stream = topology_partition_stream();
    let plane_pos = stream
        .windows(4)
        .position(|window| window == [0, 50, 0, 6])
        .unwrap();
    let mut unreferenced = stream[plane_pos..plane_pos + 91].to_vec();
    put_ref(&mut unreferenced, 2, 99);
    unreferenced[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
    stream.extend(unreferenced);
    let line_pos = stream
        .windows(4)
        .position(|window| window == [0, 30, 0, 9])
        .unwrap();
    let mut successor = stream[line_pos..line_pos + 67].to_vec();
    put_ref(&mut successor, 2, 100);
    stream.extend(successor);

    let graph = Graph::parse(&stream);

    assert!(graph.get(50, 99).is_none());
    assert!(graph.get(30, 100).is_some());
    assert!(graph.has_complete_body_topology());
}

#[test]
fn topology_admits_high_identity_region_from_shell_ownership() {
    let mut stream = topology_partition_stream();
    let initial_graph = Graph::parse(&stream);
    let region = initial_graph.get(19, 12).unwrap();
    let node_id_offset = region.pos + 4 + region.shift;
    stream[node_id_offset..node_id_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());

    let graph = Graph::parse(&stream);

    assert_eq!(graph.get(19, 12).and_then(Node::node_id), Some(u32::MAX));
    assert!(graph.has_complete_body_topology());
}

#[test]
fn topology_closes_high_identity_procedural_surface_dependencies() {
    let mut stream = offset_surface_topology_partition_stream();
    let initial_graph = Graph::parse(&stream);
    let offset = initial_graph.get(60, 12).unwrap();
    let node_id_offset = offset.pos + 4 + offset.shift;
    stream[node_id_offset..node_id_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());

    let graph = Graph::parse(&stream);

    assert_eq!(
        graph.get(60, 12).and_then(|node| node.u32_at(4)),
        Some(u32::MAX)
    );
    assert_eq!(graph.offset_surfaces().len(), 1);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
}

#[test]
fn topology_resolves_kernel_node_identity_only_within_one_unique_family() {
    let mut stream = topology_partition_stream();
    let graph = crate::topology::Graph::parse(&stream);
    let face = graph.get(14, 4).unwrap();
    let node_id = face.node_id().unwrap();
    assert_eq!(graph.unique_xmt_by_node_id(14, node_id), Some(4));
    assert_eq!(graph.unique_xmt_by_node_id(16, node_id), Some(8));

    let mut duplicate = face.bytes.clone();
    duplicate[3] = 39;
    stream.extend(duplicate);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.get(14, 39).and_then(Node::node_id), Some(node_id));
    assert_eq!(graph.unique_xmt_by_node_id(14, node_id), None);
}

#[test]
fn topology_accepts_cached_last_face_and_implicit_region_identity() {
    let mut stream = topology_partition_stream();
    let shell = stream
        .windows(4)
        .position(|window| window == [0, 13, 0, 3])
        .expect("shell record");
    put_ref(&mut stream, shell + 22, 4);
    let region = stream
        .windows(4)
        .position(|window| window == [0, 19, 0, 12])
        .expect("region record");
    stream[region..region + 16].fill(0xff);
    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 1);
    put_ref(&mut second_face, 22, 1);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(19, 12).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);
    assert_eq!(graph.body_shape_face_count(), 2);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.regions[0].id.0, "nx:s0:region#12");
    assert_eq!(result.ir().model.faces.len(), 2);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_rejects_nonreciprocal_fin_ring() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 8, 99);
    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.face_loop_rings(4).is_none());

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.loops.is_empty());
    assert!(result.ir().model.coedges.is_empty());
    assert!(result.ir().model.edges.is_empty());

    let mut broken_partner = topology_partition_stream();
    let fin = broken_partner
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut broken_partner, fin + 14, 99);
    assert!(crate::topology::Graph::parse(&broken_partner)
        .face_loop_rings(4)
        .is_none());
}

#[test]
fn topology_accepts_fixed_record_envelope_escape() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    stream.insert(fin + 2, 0xff);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(17, 7).unwrap().attribute_field_offset(),
        Some(fin + 5)
    );
    assert_eq!(graph.face_loop_rings(4).unwrap().len(), 1);
}

#[test]
fn topology_prefers_escaped_body_shape_over_direct_extended_xmt() {
    let mut stream = topology_partition_stream();
    let shell = stream
        .windows(4)
        .position(|window| window == [0, 13, 0, 3])
        .expect("shell record");
    stream.insert(shell + 2, 0xff);

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.get(13, 3).map(|node| node.pos), Some(shell));
    assert_eq!(graph.body_shape_shells().len(), 1);
    assert_eq!(graph.body_shape_face_count(), 1);
}

#[test]
fn topology_iterates_each_record_family_in_physical_order() {
    let mut stream = Vec::new();
    for (xmt, x) in [(77, 0.01), (3, 0.02)] {
        let mut point = record(29, 40);
        put_ref(&mut point, 2, xmt);
        put_vec3(&mut point, 16, [x, 0.0, 0.0]);
        stream.extend(point);
    }

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.of_kind(29).map(|node| node.xmt).collect::<Vec<_>>(),
        vec![77, 3]
    );
}

#[test]
fn topology_invalid_candidate_cannot_shadow_later_valid_record() {
    let mut stream = record(14, 39);
    put_ref(&mut stream, 2, 4);
    stream.extend(topology_partition_stream());

    let graph = crate::topology::Graph::parse(&stream);
    let face = graph.get(14, 4).expect("valid later FACE");
    assert!(face.pos >= 39);
    assert!(face.face_fields().is_some());
}

#[test]
fn topology_selects_one_candidate_at_an_ambiguous_record_offset() {
    let mut stream = vec![0; 26];
    stream[..7].copy_from_slice(&[0, 12, 0xff, 0xfe, 0x00, 0x02, 0x01]);
    let mut successor = record(12, 24);
    put_ref(&mut successor, 2, 3);
    stream.extend_from_slice(&successor);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.of_kind(12).count(), 2);
    assert_eq!(graph.at_pos(0).map(|node| node.xmt), Some(65_536));
    assert_eq!(graph.at_pos(26).map(|node| node.xmt), Some(3));
}

#[test]
fn topology_disambiguates_direct_large_index_from_escaped_compact_record() {
    let mut stream = vec![0; 25];
    stream[..6].copy_from_slice(&[0, 17, 0xff, 0x7f, 0x00, 0x01]);
    for index in 0..8 {
        put_ref(&mut stream, 6 + index * 2, 2);
    }
    stream[22..24].copy_from_slice(b"++");
    stream[24] = b'+';

    let mut successor = record(17, 23);
    put_ref(&mut successor, 2, 7);
    for index in 0..9 {
        put_ref(&mut successor, 4 + index * 2, 2);
    }
    successor[22] = b'+';
    stream.extend_from_slice(&successor);

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.at_pos(0).map(|node| node.xmt), Some(32_896));
    assert_eq!(graph.at_pos(0).map(crate::topology::Node::end), Some(25));
    assert_eq!(graph.at_pos(25).map(|node| node.xmt), Some(7));

    let mut ambiguous = stream[..25].to_vec();
    ambiguous.extend_from_slice(&[0; 5]);
    assert!(crate::topology::Graph::parse(&ambiguous)
        .at_pos(0)
        .is_none());
}

#[test]
fn topology_rejects_duplicate_fixed_record_identity() {
    let mut first = record(29, 40);
    put_ref(&mut first, 2, 11);
    put_vec3(&mut first, 16, [0.01, 0.02, 0.03]);
    let mut duplicate = record(29, 40);
    put_ref(&mut duplicate, 2, 11);
    put_vec3(&mut duplicate, 16, [0.04, 0.05, 0.06]);
    first.extend(duplicate);

    let graph = crate::topology::Graph::parse(&first);
    assert!(graph.get(29, 11).is_none());
    assert!(graph.of_kind(29).next().is_none());
}

#[test]
fn topology_rejects_duplicate_identity_instead_of_preferring_body_shape() {
    let mut stream = topology_partition_stream();
    let mut duplicate = record(13, 24);
    put_ref(&mut duplicate, 2, 3);
    put_ref(&mut duplicate, 8, 2);
    put_ref(&mut duplicate, 10, 2);
    put_ref(&mut duplicate, 12, 2);
    put_ref(&mut duplicate, 14, 4);
    put_ref(&mut duplicate, 16, 0);
    put_ref(&mut duplicate, 18, 0);
    put_ref(&mut duplicate, 20, 12);
    put_ref(&mut duplicate, 22, 0);
    stream.extend(duplicate);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(13, 3).is_none());
}

#[test]
fn topology_rejects_overlapping_candidates_without_ranking() {
    let first = NodeCandidate {
        kind: 29,
        xmt: 11,
        pos: 0,
        shift: 0,
        end: 24,
    };
    let second = NodeCandidate {
        kind: 29,
        xmt: 12,
        pos: 8,
        shift: 0,
        end: 32,
    };

    assert!(Graph::select_non_overlapping_candidates(&[], vec![first, second]).is_empty());
}

#[test]
fn topology_ownership_candidate_cannot_suppress_typed_candidate() {
    let mut face = record(14, 39);
    put_ref(&mut face, 2, 4);
    put_f64(&mut face, 10, 0.000_2);
    put_ref(&mut face, 18, 1);
    put_ref(&mut face, 20, 1);
    put_ref(&mut face, 22, 1);
    put_ref(&mut face, 24, 3);
    put_ref(&mut face, 26, 6);
    face[28] = b'+';

    let mut stream = vec![0, 12];
    stream.extend(face);

    let graph = Graph::parse(&stream);
    assert!(graph.get(14, 4).is_some());
    assert!(graph.get(12, 14).is_none());
}

#[test]
fn topology_resolves_ownership_overlap_before_duplicate_identity() {
    let mut outer = record(12, 24);
    put_ref(&mut outer, 2, 7);
    outer[8..10].copy_from_slice(&[0, 12]);
    put_ref(&mut outer, 10, 7);
    let mut successor = record(12, 24);
    put_ref(&mut successor, 2, 8);

    let mut stream = outer;
    stream.extend(successor);

    let graph = Graph::parse(&stream);
    assert_eq!(graph.get(12, 7).map(|node| node.pos), Some(0));
    assert_eq!(graph.get(12, 8).map(|node| node.pos), Some(24));
}

#[test]
fn topology_retains_non_overlapping_ownership_records() {
    let graph = Graph::parse(&topology_partition_stream());

    assert!(graph.get(12, 2).is_some());
    assert!(graph.get(19, 12).is_some());
}

#[test]
fn topology_resolves_overlap_before_duplicate_identity() {
    let stream = vec![0; 40];
    let outer = NodeCandidate {
        kind: 29,
        xmt: 11,
        pos: 0,
        shift: 0,
        end: 40,
    };
    let embedded = NodeCandidate {
        kind: 29,
        xmt: 11,
        pos: 8,
        shift: 0,
        end: 32,
    };

    assert!(Graph::select_unique_candidates(vec![outer, embedded]).is_empty());
    let non_overlapping = Graph::select_non_overlapping_candidates(&stream, vec![outer, embedded]);
    let selected = Graph::select_unique_candidates(non_overlapping);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].pos, outer.pos);
    assert_eq!(selected[0].end(), outer.end());
}

#[test]
fn topology_rejects_status_framed_delta_as_fixed_record() {
    let graph = Graph::parse(&variable_status_framed_deltas_stream());

    assert!(graph.of_kind(15).next().is_none());
}

#[test]
fn intersection_data_requires_complete_schema_header() {
    let source = deltas_intersection_curve_stream();
    let header_start = source
        .windows(TYPE_38_SCHEMA_HEADER.len())
        .position(|window| window == TYPE_38_SCHEMA_HEADER)
        .expect("schema header");
    let after_header = header_start + TYPE_38_SCHEMA_HEADER.len();
    let record_start = source[after_header..]
        .iter()
        .position(|byte| *byte == 0x5a)
        .map(|offset| after_header + offset)
        .expect("standalone intersection-data record");

    let mut incomplete_header =
        source[header_start..header_start + TYPE_38_SCHEMA_HEADER.len() - 1].to_vec();
    incomplete_header.push(0xfe);
    incomplete_header.extend_from_slice(&source[record_start..]);
    assert!(intersection_data_curves(&incomplete_header).is_empty());
    assert!(crate::deltas::walk(&incomplete_header)
        .records
        .iter()
        .all(|record| record.kind != 90));
}
