// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use crate::test_support::build_prt;
use crate::test_support::push_generated_plane_row;
use crate::test_support::push_generated_topology_row;
use crate::test_support::visibgeom_payload;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container::{self};
use crate::CreoCodec;

use super::{
    edge_start_vertex_pairs, edge_vertex_pairs, face_components, selected_body_count,
    vertex_incident_faces, vertex_orbits, HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence,
};
use crate::curve::CurveTopologyRow;

fn row(id: u32, next: u32) -> CurveTopologyRow {
    CurveTopologyRow {
        id,
        type_byte: 0,
        feature_id: 0,
        directions: [1, 1],
        faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(20)],
        next_edges: [next, next],
        offset: 0,
    }
}

fn build(rows: &[CurveTopologyRow]) -> (Vec<HalfEdge>, Vec<super::Loop>) {
    super::build(rows).expect("derived topology")
}

fn paired_incidence() -> [HalfEdgeVertexIncidence; 2] {
    [
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            },
            start_vertex_id: std::num::NonZeroU32::new(10).expect("one-based vertex fixture"),
            end_vertex_id: std::num::NonZeroU32::new(20),
        },
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::One,
            },
            start_vertex_id: std::num::NonZeroU32::new(20).expect("one-based vertex fixture"),
            end_vertex_id: None,
        },
    ]
}

#[test]
fn edge_vertex_pairs_reject_duplicate_side_without_a_temporary_vector() {
    let [first, second] = paired_incidence();
    let incidence = [first.clone(), first, second];
    let start = edge_start_vertex_pairs(&incidence);
    let edge = edge_vertex_pairs(&incidence);
    assert!(!start.contains_key(&7));
    assert!(!edge.contains_key(&7));
}

fn orphan_edge() -> HalfEdge {
    HalfEdge {
        id: HalfEdgeId {
            curve_id: 1,
            side: crate::topology::Side::Zero,
        },
        face_id: std::num::NonZeroU32::new(10),
        next: None,
    }
}

#[test]
fn builds_closed_face_side_rings_without_guessing() {
    let (half_edges, loops) = build(&[row(1, 2), row(2, 3), row(3, 1)]);
    assert_eq!(half_edges.len(), 6);
    assert_eq!(loops.len(), 2);
    assert_eq!(loops[0].face_id, std::num::NonZeroU32::new(10));
    assert_eq!(
        loops[0].half_edges(),
        vec![
            HalfEdgeId {
                curve_id: 1,
                side: crate::topology::Side::Zero
            },
            HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::Zero
            },
            HalfEdgeId {
                curve_id: 3,
                side: crate::topology::Side::Zero
            }
        ]
    );
}

#[test]
fn duplicate_curve_identities_do_not_contribute_derived_topology() {
    let rows = [row(1, 2), row(2, 1), row(2, 1)];

    let (half_edges, loops) = build(&rows);
    assert_eq!(half_edges.len(), 2);
    assert!(half_edges.iter().all(|edge| edge.id.curve_id == 1));
    assert!(half_edges.iter().all(|edge| edge.next.is_none()));
    assert!(loops.is_empty());

    let components = face_components(&rows).expect("find the face components");
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].face_ids, [10, 20]);
    assert_eq!(components[0].curve_ids, [1]);
}
#[test]
fn withholds_ambiguous_successors() {
    let (half_edges, loops) = build(&[
        row(1, 2),
        CurveTopologyRow {
            faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(10)],
            ..row(2, 1)
        },
    ]);
    assert!(half_edges.iter().any(|edge| edge.id
        == HalfEdgeId {
            curve_id: 1,
            side: crate::topology::Side::Zero
        }
        && edge.next.is_none()));
    assert!(loops.is_empty());
}

#[test]
fn vertex_orbits_close_predecessor_relations_in_both_directions() {
    let edges = vec![
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 1,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 1,
                side: crate::topology::Side::One,
            },
            face_id: std::num::NonZeroU32::new(20),
            next: Some(HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::Zero,
            }),
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(20),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::One,
            },
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        },
    ];

    let vertices = vertex_orbits(&edges)
        .expect("find the vertex orbits")
        .vertices;
    assert!(vertices.iter().any(|vertex| vertex.half_edges()
        == vec![
            HalfEdgeId {
                curve_id: 1,
                side: crate::topology::Side::Zero,
            },
            HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::Zero,
            },
        ]));
}

