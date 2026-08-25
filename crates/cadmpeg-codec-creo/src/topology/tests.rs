// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId};
use cadmpeg_ir::Exactness;

use crate::container::{self, role, Layout};
use crate::surface::TorusRadius2Encoding;
use crate::test_support::*;
use crate::CreoCodec;

use super::*;

fn row(id: u32, next: u32) -> CurveTopologyRow {
    CurveTopologyRow {
        id,
        type_byte: 0,
        feature_id: 0,
        directions: [1, 1],
        faces: [10, 20],
        next_edges: [next, next],
        offset: 0,
    }
}
#[test]
fn builds_closed_face_side_rings_without_guessing() {
    let (half_edges, loops) = build(&[row(1, 2), row(2, 3), row(3, 1)]);
    assert_eq!(half_edges.len(), 6);
    assert_eq!(loops.len(), 2);
    assert_eq!(loops[0].face_id, 10);
    assert_eq!(
        loops[0].half_edges,
        vec![
            HalfEdgeId {
                curve_id: 1,
                side: 0
            },
            HalfEdgeId {
                curve_id: 2,
                side: 0
            },
            HalfEdgeId {
                curve_id: 3,
                side: 0
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

    let components = face_components(&rows);
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].face_ids, [10, 20]);
    assert_eq!(components[0].curve_ids, [1]);
}
#[test]
fn withholds_ambiguous_successors() {
    let (half_edges, loops) = build(&[
        row(1, 2),
        CurveTopologyRow {
            faces: [10, 10],
            ..row(2, 1)
        },
    ]);
    assert!(half_edges.iter().any(|edge| edge.id
        == HalfEdgeId {
            curve_id: 1,
            side: 0
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
                side: 0,
            },
            face_id: 10,
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 1,
                side: 1,
            },
            face_id: 20,
            next: Some(HalfEdgeId {
                curve_id: 2,
                side: 0,
            }),
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 2,
                side: 0,
            },
            face_id: 20,
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 2,
                side: 1,
            },
            face_id: 10,
            next: None,
        },
    ];

    let (vertices, _) = vertex_orbits(&edges);
    assert!(vertices.iter().any(|vertex| vertex.half_edges
        == vec![
            HalfEdgeId {
                curve_id: 1,
                side: 0,
            },
            HalfEdgeId {
                curve_id: 2,
                side: 0,
            },
        ]));
}

#[test]
fn vertex_incident_faces_include_both_sides_of_each_orbit_edge() {
    let edges = vec![
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 7,
                side: 0,
            },
            face_id: 10,
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 7,
                side: 1,
            },
            face_id: 20,
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 8,
                side: 0,
            },
            face_id: 10,
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 8,
                side: 1,
            },
            face_id: 30,
            next: None,
        },
    ];
    let vertex = TopologicalVertex {
        id: 1,
        half_edges: vec![
            HalfEdgeId {
                curve_id: 7,
                side: 0,
            },
            HalfEdgeId {
                curve_id: 8,
                side: 0,
            },
        ],
    };

    assert_eq!(
        vertex_incident_faces(&[vertex], &edges).get(&1).cloned(),
        Some(BTreeSet::from([10, 20, 30]))
    );
}

#[test]
fn edge_vertex_pair_accepts_one_closed_face_and_rejects_disagreement() {
    let incidence = |reverse_end| {
        vec![
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 7,
                    side: 0,
                },
                start_vertex_id: 10,
                end_vertex_id: Some(20),
            },
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 7,
                    side: 1,
                },
                start_vertex_id: 20,
                end_vertex_id: reverse_end,
            },
        ]
    };

    assert_eq!(edge_vertex_pairs(&incidence(None)).get(&7), Some(&[10, 20]));
    assert_eq!(
        edge_vertex_pairs(&incidence(Some(10))).get(&7),
        Some(&[10, 20])
    );
    assert!(!edge_vertex_pairs(&incidence(Some(30))).contains_key(&7));
}

#[test]
fn edge_start_vertex_pair_survives_an_unresolved_successor() {
    let incidence = vec![
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: 0,
            },
            start_vertex_id: 10,
            end_vertex_id: None,
        },
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: 1,
            },
            start_vertex_id: 20,
            end_vertex_id: None,
        },
    ];

    assert_eq!(edge_start_vertex_pairs(&incidence).get(&7), Some(&[10, 20]));
    assert!(!edge_vertex_pairs(&incidence).contains_key(&7));
}

