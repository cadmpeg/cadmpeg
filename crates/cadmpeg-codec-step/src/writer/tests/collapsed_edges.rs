// SPDX-License-Identifier: Apache-2.0
//! STEP writer handling of an edge collapsed to one vertex.

#![allow(clippy::unwrap_used)]

use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepSchema, StepWriteOptions};

/// How the collapsed edge sits in the loop of [`cube_with_a_collapsed_edge`].
#[derive(Clone, Copy, PartialEq)]
enum Ring {
    /// After the coedge that arrives at its vertex, among the real edges.
    Beside,
    /// The same place, but with its two ends at different vertices.
    BetweenVertices,
    /// The collapsed edge and a second coedge of it are the whole ring.
    Alone,
    /// As [`Ring::Alone`], with a pcurve on each coedge.
    AloneWithPcurves,
    /// After a closed edge on a circle through its vertex, the two the
    /// whole ring.
    AfterAClosedCircle,
}

/// A loop of [`unit_cube`] with an edge collapsed to one of its corners, as
/// a decoder adds one at a sphere's pole, placed as `ring` says.
fn cube_with_a_collapsed_edge(ring_kind: Ring) -> CadIr {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, DegenerateCurve};
    use cadmpeg_ir::geometry::pcurve::{
        Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs, PcurveNurbsPoles,
    };
    use cadmpeg_ir::geometry::DirectedParameterRange;
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, PcurveId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::{Coedge, Edge, EdgeCarrier, PcurveUse, Sense};
    let same_vertex = ring_kind != Ring::BetweenVertices;
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
    let corner_vertex = corner.clone();
    let curve = CurveId::mint("test:model:curve#collapsed").unwrap();
    let edge = EdgeId::mint("test:model:edge#collapsed").unwrap();
    let coedge = CoedgeId::mint("test:model:coedge#collapsed").unwrap();
    source.model.curves.push(Curve {
        native_ref: None,
        id: curve.clone(),
        parameter_range: None,
        source_object: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
            DegenerateCurve::try_new(position).unwrap(),
        )),
    });
    source.model.edges.push(Edge {
        native_ref: None,
        id: edge.clone(),
        carrier: EdgeCarrier::unbounded(Some(curve)),
        start: if same_vertex { corner.clone() } else { other },
        end: corner.clone(),
        tolerance: None,
    });
    source.model.coedges.push(Coedge {
        native_ref: None,
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
    if matches!(ring_kind, Ring::Alone | Ring::AloneWithPcurves) {
        let twin = CoedgeId::mint("test:model:coedge#collapsed-twin").unwrap();
        let mut second = source.model.coedges.last().unwrap().clone();
        second.id = twin.clone();
        second.radial_next = twin.clone();
        source.model.coedges.push(second);
        coedges = vec![coedge.clone(), twin];
    }
    if ring_kind == Ring::AloneWithPcurves {
        let pcurve = PcurveId::mint("test:model:pcurve#collapsed").unwrap();
        source.model.pcurves.push(Pcurve {
            native_ref: None,
            id: pcurve.clone(),
            geometry: PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    PcurveNurbsPoles::Polynomial {
                        points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                    },
                    false,
                )
                .unwrap(),
            },
            metadata: PcurveMetadata::default(),
        });
        for coedge in source
            .model
            .coedges
            .iter_mut()
            .filter(|coedge| coedges.contains(&coedge.id))
        {
            coedge.pcurves.push(PcurveUse {
                pcurve: pcurve.clone(),
                isoparametric: None,
                parameter_range: Some(DirectedParameterRange::new([0.0, 1.0]).unwrap()),
            });
        }
    }
    if ring_kind == Ring::AfterAClosedCircle {
        let circle = CurveId::mint("test:model:curve#closed-circle").unwrap();
        let circle_edge = EdgeId::mint("test:model:edge#closed-circle").unwrap();
        let circle_coedge = CoedgeId::mint("test:model:coedge#closed-circle").unwrap();
        source.model.curves.push(Curve {
            native_ref: None,
            id: circle.clone(),
            parameter_range: None,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(position.x - 0.25, position.y, position.z),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    0.25,
                )
                .unwrap(),
            )),
        });
        source.model.edges.push(Edge {
            native_ref: None,
            id: circle_edge.clone(),
            carrier: EdgeCarrier::unbounded(Some(circle)),
            start: corner_vertex.clone(),
            end: corner_vertex,
            tolerance: None,
        });
        source.model.coedges.push(Coedge {
            native_ref: None,
            id: circle_coedge.clone(),
            owner_loop: source.model.loops[loop_index].id.clone(),
            edge: circle_edge,
            radial_next: circle_coedge.clone(),
            sense: Sense::Forward,
            use_curve: None,
            pcurves: Vec::new(),
        });
        coedges = vec![circle_coedge, coedge];
    }
    let vertex_uses = source.model.loops[loop_index]
        .anchored_vertex_uses()
        .to_vec();
    source.model.loops[loop_index]
        .replace_ring(coedges, vertex_uses)
        .expect("the ring with a collapsed edge is valid");
    source
}