#[test]
fn vertex_orbits_do_not_join_ambiguous_predecessors() {
    let first = orphan_edge();
    let second = HalfEdge {
        id: HalfEdgeId {
            side: crate::topology::Side::One,
            ..first.id
        },
        ..orphan_edge()
    };
    let target = HalfEdge {
        id: HalfEdgeId {
            curve_id: 2,
            ..first.id
        },
        ..orphan_edge()
    };
    let linked_first = HalfEdge {
        next: Some(target.id),
        ..first.clone()
    };
    let linked_second = HalfEdge {
        next: Some(target.id),
        ..second.clone()
    };
    for edges in [
        vec![linked_first.clone(), linked_second, target.clone()],
        vec![
            linked_first.clone(),
            linked_first,
            second.clone(),
            target.clone(),
        ],
    ] {
        let orbits = vertex_orbits(&edges).expect("ambiguous predecessor graph");
        assert_eq!(orbits.vertices.len(), 3);
        for (vertex, expected) in orbits.vertices.iter().zip([first.id, second.id, target.id]) {
            assert_eq!(vertex.half_edges(), [expected]);
        }
        assert_eq!(orbits.incidence.len(), edges.len());
    }
}

#[test]
fn vertex_incident_faces_include_both_sides_of_each_orbit_edge() {
    let edges = vec![
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::One,
            },
            face_id: std::num::NonZeroU32::new(20),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 8,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 8,
                side: crate::topology::Side::One,
            },
            face_id: std::num::NonZeroU32::new(30),
            next: None,
        },
    ];
    let vertex = {
        crate::topology::TopologicalVertex::new(
            1,
            vec![
                HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::Zero,
                },
                HalfEdgeId {
                    curve_id: 8,
                    side: crate::topology::Side::Zero,
                },
            ],
        )
    }
    .expect("vertex admission");

    let incident_faces = { vertex_incident_faces(&[vertex], &edges) };
    assert_eq!(
        incident_faces
            .get(&std::num::NonZeroU32::new(1).expect("one-based vertex fixture"))
            .cloned(),
        Some(BTreeSet::from([10, 20, 30]))
    );
}

#[test]
fn edge_vertex_pair_accepts_one_closed_face_and_rejects_disagreement() {
    let incidence = |reverse_end: Option<u32>| {
        vec![
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::Zero,
                },
                start_vertex_id: std::num::NonZeroU32::new(10).expect("one-based vertex fixture"),
                end_vertex_id: std::num::NonZeroU32::new(20),
            },
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::One,
                },
                start_vertex_id: std::num::NonZeroU32::new(20).expect("one-based vertex fixture"),
                end_vertex_id: reverse_end
                    .map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")),
            },
        ]
    };

    assert_eq!(
        edge_vertex_pairs(&incidence(None)).get(&7),
        Some(&[10, 20].map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")))
    );
    assert_eq!(
        edge_vertex_pairs(&incidence(Some(10))).get(&7),
        Some(&[10, 20].map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")))
    );
    assert!(!edge_vertex_pairs(&incidence(Some(30))).contains_key(&7));
}

#[test]
fn edge_start_vertex_pair_survives_an_unresolved_successor() {
    let incidence = vec![
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            },
            start_vertex_id: std::num::NonZeroU32::new(10).expect("one-based vertex fixture"),
            end_vertex_id: None,
        },
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::One,
            },
            start_vertex_id: std::num::NonZeroU32::new(20).expect("one-based vertex fixture"),
            end_vertex_id: None,
        },
    ];

    assert_eq!(
        edge_start_vertex_pairs(&incidence).get(&7),
        Some(&[10, 20].map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")))
    );
    assert!(!edge_vertex_pairs(&incidence).contains_key(&7));
}

#[test]
fn scan_groups_connected_nonzero_face_references() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3\x08\x08\x04\x01\xf6\x0b\x0c\x08\x08\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes_ok(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topology.face_components.len(), 1);
    assert_eq!(
        scan.topology.face_components[0].face_ids(),
        vec![10, 11, 12]
    );
    assert_eq!(scan.topology.face_components[0].curve_ids(), vec![7, 8]);
}

