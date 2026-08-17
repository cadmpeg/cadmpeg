// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;

use super::{
    component_is_closed, split_neutral_component_shells, transfer_native_brep, NeutralShellSpec,
};

#[test]
fn partitions_face_shells_and_retains_unattached_wire_curves() {
    let faces = [1, 2, 3];
    let face_adjacency = BTreeMap::from([
        (1, BTreeSet::from([2])),
        (2, BTreeSet::from([1])),
        (3, BTreeSet::new()),
    ]);
    let face_vertices = BTreeMap::from([
        (1, BTreeSet::from([10, 11])),
        (2, BTreeSet::from([11, 12])),
        (3, BTreeSet::from([30, 31])),
    ]);
    let edge_vertices = BTreeMap::from([(100, [11, 12]), (101, [40, 41])]);

    let shells = split_neutral_component_shells(
        &faces,
        &BTreeSet::from([100, 101]),
        &face_adjacency,
        &face_vertices,
        &edge_vertices,
    );

    assert_eq!(
        shells,
        vec![
            NeutralShellSpec {
                faces: vec![1, 2],
                wire_curves: BTreeSet::from([100]),
            },
            NeutralShellSpec {
                faces: vec![3],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: Vec::new(),
                wire_curves: BTreeSet::from([101]),
            },
        ]
    );
}

#[test]
fn retains_wire_curve_when_shell_attachment_is_ambiguous() {
    let faces = [1, 2];
    let face_adjacency = BTreeMap::from([(1, BTreeSet::new()), (2, BTreeSet::new())]);
    let face_vertices =
        BTreeMap::from([(1, BTreeSet::from([10, 11])), (2, BTreeSet::from([20, 21]))]);
    let edge_vertices = BTreeMap::from([(100, [10, 20])]);

    assert_eq!(
        split_neutral_component_shells(
            &faces,
            &BTreeSet::from([100]),
            &face_adjacency,
            &face_vertices,
            &edge_vertices,
        ),
        vec![
            NeutralShellSpec {
                faces: vec![1],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: vec![2],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: Vec::new(),
                wire_curves: BTreeSet::from([100]),
            },
        ]
    );
}

#[test]
fn closed_component_counts_two_uses_of_one_face() {
    let edges = BTreeMap::from([
        (
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 0,
            },
            crate::topology::HalfEdge {
                id: crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: 0,
                },
                face_id: 5,
                next: None,
            },
        ),
        (
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 1,
            },
            crate::topology::HalfEdge {
                id: crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: 1,
                },
                face_id: 5,
                next: None,
            },
        ),
    ]);
    let half_edges = edges
        .iter()
        .map(|(id, edge)| (*id, edge))
        .collect::<BTreeMap<_, _>>();

    assert!(component_is_closed(
        &BTreeSet::from([7]),
        &BTreeSet::from([
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 0,
            },
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 1,
            },
        ]),
        &half_edges,
        &[5],
    ));
    assert!(!component_is_closed(
        &BTreeSet::from([7]),
        &BTreeSet::from([crate::topology::HalfEdgeId {
            curve_id: 7,
            side: 0,
        }]),
        &half_edges,
        &[5],
    ));
}

#[test]
fn native_brep_rejects_duplicate_model_curve_ids() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.framing.declared_body_count = Some(1);
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 5,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 0,
        });
    let points = [
        [[0.0, 0.0], [1.0, 0.0]],
        [[1.0, 0.0], [1.0, 1.0]],
        [[1.0, 1.0], [0.0, 0.0]],
    ];
    scan.curves.topology_rows = [10_u32, 11, 12]
        .into_iter()
        .map(|id| crate::curve::CurveTopologyRow {
            id,
            type_byte: 0,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [5, 0],
            next_edges: [id, 0],
            offset: 0,
        })
        .collect();
    scan.curves.pcurves = [10_u32, 11, 12]
        .into_iter()
        .zip(points)
        .map(|(curve_id, endpoints)| crate::curve::PcurveEndpoints {
            curve_id,
            faces: [5, 0],
            face_0_endpoints: endpoints,
            face_1_endpoints: [[0.0, 0.0], [0.0, 0.0]],
            offset: 0,
        })
        .collect();
    scan.topology.half_edges = [10_u32, 11, 12]
        .into_iter()
        .map(|curve_id| crate::topology::HalfEdge {
            id: crate::topology::HalfEdgeId { curve_id, side: 0 },
            face_id: 5,
            next: None,
        })
        .chain(
            [10_u32, 11, 12]
                .into_iter()
                .map(|curve_id| crate::topology::HalfEdge {
                    id: crate::topology::HalfEdgeId { curve_id, side: 1 },
                    face_id: 0,
                    next: None,
                }),
        )
        .collect();
    scan.topology.loops.push(crate::topology::Loop {
        face_id: 5,
        half_edges: [10_u32, 11, 12]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId { curve_id, side: 0 })
            .collect(),
    });
    scan.topology
        .face_components
        .push(crate::topology::FaceComponent {
            face_ids: vec![5],
            curve_ids: vec![10, 11, 12],
        });
    scan.topology.vertices = [1_u32, 2, 3]
        .into_iter()
        .zip([10_u32, 11, 12])
        .map(|(id, curve_id)| crate::topology::TopologicalVertex {
            id,
            half_edges: vec![crate::topology::HalfEdgeId { curve_id, side: 0 }],
        })
        .collect();
    let endpoint_pairs = [(10, 1, 2), (11, 2, 3), (12, 3, 1)];
    scan.topology.half_edge_vertex_incidence = endpoint_pairs
        .into_iter()
        .flat_map(|(curve_id, start, end)| {
            [
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId { curve_id, side: 0 },
                    start_vertex_id: start,
                    end_vertex_id: Some(end),
                },
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId { curve_id, side: 1 },
                    start_vertex_id: end,
                    end_vertex_id: Some(start),
                },
            ]
        })
        .collect();

    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#5".to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    for (id, origin, direction) in [
        (10, Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (11, Point3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        (
            12,
            Point3::new(1.0, 1.0, 0.0),
            Vector3::new(-1.0, -1.0, 0.0),
        ),
    ] {
        let curve = Curve {
            id: CurveId(format!("creo:visibgeom:curve#{id}")),
            geometry: CurveGeometry::Line { origin, direction },
            source_object: None,
        };
        ir.model.curves.extend([curve.clone(), curve]);
    }

    let counts = transfer_native_brep(
        &scan,
        &mut ir,
        &mut AnnotationBuilder::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    );

    assert_eq!(counts, (0, 0));
    assert!(ir.model.points.is_empty());
    assert!(ir.model.vertices.is_empty());
    assert!(ir.model.edges.is_empty());
    assert!(ir.model.faces.is_empty());
    assert!(ir.model.loops.is_empty());
    assert!(ir.model.coedges.is_empty());
    assert!(ir.model.bodies.is_empty());
    assert!(ir.model.regions.is_empty());
    assert!(ir.model.shells.is_empty());
}