#[test]
fn scan_groups_connected_nonzero_face_references() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3\x08\x08\x04\x01\xf6\x0b\x0c\x08\x08\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topology.face_components.len(), 1);
    assert_eq!(scan.topology.face_components[0].face_ids, vec![10, 11, 12]);
    assert_eq!(scan.topology.face_components[0].curve_ids, vec![7, 8]);
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
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topology.vertices.len(), 2);
    assert_eq!(
        scan.topology.vertices[0].half_edges,
        vec![
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 0
            },
            crate::topology::HalfEdgeId {
                curve_id: 8,
                side: 1
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
                    side: 0,
                }
        })
        .expect("half-edge incidence");
    assert_eq!(incidence.start_vertex_id, 1);
    assert_eq!(incidence.end_vertex_id, Some(2));
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
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.planes.local_systems.len(), 4);
    assert_eq!(scan.curves.topology_rows.len(), 6);
    assert!(
        scan.features.affected_ids.iter().any(|record| {
            record.feature_id == 4
                && record.kind == crate::feature::AffectedIdKind::Edges
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
    assert_eq!(namespace.arenas["half_edges"].len(), 12);
    assert_eq!(namespace.arenas["loops"].len(), 4);
    assert_eq!(namespace.arenas["topological_vertices"].len(), 4);
    assert_eq!(namespace.arenas["half_edge_vertex_incidence"].len(), 12);
    assert_eq!(namespace.arenas["face_components"].len(), 1);
    assert_eq!(namespace.arenas["half_edges"][0].fields()["curve_id"], 10);
    assert_eq!(namespace.arenas["half_edges"][0].fields()["side"], 0);

    assert_eq!(model.points.len(), 4);
    assert_eq!(model.vertices.len(), 4);
    assert_eq!(model.edges.len(), 6);
    assert_eq!(model.curves.len(), 6);
    assert!(model.edges.iter().all(|edge| edge.curve.is_some()));
    assert!(model.edges.iter().all(|edge| edge.param_range.is_some()));
    for edge in &model.edges {
        let [start_parameter, end_parameter] = edge.param_range.expect("line edge range");
        assert_eq!(start_parameter, 0.0);
        assert!(end_parameter > 0.0);
        let curve = model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve.as_ref())
            .expect("edge curve");
        let cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } = curve.geometry else {
            panic!("edge line: {curve:#?}");
        };
        let start = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.start)
            .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            .expect("edge start point")
            .position;
        let end = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.end)
            .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            .expect("edge end point")
            .position;
        assert_eq!(origin, start);
        let evaluated = [
            origin.x + direction.x * end_parameter,
            origin.y + direction.y * end_parameter,
            origin.z + direction.z * end_parameter,
        ];
        assert!(evaluated
            .into_iter()
            .zip([end.x, end.y, end.z])
            .all(|(evaluated, expected)| (evaluated - expected).abs() < 1e-10));
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
    assert!(model
        .loops
        .iter()
        .all(|lp| lp.boundary_role == cadmpeg_ir::topology::LoopBoundaryRole::Outer));
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
            cadmpeg_ir::geometry::PcurveGeometry::Line { .. }
        ));
        let edge = model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .expect("pcurve edge");
        assert_eq!(pcurve.parameter_range, edge.param_range);
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
    assert_eq!(feature.outputs, vec![model.bodies[0].id.clone()]);
    let cadmpeg_ir::features::FeatureDefinition::Fillet { groups } = &feature.definition else {
        panic!("round definition: {:#?}", feature.definition);
    };
    let [cadmpeg_ir::features::FilletGroup { edges, .. }] = groups.as_slice() else {
        panic!("round groups: {groups:#?}");
    };
    let cadmpeg_ir::features::EdgeSelection::Resolved { edges, native } = edges else {
        panic!("round edges: {edges:#?}");
    };
    assert_eq!(
        edges,
        &[
            cadmpeg_ir::ids::EdgeId("creo:visibgeom:edge#10".to_string()),
            cadmpeg_ir::ids::EdgeId("creo:visibgeom:edge#11".to_string()),
        ]
    );
    assert_eq!(native, "creo:allfeatur:edgs_affected#4:10,11");
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_withholds_native_brep_when_declared_body_count_disagrees() {
    let data = closed_plane_intersection_data(Some(b"n_bodies\0\x02"));
    let scan = container::scan_bytes(data.clone());
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