#[test]
fn selects_body_count_in_metadata_precedence_order() {
    assert_eq!(selected_body_count(Some(2), Some(0), 7), Some(2));
    assert_eq!(selected_body_count(None, Some(0), 7), Some(1));
    assert_eq!(selected_body_count(None, None, 7), Some(7));
    assert_eq!(selected_body_count(None, Some(9), 7), None);
    assert_eq!(selected_body_count(None, Some(9), 1), Some(1));
    assert_eq!(selected_body_count(None, Some(0), 0), Some(1));
    assert_eq!(selected_body_count(Some(0), None, 7), None);
}

#[test]
fn scan_builds_topological_vertex_orbits_and_incidence() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x08\x08\0\0\xe3\xe1\xe3\
          \x08\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes_ok(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topology.vertices.len(), 2);
    assert_eq!(
        scan.topology.vertices[0].half_edges(),
        vec![
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero
            },
            crate::topology::HalfEdgeId {
                curve_id: 8,
                side: crate::topology::Side::One
            },
        ]
    );
    let incidence = scan
        .topology
        .half_edge_vertex_incidence
        .iter()
        .find(|incidence| {
            incidence.half_edge
                == crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::Zero,
                }
        })
        .expect("half-edge incidence");
    assert_eq!(incidence.start_vertex_id.get(), 1);
    assert_eq!(
        incidence.end_vertex_id.map(std::num::NonZeroU32::get),
        Some(2)
    );
}

