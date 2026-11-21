// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use cadmpeg_ir::features::edge_treatments::{ChamferGroup, ChamferSpec};
use cadmpeg_ir::features::{
    DistinctMembers, EdgeSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation, HistoricalEdgeGeometry, NonEmptyMembers,
};
use cadmpeg_ir::ids::{HistoricalEdgeId, IdentityKey};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{InteriorAngle, PositiveLength};

use super::{attach, clear, supports_for, FaceSense, SENSES};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
    AsmHistoricalEdgeSample, AsmHistoricalPlane, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology,
};

fn feature_id() -> FeatureId {
    "f3d:model:feature#chamfer".to_owned().try_into().unwrap()
}

fn test_prefix() -> IdentityKey {
    crate::ids::feature_input_prefix(&feature_id(), 4)
}

fn record_sense(history: &str, edge: i64, face: i64, with_edge: bool, face_forward: bool) {
    SENSES.with(|senses| {
        senses.borrow_mut().insert(
            (Arc::from(history), edge, face),
            FaceSense {
                with_edge,
                face_forward,
            },
        );
    });
}

fn plane_plane_topology() -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: vec![10, 20],
        loops: vec![100, 200],
        edges: vec![1],
        surfaces: vec![1000, 2000],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![100],
            },
            AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![200],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 101,
                owner_loop: 100,
                edge: 1,
                next: 101,
                previous: 101,
                radial_next: 201,
            },
            AsmHistoricalCoedge {
                coedge: 201,
                owner_loop: 200,
                edge: 1,
                next: 201,
                previous: 201,
                radial_next: 101,
            },
        ],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 1000,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 2000,
            },
        ],
        surface_planes: vec![
            AsmHistoricalPlane {
                surface: 1000,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 2000,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
            },
        ],
        edge_samples: vec![AsmHistoricalEdgeSample {
            edge: 1,
            interior: Point3::new(0.0, 5.0, 0.0),
            tangent: Some(Vector3::new(0.0, 1.0, 0.0)),
            bounds: None,
            carrier: None,
        }],
        ..AsmHistoricalTopology::default()
    }
}

fn plane_cylinder_topology(with_other_cylinder_vertex: bool) -> AsmHistoricalTopology {
    // Top plane z = 10 meets vertical cylinder x^2 + y^2 = 5^2 along circle z = 10.
    // Edge interior at (5, 0, 10) with tangent (0, 1, 0) (CCW seen from +Z).
    // Cylinder extends down to z = 0, so another vertex on face 20 sits at (5, 0, 0).
    let mut topology = AsmHistoricalTopology {
        faces: vec![10, 20],
        loops: vec![100, 200],
        edges: vec![1, 2],
        vertices: vec![11, 12, 13],
        points: vec![111, 112, 113],
        surfaces: vec![1000, 2000],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![100],
            },
            AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![200],
            },
        ],
        loop_coedges: vec![
            AsmHistoricalRelation {
                owner_ref: 100,
                member_refs: vec![101],
            },
            AsmHistoricalRelation {
                owner_ref: 200,
                member_refs: vec![201],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 101,
                owner_loop: 100,
                edge: 1,
                next: 101,
                previous: 101,
                radial_next: 201,
            },
            AsmHistoricalCoedge {
                coedge: 201,
                owner_loop: 200,
                edge: 1,
                next: 202,
                previous: 202,
                radial_next: 101,
            },
        ],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 1000,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 2000,
            },
        ],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 1000,
            origin: Point3::new(0.0, 0.0, 10.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 2000,
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 5.0,
        }],
        edge_samples: vec![AsmHistoricalEdgeSample {
            edge: 1,
            interior: Point3::new(5.0, 0.0, 10.0),
            tangent: Some(Vector3::new(0.0, 1.0, 0.0)),
            bounds: None,
            carrier: None,
        }],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 1,
            start_vertex: 11,
            end_vertex: 12,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 111,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 112,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 111,
                position: Point3::new(5.0, 0.0, 10.0),
            },
            AsmHistoricalPoint {
                point: 112,
                position: Point3::new(-5.0, 0.0, 10.0),
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    if with_other_cylinder_vertex {
        topology.loop_coedges[1].member_refs.push(202);
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge: 202,
            owner_loop: 200,
            edge: 2,
            next: 201,
            previous: 201,
            radial_next: 202,
        });
        topology.edge_vertices.push(AsmHistoricalEdge {
            edge: 2,
            start_vertex: 11,
            end_vertex: 13,
        });
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: 13,
            carrier: 113,
        });
        topology.point_positions.push(AsmHistoricalPoint {
            point: 113,
            position: Point3::new(5.0, 0.0, 0.0),
        });
    }
    topology
}

fn chamfer_feature(spec: ChamferSpec, prefix: &IdentityKey, edge_id: HistoricalEdgeId) -> Feature {
    Feature {
        id: feature_id(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Chamfer".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Chamfer {
                groups: NonEmptyMembers::one(ChamferGroup {
                    edges: EdgeSelection::historical(
                        crate::ids::history_input_state_id(prefix),
                        vec![edge_id],
                        "f3d:test:chamfer-group".to_owned(),
                    )
                    .unwrap(),
                    spec,
                }),
                flip_direction: false,
            },
        )),
        native_ref: None,
        product_definition: None,
    }
}

