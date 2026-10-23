// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::{domain_normal, resolve, Topology};
use crate::brep::eval::{dot, sub};
use crate::brep::model::ExactModel;
use crate::brep::resolve::{domain_point, PcurveSlot};
use crate::test_support::exact::{cube_stream, Stream, CUBE_SIZE, SINGULAR_USE};

fn parse(stream: Stream) -> ExactModel {
    let id = stream.id();
    let bytes = stream.finish();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    crate::brep::parse(&ctx, id, root).unwrap()
}

/// Every face normal of the resolved sides points away from the cube
/// centre.
fn assert_outward(model: &ExactModel, topology: &Topology) {
    let centre = [CUBE_SIZE / 2.0; 3];
    for (index, face) in topology.faces.iter().enumerate() {
        assert!(face.side_derived, "face {index}");
        let record = &model.faces[index];
        let point = domain_point(record, PcurveSlot::Native, [0.5, 0.5]).unwrap();
        let normal = domain_normal(model, index, PcurveSlot::Native, [0.5, 0.5]).unwrap();
        let side = if face.reversed { -1.0 } else { 1.0 } * face.winding;
        assert!(dot(normal, sub(point, centre)) * side > 0.0, "face {index}");
    }
}

#[test]
fn closed_cube_turns_every_face_outward() {
    for inward in [[false; 6], [true, false, true, false, false, true]] {
        for reversed in [[false; 6], [false, true, true, false, true, false]] {
            let model = parse(cube_stream(true, inward, reversed));
            let topology = resolve(&model).unwrap();
            assert_eq!(topology.side_conflicts, 0);
            assert_outward(&model, &topology);
            for face in &topology.faces {
                assert!(face.loops.iter().all(|loop_| loop_.gap == Some(0.0)));
                assert_eq!(face.regions.len(), 1);
            }
        }
    }
}

#[test]
fn cube_corners_join_into_eight_vertices() {
    let model = parse(cube_stream(false, [false; 6], [false; 6]));
    let topology = resolve(&model).unwrap();
    let used: std::collections::BTreeSet<usize> =
        topology.slot_vertex[..24].iter().copied().collect();
    assert_eq!(used.len(), 8);
    assert_eq!(topology.split_junctions, 0);
    assert!(topology
        .vertex_spread
        .iter()
        .all(|spread| *spread <= f64::EPSILON));
}

#[test]
fn coincident_single_use_edges_carry_the_side() {
    let mut stream = cube_stream(true, [false, true, false, false, true, false], [false; 6]);
    let duplicate = stream
        .payload()
        .u8(5)
        .u8(0)
        .scalars(&[0.0, 0.0, 0.0, 0.0, CUBE_SIZE, 0.0])
        .u8(0)
        .bytes();
    stream.record(crate::test_support::exact::EDGE_CURVE, &duplicate);
    let model = parse(stream);
    let mut model = model;
    let (loop_index, position) = model
        .loops
        .iter()
        .enumerate()
        .find_map(|(index, record)| {
            let position = record
                .coedges
                .iter()
                .position(|coedge| coedge.edge == Some(0))?;
            Some((index, position))
        })
        .unwrap();
    model.loops[loop_index].coedges[position].edge = Some(12);
    let topology = resolve(&model).unwrap();
    assert_outward(&model, &topology);
}

#[test]
fn collapsed_singular_edge_joins_its_ends() {
    let mut stream = Stream::new(true);
    stream.plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let singular = stream.payload().u32(0).scalar(0.0).scalar(1.0).bytes();
    stream.record(SINGULAR_USE, &singular).terminator();
    stream.line_edge([0.5, 0.5, 0.0], [0.0; 3]);
    let model = parse(stream);
    let topology = resolve(&model).unwrap();
    assert!(topology.singular_edges.contains(&0));
    assert_eq!(topology.slot_vertex[0], topology.slot_vertex[1]);
}

#[test]
fn extended_singular_edge_keeps_its_ends() {
    let mut stream = Stream::new(true);
    stream.plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let singular = stream.payload().u32(0).scalar(0.0).scalar(1.0).bytes();
    stream.record(SINGULAR_USE, &singular).terminator();
    stream.line_edge([0.0; 3], [0.01, 0.0, 0.0]);
    let model = parse(stream);
    let topology = resolve(&model).unwrap();
    assert!(topology.singular_edges.is_empty());
    assert_ne!(topology.slot_vertex[0], topology.slot_vertex[1]);
}