fn closed_plane_intersection_data(geomlists: Option<&[u8]>) -> Vec<u8> {
    let mut payload = b"srf_array\0\xf8\x04".to_vec();
    push_generated_plane_row(
        &mut payload,
        1,
        true,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        2,
        false,
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        3,
        false,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        4,
        false,
        [-2.0, -1.0, 2.0],
        [2.0, -2.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\x06topol_ref_data\0");
    for (curve, faces, next) in [
        (10, [1, 2], [12, 13]),
        (11, [1, 3], [10, 15]),
        (12, [1, 4], [11, 14]),
        (13, [2, 3], [14, 11]),
        (14, [2, 4], [10, 15]),
        (15, [3, 4], [13, 12]),
    ] {
        push_generated_topology_row(&mut payload, curve, faces, next);
    }

    let allfeatur = b"\x04\xeb\x04\x00\x10\x01\x00\xe5\xe3\xf6\x83\x91\xe1\
        \xe0\x21geoms_affected\0\xf8\x01\x63\
        \xe0\x21edgs_affected\0\xf8\x02\x0a\x0b"
        .to_vec();
    let mut sections = vec![
        ("VisibGeom", payload),
        ("AllFeatur", allfeatur),
        ("MdlStatus", b"Round id 4\0".to_vec()),
    ];
    if let Some(geomlists) = geomlists {
        sections.push(("Geomlists", geomlists.to_vec()));
    }
    build_prt("c", &sections)
}

#[test]
fn decode_transfers_closed_plane_intersection_brep() {
    let data = closed_plane_intersection_data(None);
    let scan = container::scan_bytes_ok(data.clone());
    assert_eq!(scan.planes.local_systems.len(), 4);
    assert_eq!(scan.curves.topology_rows.len(), 6);
    assert!(
        scan.features.affected_ids.iter().any(|record| {
            record.feature_id == 4
                && record.kind == crate::feature::rows::AffectedIdKind::Edges
                && record.ids == [10, 11]
        }),
        "affected ids: {:#?}",
        scan.features.affected_ids
    );
    assert_eq!(scan.topology.loops.len(), 4);
    assert_eq!(scan.topology.vertices.len(), 4);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let model = &result.ir().model;
    let namespace = result.ir().native.namespace("creo").unwrap();
    assert_eq!(namespace.arenas()["half_edges"].len(), 12);
    assert_eq!(namespace.arenas()["loops"].len(), 4);
    assert_eq!(namespace.arenas()["topological_vertices"].len(), 4);
    assert_eq!(namespace.arenas()["half_edge_vertex_incidence"].len(), 12);
    assert_eq!(namespace.arenas()["face_components"].len(), 1);
    assert_eq!(namespace.arenas()["half_edges"][0].fields()["curve_id"], 10);
    assert_eq!(namespace.arenas()["half_edges"][0].fields()["side"], 0);

    assert_eq!(model.points.len(), 4);
    assert_eq!(model.vertices.len(), 4);
    assert_eq!(model.edges.len(), 6);
    assert_eq!(model.curves.len(), 6);
    assert!(model.edges.iter().all(|edge| edge.curve().is_some()));
    assert!(model.edges.iter().all(|edge| edge.param_range().is_some()));
    for edge in &model.edges {
        let [start_parameter, end_parameter] = edge.param_range().expect("line edge range").get();
        assert_eq!(start_parameter, 0.0);
        assert!(end_parameter > 0.0);
        let curve = model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())
            .expect("edge curve");
        let Some(SolvedCurveGeometry::Line(line_curve)) = curve.geometry.solved() else {
            panic!("edge line: {curve:#?}");
        };
        let origin = line_curve.origin().get();
        let direction = *line_curve.direction().as_raw();
        let start = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.start)
            .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            .expect("edge start point")
            .position()
            .get();
        let end = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.end)
            .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            .expect("edge end point")
            .position()
            .get();
        assert_eq!(origin, start);
        let evaluated = [
            origin.x + direction.x * end_parameter,
            origin.y + direction.y * end_parameter,
            origin.z + direction.z * end_parameter,
        ];
        assert!(evaluated
            .into_iter()
            .zip([end.x, end.y, end.z])
            .all(|(evaluated, expected)| (evaluated - expected).abs() < 1.0e-10));
    }
    assert_eq!(model.faces.len(), 4);
    assert_eq!(
        model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "creo:visibgeom:face#1")
            .expect("reversed face")
            .sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "creo:visibgeom:face#2")
            .expect("forward face")
            .sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert_eq!(model.loops.len(), 4);
    assert!(model.loops.iter().all(|lp| {
        model
            .faces
            .iter()
            .find(|face| face.id == lp.face)
            .map(|face| face.loop_role(&lp.id))
            .unwrap_or_default()
            == cadmpeg_ir::topology::LoopBoundaryRole::Outer
    }));
    assert_eq!(model.coedges.len(), 12);
    assert_eq!(model.pcurves.len(), 12);
    assert!(model.coedges.iter().all(|coedge| coedge.pcurves.len() == 1));
    for coedge in &model.coedges {
        let pcurve = model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
            .expect("projected plane pcurve");
        assert!(matches!(
            pcurve.geometry,
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(_)
        ));
        let edge = model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .expect("pcurve edge");
        assert_eq!(pcurve.parameter_range(), edge.param_range());
    }
    assert_eq!(model.shells.len(), 1);
    assert_eq!(model.regions.len(), 1);
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.bodies[0].kind, cadmpeg_ir::topology::BodyKind::Solid);
    let feature = model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature 4");
    assert_eq!(
        *feature.evaluation.outputs(),
        vec![model.bodies[0].id.clone()]
    );
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Fillet { groups },
    ) = feature.evaluation.definition()
    else {
        panic!("round definition: {:#?}", feature.evaluation.definition());
    };
    let [cadmpeg_ir::features::edge_treatments::FilletGroup { edges, .. }] = groups.as_slice()
    else {
        panic!("round groups: {groups:#?}");
    };
    let cadmpeg_ir::features::EdgeSelection::Resolved { edges, native } = edges else {
        panic!("round edges: {edges:#?}");
    };
    assert_eq!(
        edges,
        &[
            cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#10".to_string())
                .expect("identity grammar"),
            cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#11".to_string())
                .expect("identity grammar"),
        ]
    );
    assert_eq!(native, "creo:allfeatur:edgs_affected#4:10,11");
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_withholds_native_brep_when_declared_body_count_disagrees() {
    let data = closed_plane_intersection_data(Some(b"n_bodies\0\x02"));
    let scan = container::scan_bytes_ok(data.clone());
    assert_eq!(scan.framing.declared_body_count, Some(2));
    assert_eq!(scan.topology.face_components.len(), 1);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let model = &result.ir().model;
    assert_eq!(model.points.len(), 4);
    assert!(model.vertices.is_empty());
    assert!(model.edges.is_empty());
    assert!(model.faces.is_empty());
    assert!(model.loops.is_empty());
    assert!(model.coedges.is_empty());
    assert!(model.shells.is_empty());
    assert!(model.regions.is_empty());
    assert!(model.bodies.is_empty());
}

