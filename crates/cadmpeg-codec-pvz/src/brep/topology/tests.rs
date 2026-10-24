// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use cadmpeg_core::decode::WorkBudget;

use super::{coincident_partners, domain_normal, resolve, work_limit, Topology};
use crate::brep::eval::{dot, sub};
use crate::brep::model::{Conic, CurveGeometry, EdgeCurve, ExactModel, Precision};
use crate::brep::resolve::{domain_point, PcurveSlot};
use crate::test_support::exact::{
    cube_stream, split_shared_edges, Stream, CUBE_SIZE, SINGULAR_USE,
};

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
            let topology = resolve(&model, work_limit(&model)).unwrap();
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
    let topology = resolve(&model, work_limit(&model)).unwrap();
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
    let topology = resolve(&model, work_limit(&model)).unwrap();
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
    let topology = resolve(&model, work_limit(&model)).unwrap();
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
    let topology = resolve(&model, work_limit(&model)).unwrap();
    assert!(topology.singular_edges.is_empty());
    assert_ne!(topology.slot_vertex[0], topology.slot_vertex[1]);
}

#[test]
fn edges_stored_once_per_use_pair_into_one_class() {
    for reverse in [false, true] {
        let mut model = parse(cube_stream(
            true,
            [false, true, false, false, true, false],
            [false; 6],
        ));
        let copies = split_shared_edges(&mut model, reverse);
        assert_eq!(copies.len(), 12);
        let topology = resolve(&model, work_limit(&model)).unwrap();
        for (edge, copy) in &copies {
            assert_eq!(topology.partners.get(copy), Some(&(*edge, !reverse)));
        }
        assert_outward(&model, &topology);
        let used: std::collections::BTreeSet<usize> =
            topology.slot_vertex[..48].iter().copied().collect();
        assert_eq!(used.len(), 8);
    }
}

#[test]
fn one_unpaired_edge_keeps_the_outward_test() {
    let mut model = parse(cube_stream(true, [false; 6], [false; 6]));
    let copies = split_shared_edges(&mut model, false);
    let moved = copies[&0];
    let CurveGeometry::Line { origin, .. } = &mut model.edges[moved].geometry else {
        panic!("line");
    };
    origin[2] += 1.0e-3;
    let topology = resolve(&model, work_limit(&model)).unwrap();
    assert_eq!(topology.partners.len(), 11);
    assert_outward(&model, &topology);
}

/// A model with the edge curves `curves`, each used once by its own face.
fn single_use_edges(curves: Vec<CurveGeometry<3>>) -> (ExactModel, Vec<Vec<(usize, bool)>>) {
    let uses = (0..curves.len()).map(|edge| vec![(edge, true)]).collect();
    let model = ExactModel {
        precision: Precision::Double,
        faces: Vec::new(),
        loops: Vec::new(),
        edges: curves
            .into_iter()
            .map(|geometry| EdgeCurve {
                geometry,
                offset: 0,
            })
            .collect(),
        pcurves: Vec::new(),
        opaque: Vec::new(),
    };
    (model, uses)
}

fn partners_of(curves: Vec<CurveGeometry<3>>) -> std::collections::BTreeMap<usize, (usize, bool)> {
    let (model, uses) = single_use_edges(curves);
    coincident_partners(
        &model,
        &uses,
        &std::collections::BTreeSet::new(),
        &WorkBudget::new(usize::MAX),
    )
}

fn line(start: [f64; 3], end: [f64; 3]) -> CurveGeometry<3> {
    CurveGeometry::Line {
        origin: start,
        direction: sub(end, start),
    }
}

#[test]
fn partner_ends_meet_across_grid_cells() {
    let cell = crate::brep::resolve::ENDPOINT_TOLERANCE;
    let start = [-408.0 * cell + 1.0e-12, 0.0, 0.0];
    let end = [0.0, 0.01, 0.0];
    let shifted = |point: [f64; 3]| [point[0] - 2.0e-12, point[1], point[2]];
    let partners = partners_of(vec![line(start, end), line(shifted(end), shifted(start))]);
    assert_eq!(partners.get(&1), Some(&(0, false)));
}

#[test]
fn partners_lie_on_each_other_between_their_ends() {
    let (start, end) = ([0.0; 3], [0.01, 0.0, 0.0]);
    let uneven = CurveGeometry::Polyline(vec![end, [0.008, 0.0, 0.0], start]);
    assert_eq!(
        partners_of(vec![line(start, end), uneven]).get(&1),
        Some(&(0, false))
    );
    let arc = CurveGeometry::Circle(Conic {
        center: [0.005, 0.0, 0.0],
        x: [-0.005, 0.0, 0.0],
        y: [0.0, 0.005, 0.0],
        range: Some([0.0, std::f64::consts::PI]),
    });
    assert!(partners_of(vec![line(start, end), arc]).is_empty());
}

#[test]
fn an_edge_pairs_with_the_nearest_coincident_edge() {
    let (start, end) = ([0.0; 3], [0.01, 0.0, 0.0]);
    let offset = |point: [f64; 3]| [point[0], point[1], point[2] + 1.0e-6];
    let partners = partners_of(vec![
        line(start, end),
        line(offset(start), offset(end)),
        line(offset(end), offset(start)),
        line(end, start),
    ]);
    assert_eq!(partners.get(&3), Some(&(0, false)));
    assert_eq!(partners.get(&2), Some(&(1, false)));
}

#[test]
fn points_have_no_partner() {
    let point = CurveGeometry::Point([0.0; 3]);
    assert!(partners_of(vec![point.clone(), point]).is_empty());
}