#[test]
fn plane_plane_assigns_with_edge_and_flips_reversed_face_sense() {
    clear();
    let history = "f3d:test:history#plane-plane";
    let topology = plane_plane_topology();
    let prefix = test_prefix();

    // Case 1: face 10 runs with edge and is forward; face 20 runs against edge and is reversed.
    record_sense(history, 1, 10, true, true);
    record_sense(history, 1, 20, false, false);
    let supports = supports_for(history, &topology, 1, &prefix).expect("plane+plane supports");
    assert_eq!(supports.len(), 2);
    assert!(supports[0].with_edge);
    assert!((supports[0].normal.as_raw().z - 1.0).abs() < 1e-12);
    assert!(!supports[1].with_edge);
    assert!((supports[1].normal.as_raw().x + 1.0).abs() < 1e-12);

    // Case 2: swapped coedge senses and flipped face 10 sense.
    clear();
    record_sense(history, 1, 10, false, false);
    record_sense(history, 1, 20, true, true);
    let supports =
        supports_for(history, &topology, 1, &prefix).expect("plane+plane swapped supports");
    assert!(!supports[0].with_edge);
    assert!((supports[0].normal.as_raw().z + 1.0).abs() < 1e-12);
    assert!(supports[1].with_edge);
    assert!((supports[1].normal.as_raw().x - 1.0).abs() < 1e-12);
}

#[test]
fn plane_cylinder_resolves_outward_radial_in_both_coedge_senses() {
    let history = "f3d:test:history#plane-cyl";
    let topology = plane_cylinder_topology(true);
    let prefix = test_prefix();

    // Case 1: cylinder face 20 runs against the edge (along = -Y, into = -Z => along x into = +X).
    clear();
    record_sense(history, 1, 10, true, true);
    record_sense(history, 1, 20, false, true);
    let supports = supports_for(history, &topology, 1, &prefix).expect("plane+cylinder case 1");
    assert_eq!(supports.len(), 2);
    assert!(supports[0].with_edge);
    assert!(!supports[1].with_edge);
    assert!((supports[1].normal.as_raw().x - 1.0).abs() < 1e-12);

    // Case 2: cylinder face 20 runs with the edge (along = +Y, into = -Z => along x into = -X).
    clear();
    record_sense(history, 1, 10, false, true);
    record_sense(history, 1, 20, true, true);
    let supports = supports_for(history, &topology, 1, &prefix).expect("plane+cylinder case 2");
    assert!(!supports[0].with_edge);
    assert!(supports[1].with_edge);
    assert!((supports[1].normal.as_raw().x + 1.0).abs() < 1e-12);
}

#[test]
fn cylinder_without_inward_decision_is_rejected_even_when_radial_is_flat() {
    clear();
    let history = "f3d:test:history#flat-radial";
    // Radial direction at (5, 0, 10) is (1, 0, 0) (radial.z == 0), and no other vertex
    // exists on face 20 so face_inward returns None.
    let topology = plane_cylinder_topology(false);
    let prefix = test_prefix();
    record_sense(history, 1, 10, true, true);
    record_sense(history, 1, 20, false, true);
    assert!(supports_for(history, &topology, 1, &prefix).is_none());
}

#[test]
fn invalid_surface_pairs_and_with_edge_counts_are_rejected() {
    let history = "f3d:test:history#gates";
    let prefix = test_prefix();

    // Both faces marked with_edge = true -> rejected.
    clear();
    let topology = plane_plane_topology();
    record_sense(history, 1, 10, true, true);
    record_sense(history, 1, 20, true, true);
    assert!(supports_for(history, &topology, 1, &prefix).is_none());

    // Neither face marked with_edge = true -> rejected.
    clear();
    record_sense(history, 1, 10, false, true);
    record_sense(history, 1, 20, false, true);
    assert!(supports_for(history, &topology, 1, &prefix).is_none());

    // Two cylinders -> rejected.
    clear();
    let mut two_cylinders = plane_cylinder_topology(true);
    two_cylinders.surface_planes.clear();
    two_cylinders.surface_cylinders.push(AsmHistoricalCylinder {
        surface: 1000,
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        radius: 5.0,
    });
    record_sense(history, 1, 10, true, true);
    record_sense(history, 1, 20, false, true);
    assert!(supports_for(history, &two_cylinders, 1, &prefix).is_none());
}

#[test]
fn attach_filters_out_equal_distance_and_non_two_distances_chamfers() {
    clear();
    let history = "f3d:test:history#attach";
    let topology = plane_plane_topology();
    let prefix = test_prefix();
    let edge_id = crate::ids::history_input_edge_id(&prefix, 1);
    record_sense(history, 1, 10, true, true);
    record_sense(history, 1, 20, false, true);

    let run = |spec: ChamferSpec| {
        let feature = chamfer_feature(spec, &prefix, edge_id.clone());
        let mut geometry = vec![HistoricalEdgeGeometry {
            edge: edge_id.clone(),
            interior: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 5.0, 0.0)).unwrap(),
            tangent: None,
            start: None,
            end: None,
            faces: DistinctMembers::try_from(vec![]).unwrap(),
            carrier: None,
            supports: Vec::new(),
        }];
        attach(&feature, history, &topology, &prefix, &mut geometry);
        geometry[0].supports.len()
    };

    assert_eq!(
        run(ChamferSpec::TwoDistances {
            first: PositiveLength::new(1.0).unwrap(),
            second: PositiveLength::new(2.0).unwrap(),
        }),
        2
    );
    assert_eq!(
        run(ChamferSpec::TwoDistances {
            first: PositiveLength::new(2.0).unwrap(),
            second: PositiveLength::new(2.0).unwrap(),
        }),
        0
    );
    assert_eq!(
        run(ChamferSpec::Distance {
            distance: PositiveLength::new(2.0).unwrap(),
        }),
        0
    );
    assert_eq!(
        run(ChamferSpec::DistanceAngle {
            distance: PositiveLength::new(2.0).unwrap(),
            angle: InteriorAngle::new(0.5).unwrap(),
        }),
        0
    );
}