#[test]
fn closed_ring_constructor_rejects_empty_repeated_disconnected_and_mixed_faces() {
    let a = HalfEdgeId {
        curve_id: 1,
        side: crate::topology::Side::Zero,
    };
    let b = HalfEdgeId {
        curve_id: 2,
        side: crate::topology::Side::Zero,
    };
    let face = std::num::NonZeroU32::new(10);
    let graph = [
        HalfEdge {
            id: a,
            face_id: face,
            next: Some(b),
        },
        HalfEdge {
            id: b,
            face_id: face,
            next: Some(a),
        },
    ];
    {
        for ring in [vec![], vec![a, a], vec![a], vec![b]] {
            assert!(super::Loop::new(face, ring, &graph).is_none());
        }
        let ring = super::Loop::new(face, vec![a, b], &graph).expect("ring validation");
        assert_eq!(ring.half_edges(), [a, b]);
        let mut mixed = graph.clone();
        mixed[1].face_id = std::num::NonZeroU32::new(11);
        assert!(super::Loop::new(face, vec![a, b], &mixed).is_none());
        let mut duplicate = graph.to_vec();
        duplicate.insert(0, graph[0].clone());
        assert!(super::Loop::new(face, vec![a, b], &duplicate).is_none());
    };
}

#[test]
fn face_component_constructor_rejects_zero_repeated_and_unordered_identity_sets() {
    {
        for faces in [vec![], vec![0], vec![2, 0, 2, 1], vec![2, 1], vec![1, 1]] {
            assert!(super::FaceComponent::new(faces, vec![1]).is_none());
        }
        for curves in [vec![2, 1], vec![1, 1]] {
            assert!(super::FaceComponent::new(vec![1], curves).is_none());
        }
        let component =
            super::FaceComponent::new(vec![1, 2], vec![0, 1]).expect("component validation");
        assert_eq!(component.face_ids(), [1, 2]);
        assert_eq!(component.curve_ids(), [0, 1]);
    };
}

#[test]
fn vertex_orbit_constructor_rejects_zero_empty_repeated_and_unordered_members() {
    let a = HalfEdgeId {
        curve_id: 1,
        side: crate::topology::Side::Zero,
    };
    let b = HalfEdgeId {
        curve_id: 2,
        side: crate::topology::Side::Zero,
    };
    {
        assert!(super::TopologicalVertex::new(0, vec![a]).is_none());
        for members in [vec![], vec![a, a], vec![b, a]] {
            assert!(super::TopologicalVertex::new(1, members).is_none());
        }
        let vertex = super::TopologicalVertex::new(1, vec![a, b]).expect("vertex validation");
        assert_eq!(vertex.id.get(), 1);
        assert_eq!(vertex.half_edges(), [a, b]);
    };
}

#[test]
fn topology_build_keeps_an_open_successor_chain_out_of_loops() {
    let (edges, loops) = build(&[row(1, 2), row(2, 3), row(3, 99)]);
    assert_eq!(edges.len(), 6);
    assert!(loops.is_empty());
}

#[test]
fn long_successor_ring_validates_as_one_loop_per_face() {
    const COUNT: u32 = 200_000;
    let rows: Vec<_> = (1..=COUNT)
        .map(|id| row(id, if id == COUNT { 1 } else { id + 1 }))
        .collect();
    let (edges, loops) = build(&rows);
    assert_eq!(loops.len(), 2);
    let ring_len = usize::try_from(COUNT).unwrap();
    assert!(loops.iter().all(|ring| ring.half_edges().len() == ring_len));
    let ring = loops[0].half_edges().to_vec();
    assert!(super::Loop::new(loops[0].face_id(), ring, &edges).is_some());
}

