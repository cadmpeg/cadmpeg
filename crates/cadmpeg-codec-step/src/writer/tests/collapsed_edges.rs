// SPDX-License-Identifier: Apache-2.0
//! STEP writer handling of an edge collapsed to one vertex.

#![allow(clippy::unwrap_used)]

use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepSchema, StepWriteOptions};

/// A loop of [`unit_cube`] with an edge collapsed to one of its corners
/// added after the coedge that arrives there, as a decoder adds one at a
/// sphere's pole, or with one whose two ends are different vertices, or
/// (`alone`) with the collapsed edge and a second coedge of it as the whole
/// ring.
fn cube_with_a_collapsed_edge(same_vertex: bool, alone: bool) -> CadIr {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId};
    use cadmpeg_ir::topology::{Coedge, Edge, EdgeCarrier, Sense};
    let mut source = unit_cube().expect("unit cube fixture is admitted");
    let (loop_index, ring) = source
        .model
        .loops
        .iter()
        .enumerate()
        .find(|(_, loop_)| loop_.coedges().len() >= 3)
        .map(|(index, loop_)| (index, loop_.coedges().to_vec()))
        .expect("unit cube has an edge loop");
    let ends = |id: &CoedgeId| {
        let coedge = source.model.coedges.iter().find(|c| c.id == *id).unwrap();
        let edge = source
            .model
            .edges
            .iter()
            .find(|e| e.id == coedge.edge)
            .unwrap();
        match coedge.sense {
            Sense::Forward => (edge.start.clone(), edge.end.clone()),
            Sense::Reversed => (edge.end.clone(), edge.start.clone()),
        }
    };
    let (other, corner) = ends(&ring[0]);
    let position = {
        let vertex = source
            .model
            .vertices
            .iter()
            .find(|v| v.id == corner)
            .unwrap();
        source
            .model
            .points
            .iter()
            .find(|p| p.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    let curve = CurveId::mint("test:model:curve#collapsed").unwrap();
    let edge = EdgeId::mint("test:model:edge#collapsed").unwrap();
    let coedge = CoedgeId::mint("test:model:coedge#collapsed").unwrap();
    source.model.curves.push(Curve {
        id: curve.clone(),
        parameter_range: None,
        source_object: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
            cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(position).unwrap(),
        )),
    });
    source.model.edges.push(Edge {
        id: edge.clone(),
        carrier: EdgeCarrier::unbounded(Some(curve)),
        start: if same_vertex { corner.clone() } else { other },
        end: corner,
        tolerance: None,
    });
    source.model.coedges.push(Coedge {
        id: coedge.clone(),
        owner_loop: source.model.loops[loop_index].id.clone(),
        edge,
        radial_next: coedge.clone(),
        sense: Sense::Forward,
        use_curve: None,
        pcurves: Vec::new(),
    });
    let mut coedges = ring;
    coedges.insert(1, coedge.clone());
    if alone {
        let twin = CoedgeId::mint("test:model:coedge#collapsed-twin").unwrap();
        let mut second = source.model.coedges.last().unwrap().clone();
        second.id = twin.clone();
        second.radial_next = twin.clone();
        source.model.coedges.push(second);
        coedges = vec![coedge, twin];
    }
    let vertex_uses = source.model.loops[loop_index]
        .anchored_vertex_uses()
        .to_vec();
    source.model.loops[loop_index]
        .replace_ring(
            &cadmpeg_test_support::service_decode_context(),
            coedges,
            vertex_uses,
        )
        .expect("the ring with a collapsed edge is valid");
    source
}

fn oriented_edges(source: &CadIr) -> (usize, usize, bool) {
    let mut bytes = Vec::new();
    let report = write_step(
        source,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("the cube writes");
    let text = String::from_utf8(bytes).expect("STEP is UTF-8");
    let unordered = report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::LoopNoContinuousOrdering.kind());
    (
        text.matches("ORIENTED_EDGE(").count(),
        text.matches("ADVANCED_FACE(").count(),
        unordered,
    )
}

/// An edge collapsed to its one vertex is left out of its loop: the loop is
/// continuous through that vertex, and a reader closes the surface's
/// singular point in parameter space itself. A zero-length edge with a
/// pcurve line at a sphere's pole makes the face invalid in `OpenCASCADE`.
/// An edge on a collapsed curve between two different vertices is not that
/// edge and stays.
#[test]
fn writer_leaves_an_edge_collapsed_to_one_vertex_out_of_its_loop() {
    let plain = oriented_edges(&unit_cube().expect("unit cube fixture is admitted"));
    assert_eq!(plain.1, 6);
    assert_eq!(
        oriented_edges(&cube_with_a_collapsed_edge(true, false)),
        plain
    );
    let kept = oriented_edges(&cube_with_a_collapsed_edge(false, false));
    assert_ne!(
        kept, plain,
        "an edge between two vertices is written or reported"
    );
    // A ring of collapsed edges alone keeps them: its four edges become
    // two, and the face is still written.
    let alone = oriented_edges(&cube_with_a_collapsed_edge(true, true));
    assert_eq!((alone.0, alone.1), (plain.0 - 2, plain.1));
}