/// What the writer made of a model: its `ORIENTED_EDGE`, `ADVANCED_FACE`
/// and `VERTEX_LOOP` records, and the losses it reported.
#[derive(Debug, PartialEq)]
struct Written {
    oriented_edges: usize,
    faces: usize,
    vertex_loops: usize,
    losses: Vec<String>,
}

fn written(source: &CadIr) -> Written {
    let mut bytes = Vec::new();
    let report = write_step(
        source,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("the cube writes");
    let text = String::from_utf8(bytes).expect("STEP is UTF-8");
    Written {
        oriented_edges: text.matches("ORIENTED_EDGE(").count(),
        faces: text.matches("ADVANCED_FACE(").count(),
        vertex_loops: text.matches("VERTEX_LOOP(").count(),
        losses: report
            .losses
            .iter()
            .map(|loss| loss.code.to_string())
            .collect(),
    }
}

/// An edge collapsed to its one vertex is left out of its loop: the loop is
/// continuous through that vertex, and a reader closes the surface's
/// singular point in parameter space itself. A zero-length edge with a
/// pcurve line at a sphere's pole makes the face invalid in `OpenCASCADE`.
/// An edge on a collapsed curve between two different vertices is not that
/// edge and stays.
#[test]
fn writer_leaves_an_edge_collapsed_to_one_vertex_out_of_its_loop() {
    let plain = written(&unit_cube().expect("unit cube fixture is admitted"));
    assert_eq!((plain.faces, plain.vertex_loops), (6, 0));
    assert!(plain.losses.is_empty(), "{:?}", plain.losses);
    // The collapsed edge and its carrier are left out without a loss.
    assert_eq!(written(&cube_with_a_collapsed_edge(Ring::Beside)), plain);
    let kept = written(&cube_with_a_collapsed_edge(Ring::BetweenVertices));
    assert_ne!(
        kept, plain,
        "an edge between two vertices is written or reported"
    );
}

/// A ring of collapsed edges alone, with no coedge geometry, is the point
/// loop path: a `VERTEX_LOOP`, its four edges become none, and the face is
/// still written.
#[test]
fn writer_writes_a_ring_of_bare_collapsed_edges_as_a_vertex_loop() {
    let plain = written(&unit_cube().expect("unit cube fixture is admitted"));
    let alone = written(&cube_with_a_collapsed_edge(Ring::Alone));
    assert_eq!(
        (alone.oriented_edges, alone.faces, alone.vertex_loops),
        (plain.oriented_edges - 4, plain.faces, 1)
    );
}

/// A ring of collapsed edges alone whose coedges carry pcurves is not a
/// point loop, and keeps its edges: they are all the loop has, and without
/// them the face would have no bound.
#[test]
fn writer_keeps_a_ring_of_collapsed_edges_with_pcurves() {
    let plain = written(&unit_cube().expect("unit cube fixture is admitted"));
    let alone = written(&cube_with_a_collapsed_edge(Ring::AloneWithPcurves));
    assert_eq!(
        (alone.oriented_edges, alone.faces, alone.vertex_loops),
        (plain.oriented_edges - 4 + 2, plain.faces, 0)
    );
}

/// A closed edge on a circle starts and ends at one vertex but has extent,
/// so a collapsed edge beside it is still left out.
#[test]
fn writer_leaves_a_collapsed_edge_out_beside_a_closed_circle() {
    let plain = written(&unit_cube().expect("unit cube fixture is admitted"));
    let circle = written(&cube_with_a_collapsed_edge(Ring::AfterAClosedCircle));
    assert_eq!(
        (circle.oriented_edges, circle.faces, circle.vertex_loops),
        (plain.oriented_edges - 4 + 1, plain.faces, 0)
    );
}