#[test]
fn many_tails_into_one_cycle_form_one_loop_per_face() {
    const TAILS: u32 = 50_000;
    const CYCLE: u32 = 50_000;
    let first = TAILS + 1;
    let last = TAILS + CYCLE;
    let mut rows: Vec<_> = (1..=TAILS).map(|id| row(id, first)).collect();
    rows.extend((first..=last).map(|id| row(id, if id == last { first } else { id + 1 })));
    let (_, loops) = build(&rows);
    assert_eq!(loops.len(), 2);
    let ring_len = usize::try_from(CYCLE).unwrap();
    assert!(loops.iter().all(|ring| ring.half_edges().len() == ring_len));
    assert!(loops
        .iter()
        .all(|ring| ring.half_edges()[0].curve_id == first));
}

/// The linear successor scan and unmemoised walk that `build` replaced.
fn linear_scan_build(rows: &[CurveTopologyRow]) -> (Vec<HalfEdge>, Vec<super::Loop>) {
    use super::Side;
    use std::collections::BTreeMap;

    let rows = crate::identity::uniquely_identified_rows(rows, |row| row.id);
    let mut face_sides: BTreeMap<Option<std::num::NonZeroU32>, Vec<HalfEdgeId>> = BTreeMap::new();
    for row in &rows {
        for side in [Side::Zero, Side::One] {
            face_sides
                .entry(row.faces[side.index()])
                .or_default()
                .push(HalfEdgeId {
                    curve_id: row.id,
                    side,
                });
        }
    }
    let mut edges = Vec::new();
    for row in rows {
        for side in [Side::Zero, Side::One] {
            let face_id = row.faces[side.index()];
            let mut candidates = face_sides
                .get(&face_id)
                .into_iter()
                .flatten()
                .filter(|id| id.curve_id == row.next_edges[side.index()])
                .copied();
            let next = match (candidates.next(), candidates.next()) {
                (Some(candidate), None) => Some(candidate),
                _ => None,
            };
            edges.push(HalfEdge {
                id: HalfEdgeId {
                    curve_id: row.id,
                    side,
                },
                face_id,
                next,
            });
        }
    }
    edges.sort_by_key(|edge| edge.id);
    let by_id = |id: HalfEdgeId| edges.iter().find(|edge| edge.id == id);
    let mut consumed = BTreeSet::new();
    let mut open = BTreeSet::new();
    let mut loops = Vec::new();
    for edge in &edges {
        if consumed.contains(&edge.id) || open.contains(&edge.id) {
            continue;
        }
        let mut ring = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = edge.id;
        let mut open_ended = false;
        loop {
            if open.contains(&current) {
                open_ended = true;
                break;
            }
            if seen.contains(&current) {
                if current == edge.id {
                    consumed.extend(ring.iter().copied());
                    loops.push(super::Loop {
                        face_id: edge.face_id,
                        half_edges: std::mem::take(&mut ring),
                    });
                }
                break;
            }
            seen.insert(current);
            ring.push(current);
            let Some(next) = by_id(current).and_then(|entry| entry.next) else {
                open_ended = true;
                break;
            };
            if by_id(next).is_none_or(|entry| entry.face_id != edge.face_id) {
                open_ended = true;
                break;
            }
            current = next;
        }
        if open_ended {
            open.extend(ring);
        }
    }
    (edges, loops)
}

#[test]
fn build_matches_the_linear_scan_walk_on_random_successor_graphs() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut draw = |bound: u32| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        u32::try_from(state % u64::from(bound)).unwrap()
    };
    for _ in 0..4_000 {
        let count = 1 + draw(40);
        let rows: Vec<_> = (0..count)
            .map(|_| {
                let id = 1 + draw(48);
                CurveTopologyRow {
                    faces: [
                        std::num::NonZeroU32::new(draw(3)),
                        std::num::NonZeroU32::new(draw(3)),
                    ],
                    next_edges: [draw(50), draw(50)],
                    ..row(id, 0)
                }
            })
            .collect();
        let expected = linear_scan_build(&rows);
        assert_eq!(build(&rows), expected, "{rows:?}");
    }
}

mod work_admission;
