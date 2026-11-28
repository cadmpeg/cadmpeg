// SPDX-License-Identifier: Apache-2.0
//! Projected feature input topology.

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, UnresolvedFamily};

fn input_fixture() -> (
    Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
) {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:design:scope#input",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        7,
    );
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(4);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let feature = Feature {
        product_definition: None,
        id: "f3d:model:feature#input".to_owned().try_into().unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("WorkPoint".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPoint,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![(
            AsmDeltaState {
                id: "f3d:history:state#4".into(),
                parent: "f3d:history".into(),
                byte_offset: 0,
                state_id: 4,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: 0,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology {
                bodies: vec![1],
                faces: vec![2],
                edges: vec![3],
                vertices: vec![4],
                ..AsmHistoricalTopology::default()
            }),
            Vec::new(),
        )],
    );
    (feature, scope, history)
}

fn project() -> Vec<cadmpeg_ir::features::evaluation::FeatureInputTopology> {
    let (feature, scope, mut history) = input_fixture();
    super::super::input_topologies::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        std::slice::from_mut(&mut history),
        &[],
        &std::collections::HashMap::new(),
    )
}

#[test]
fn input_projection_preserves_member_order() {
    let projected = project();
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].vertices.as_slice()[0].as_str(),
        "f3d:history-input:vertex#5:input:4:4"
    );
}

#[test]
fn input_topology_uses_scope_binding_when_global_state_numbers_repeat() {
    let (feature, mut scope, mut history) = input_fixture();
    scope
        .try_edit(|draft| draft.history_state_id = Some(5))
        .unwrap();
    let mut current = history.states()[0].clone();
    current.id = "f3d:history:state#5".into();
    current.state_id = 5;
    current.node_index = 1;
    current.next_ref = Some(0);
    let topology = history.topology(&history.states()[0]).unwrap().clone();
    history = crate::test_support::history::history(
        history.id.clone(),
        history.byte_offset,
        history.preamble,
        false,
        vec![
            (
                history.states()[0].clone(),
                Some(topology.clone()),
                Vec::new(),
            ),
            (current, Some(topology), Vec::new()),
        ],
    );
    let mut other = history.clone();
    other.id = "f3d:other:history#0".into();
    other.states_mut()[0].id = "f3d:other:state#4".into();
    other.states_mut()[0].parent = other.id.clone();
    other.states_mut()[1].id = "f3d:other:state#5".into();
    other.states_mut()[1].parent = other.id.clone();
    crate::test_support::history::edit(&mut other, 0, |topology, _| {
        *topology = crate::history_records::AsmHistoricalTopology {
            faces: vec![99],
            ..Default::default()
        };
    });
    let mut histories = [history, other];
    let project = |histories: &mut [_], bindings: &std::collections::HashMap<String, String>| {
        super::super::input_topologies::project_feature_input_topologies(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&scope),
            histories,
            &[],
            bindings,
        )
    };
    assert!(project(&mut histories, &std::collections::HashMap::new()).is_empty());
    let bindings = std::collections::HashMap::from([(scope.id.clone(), histories[0].id.clone())]);
    let inputs = project(&mut histories, &bindings);
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        inputs[0].faces.as_slice(),
        &[crate::ids::history_input_face_id(
            &crate::ids::history_input_prefix(&feature.id.key(), 4),
            2
        )]
    );
    assert_eq!(
        inputs[0].native_ref.as_deref(),
        Some(histories[0].states()[0].id.as_str())
    );
    let bindings =
        std::collections::HashMap::from([(scope.id.clone(), "f3d:missing:history#0".into())]);
    assert!(project(&mut histories, &bindings).is_empty());
    assert!(
        super::super::input_topologies::project_feature_input_topologies(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&scope),
            &mut histories[..1],
            &[],
            &bindings,
        )
        .is_empty()
    );
}

#[test]
fn input_edge_geometry_carries_samples_endpoints_and_faces() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
        AsmHistoricalEdgeSample, AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::features::evaluation::{HistoricalEdgeCarrier, HistoricalEdgeCurve};
    use cadmpeg_ir::math::{Point3, Vector3};

    let carrier = HistoricalEdgeCarrier {
        curve: HistoricalEdgeCurve::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        range: cadmpeg_ir::topology::ParameterInterval::new([0.0, 10.0]).unwrap(),
    };
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11],
        loops: vec![20, 21],
        coedges: vec![30, 31],
        edges: vec![40, 41],
        vertices: vec![50, 51],
        points: vec![60, 61],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![20],
            },
            AsmHistoricalRelation {
                owner_ref: 11,
                member_refs: vec![21],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 30,
                owner_loop: 20,
                edge: 40,
                next: 30,
                previous: 30,
                radial_next: 31,
            },
            AsmHistoricalCoedge {
                coedge: 31,
                owner_loop: 21,
                edge: 40,
                next: 31,
                previous: 31,
                radial_next: 30,
            },
        ],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 40,
            start_vertex: 50,
            end_vertex: 51,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 50,
                carrier: 60,
            },
            AsmHistoricalCarrierBinding {
                entity: 51,
                carrier: 61,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 60,
                position: Point3::new(0.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 61,
                position: Point3::new(0.0, 0.0, 10.0),
            },
        ],
        edge_samples: vec![AsmHistoricalEdgeSample {
            bounds: None,
            edge: 40,
            interior: Point3::new(0.0, 0.0, 5.0),
            tangent: Some(Vector3::new(0.0, 0.0, 1.0)),
            carrier: Some(carrier.clone()),
        }],
        ..AsmHistoricalTopology::default()
    };
    let feature: cadmpeg_ir::features::FeatureId =
        "f3d:model:feature#fillet".to_owned().try_into().unwrap();
    let prefix = crate::ids::feature_input_prefix(&feature, 4);
    let geometry = super::super::input_topologies::project_input_edge_geometry(&topology, &prefix);
    let [edge] = geometry.as_slice() else {
        panic!("one sampled edge: {geometry:?}");
    };
    assert_eq!(edge.edge, crate::ids::history_input_edge_id(&prefix, 40));
    assert_eq!(
        edge.interior,
        cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(0.0, 0.0, 5.0)).unwrap()
    );
    assert_eq!(edge.tangent, Some(cadmpeg_ir::units::UnitVector3::Z_AXIS));
    assert_eq!(
        edge.start,
        cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
    );
    assert_eq!(
        edge.end,
        cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(0.0, 0.0, 10.0))
    );
    assert_eq!(
        edge.faces.iter().cloned().collect::<Vec<_>>(),
        vec![
            crate::ids::history_input_face_id(&prefix, 10),
            crate::ids::history_input_face_id(&prefix, 11),
        ]
    );
    assert_eq!(edge.carrier, Some(carrier));
}

#[test]
fn input_edge_geometry_keeps_only_selected_edges() {
    use cadmpeg_ir::features::evaluation::HistoricalEdgeGeometry;
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::math::Point3;

    let (mut feature, _, _) = input_fixture();
    let prefix = crate::ids::feature_input_prefix(&feature.id, 4);
    let selected = crate::ids::history_input_edge_id(&prefix, 40);
    let unselected = crate::ids::history_input_edge_id(&prefix, 41);
    feature.evaluation = cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::selections::NonEmptyMembers::try_from(vec![
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: cadmpeg_ir::features::selections::EdgeSelection::historical(
                        crate::ids::history_input_state_id(&prefix),
                        vec![selected.clone()],
                        "f3d:test:fillet-group".to_owned(),
                    )
                    .unwrap(),
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved {
                        form: None,
                    },
                    tangency_weight: None,
                },
            ])
            .unwrap(),
        }),
    );
    let geometry = |edge: &cadmpeg_ir::ids::HistoricalEdgeId| HistoricalEdgeGeometry {
        edge: edge.clone(),
        interior: FinitePoint3::new(Point3::new(0.0, 0.0, 5.0)).unwrap(),
        tangent: None,
        start: None,
        end: None,
        faces: Default::default(),
        carrier: None,
        supports: Vec::new(),
    };
    let mut topologies = vec![cadmpeg_ir::features::evaluation::FeatureInputTopology {
        face_owners: Vec::new(),
        face_geometry: Vec::new(),
        body_writers: Vec::new(),
        id: crate::ids::history_input_state_id(&prefix),
        input_of: feature.id.clone(),
        bodies: Default::default(),
        faces: Default::default(),
        edges: cadmpeg_ir::features::selections::DistinctMembers::try_from(vec![
            selected.clone(),
            unselected.clone(),
        ])
        .unwrap(),
        vertices: Default::default(),
        edge_geometry: vec![geometry(&selected), geometry(&unselected)],
        native_ref: None,
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::input_topologies::retain_selected_edge_geometry(
        &ctx,
        std::slice::from_ref(&feature),
        &mut topologies,
    )
    .unwrap();
    assert_eq!(topologies[0].edge_geometry, vec![geometry(&selected)]);
}

#[test]
fn input_edge_geometry_keeps_the_edges_that_bound_a_selected_face() {
    use cadmpeg_ir::features::evaluation::HistoricalEdgeGeometry;
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::math::Point3;

    let (mut feature, _, _) = input_fixture();
    let prefix = crate::ids::feature_input_prefix(&feature.id, 4);
    let removed = crate::ids::history_input_face_id(&prefix, 10);
    feature.evaluation = cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies: None,
            removed_faces: cadmpeg_ir::features::selections::FaceSelection::historical(
                crate::ids::history_input_state_id(&prefix),
                vec![removed.clone()],
                "f3d:test:shell-faces".to_owned(),
            )
            .unwrap(),
            thickness: None,
            outward: None,
            mode: None,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        }),
    );
    let geometry = |edge: i64, faces: [i64; 2]| HistoricalEdgeGeometry {
        edge: crate::ids::history_input_edge_id(&prefix, edge),
        interior: FinitePoint3::new(Point3::new(0.0, 0.0, 5.0)).unwrap(),
        tangent: None,
        start: None,
        end: None,
        faces: cadmpeg_ir::features::selections::DistinctMembers::try_from(
            faces
                .map(|face| crate::ids::history_input_face_id(&prefix, face))
                .to_vec(),
        )
        .unwrap(),
        carrier: None,
        supports: Vec::new(),
    };
    let mut topologies = vec![cadmpeg_ir::features::evaluation::FeatureInputTopology {
        face_owners: Vec::new(),
        face_geometry: Vec::new(),
        body_writers: Vec::new(),
        id: crate::ids::history_input_state_id(&prefix),
        input_of: feature.id.clone(),
        bodies: Default::default(),
        faces: Default::default(),
        edges: Default::default(),
        vertices: Default::default(),
        edge_geometry: vec![geometry(40, [10, 11]), geometry(41, [11, 12])],
        native_ref: None,
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::input_topologies::retain_selected_edge_geometry(
        &ctx,
        std::slice::from_ref(&feature),
        &mut topologies,
    )
    .unwrap();
    assert_eq!(topologies[0].edge_geometry, vec![geometry(40, [10, 11])]);
}

fn relation(owner_ref: i64, member_refs: &[i64]) -> crate::history_records::AsmHistoricalRelation {
    crate::history_records::AsmHistoricalRelation {
        owner_ref,
        member_refs: member_refs.to_vec(),
    }
}

/// Two bodies. Body 1 holds faces 2 and 3 through region 11 and shell 21.
/// Body 5 holds face 6 through region 15 and shell 25, and face 7 through
/// region 16 and shell 26. Face 7 is also in shell 21. Face 8 is in no shell.
fn multi_body_topology() -> crate::history_records::AsmHistoricalTopology {
    crate::history_records::AsmHistoricalTopology {
        bodies: vec![1, 5],
        regions: vec![11, 15, 16],
        shells: vec![21, 25, 26],
        faces: vec![2, 3, 6, 7, 8],
        body_regions: vec![relation(1, &[11]), relation(5, &[15, 16])],
        region_shells: vec![
            relation(11, &[21]),
            relation(15, &[25]),
            relation(16, &[26]),
        ],
        shell_faces: vec![
            relation(21, &[2, 3, 7]),
            relation(25, &[6]),
            relation(26, &[7]),
        ],
        ..crate::history_records::AsmHistoricalTopology::default()
    }
}

#[test]
fn input_topology_states_the_body_that_holds_each_face() {
    let (feature, scope, mut history) = input_fixture();
    crate::test_support::history::edit(&mut history, 0, |topology, _| {
        *topology = multi_body_topology();
    });
    let projected = super::super::input_topologies::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        std::slice::from_mut(&mut history),
        &[],
        &std::collections::HashMap::new(),
    );
    let prefix = crate::ids::feature_input_prefix(&feature.id, 4);
    let owner = |face, body| cadmpeg_ir::features::evaluation::HistoricalFaceOwner {
        face: crate::ids::history_input_face_id(&prefix, face),
        body: crate::ids::history_input_body_id(&prefix, body),
    };
    assert_eq!(projected.len(), 1);
    // Face 7 is listed by shells of both bodies and face 8 by none, so neither
    // has an owner.
    assert_eq!(
        projected[0].face_owners,
        vec![owner(2, 1), owner(3, 1), owner(6, 5)]
    );
}

#[test]
fn input_topology_of_one_body_states_no_face_owner() {
    let (feature, scope, mut history) = input_fixture();
    crate::test_support::history::edit(&mut history, 0, |topology, _| {
        *topology = crate::history_records::AsmHistoricalTopology {
            bodies: vec![1],
            regions: vec![11],
            shells: vec![21],
            faces: vec![2],
            body_regions: vec![relation(1, &[11])],
            region_shells: vec![relation(11, &[21])],
            shell_faces: vec![relation(21, &[2])],
            ..crate::history_records::AsmHistoricalTopology::default()
        };
    });
    let projected = super::super::input_topologies::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        std::slice::from_mut(&mut history),
        &[],
        &std::collections::HashMap::new(),
    );
    assert_eq!(projected.len(), 1);
    assert!(projected[0].face_owners.is_empty());
}

#[test]
fn input_face_owners_keep_only_the_faces_a_feature_selects() {
    use cadmpeg_ir::features::direct_edit::FaceMotion;
    use cadmpeg_ir::features::evaluation::{FeatureInputTopology, HistoricalFaceOwner};
    use cadmpeg_ir::features::selections::{DistinctMembers, FaceSelection};

    let (mut feature, _, _) = input_fixture();
    let prefix = crate::ids::feature_input_prefix(&feature.id, 4);
    let face = |slot| crate::ids::history_input_face_id(&prefix, slot);
    let body = |slot| crate::ids::history_input_body_id(&prefix, slot);
    feature.evaluation = cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::MoveFace {
            faces: FaceSelection::historical(
                crate::ids::history_input_state_id(&prefix),
                vec![face(6)],
                "f3d:test:offset-faces".to_owned(),
            )
            .unwrap(),
            motion: FaceMotion::Offset {
                distance: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
            },
        }),
    );
    let owner = |slot, owner| HistoricalFaceOwner {
        face: face(slot),
        body: body(owner),
    };
    let mut topologies = vec![FeatureInputTopology {
        face_owners: vec![owner(2, 1), owner(6, 5)],
        face_geometry: Vec::new(),
        body_writers: Vec::new(),
        id: crate::ids::history_input_state_id(&prefix),
        input_of: feature.id.clone(),
        bodies: DistinctMembers::try_from(vec![body(1), body(5)]).unwrap(),
        faces: DistinctMembers::try_from(vec![face(2), face(6)]).unwrap(),
        edges: Default::default(),
        vertices: Default::default(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::input_topologies::retain_selected_face_owners(
        &ctx,
        std::slice::from_ref(&feature),
        &mut topologies,
    )
    .unwrap();
    assert_eq!(topologies[0].face_owners, vec![owner(6, 5)]);
}

fn face_surfaces() -> [cadmpeg_ir::features::evaluation::HistoricalFaceSurface; 2] {
    use cadmpeg_ir::features::evaluation::HistoricalFaceSurface;
    use cadmpeg_ir::math::{Point3, Vector3};
    [
        HistoricalFaceSurface::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 10.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        HistoricalFaceSurface::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .unwrap(),
        ),
    ]
}

#[test]
fn input_face_geometry_carries_planar_and_cylindrical_surfaces_of_the_state() {
    use crate::history_records::{AsmHistoricalFaceSample, AsmHistoricalTopology};
    use cadmpeg_ir::features::evaluation::HistoricalFaceGeometry;

    let [plane, cylinder] = face_surfaces();
    let topology = AsmHistoricalTopology {
        faces: vec![2, 3, 9],
        face_samples: vec![
            AsmHistoricalFaceSample {
                face: 2,
                surface: plane.clone(),
                reversed: false,
            },
            AsmHistoricalFaceSample {
                face: 3,
                surface: cylinder.clone(),
                reversed: true,
            },
            AsmHistoricalFaceSample {
                face: 7,
                surface: plane.clone(),
                reversed: false,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let feature: cadmpeg_ir::features::FeatureId =
        "f3d:model:feature#offset".to_owned().try_into().unwrap();
    let prefix = crate::ids::feature_input_prefix(&feature, 4);
    assert_eq!(
        super::super::input_topologies::project_input_face_geometry(&topology, &prefix),
        vec![
            HistoricalFaceGeometry {
                face: crate::ids::history_input_face_id(&prefix, 2),
                surface: plane,
                reversed: false,
            },
            HistoricalFaceGeometry {
                face: crate::ids::history_input_face_id(&prefix, 3),
                surface: cylinder,
                reversed: true,
            },
        ]
    );
}

#[test]
fn input_face_geometry_keeps_only_the_faces_a_feature_selects() {
    use cadmpeg_ir::features::direct_edit::FaceMotion;
    use cadmpeg_ir::features::evaluation::{FeatureInputTopology, HistoricalFaceGeometry};
    use cadmpeg_ir::features::selections::{DistinctMembers, FaceSelection};

    let (mut feature, _, _) = input_fixture();
    let prefix = crate::ids::feature_input_prefix(&feature.id, 4);
    let face = |slot| crate::ids::history_input_face_id(&prefix, slot);
    feature.evaluation = cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::MoveFace {
            faces: FaceSelection::historical(
                crate::ids::history_input_state_id(&prefix),
                vec![face(3)],
                "f3d:test:offset-faces".to_owned(),
            )
            .unwrap(),
            motion: FaceMotion::Offset {
                distance: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
            },
        }),
    );
    let [plane, cylinder] = face_surfaces();
    let geometry = |slot, surface| HistoricalFaceGeometry {
        face: face(slot),
        surface,
        reversed: false,
    };
    let mut topologies = vec![FeatureInputTopology {
        face_owners: Vec::new(),
        face_geometry: vec![geometry(2, plane), geometry(3, cylinder.clone())],
        body_writers: Vec::new(),
        id: crate::ids::history_input_state_id(&prefix),
        input_of: feature.id.clone(),
        bodies: Default::default(),
        faces: DistinctMembers::try_from(vec![face(2), face(3)]).unwrap(),
        edges: Default::default(),
        vertices: Default::default(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::input_topologies::retain_selected_face_geometry(
        &ctx,
        std::slice::from_ref(&feature),
        &mut topologies,
    )
    .unwrap();
    assert_eq!(topologies[0].face_geometry, vec![geometry(3, cylinder)]);
}

/// A Hole that selects face 10 and a Chamfer that selects edge 2, both on the
/// input state 4 of one history, projected as the decode path projects them:
/// before the history releases its projection rows, then reduced to the
/// selected geometry once the definitions are final.
#[test]
fn hole_and_chamfer_keep_selected_input_geometry_across_projection_release() {
    use crate::history_records::{
        AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::features::edge_treatments::{ChamferGroup, ChamferSpec};
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::selections::{EdgeSelection, FaceSelection, NonEmptyMembers};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::scalar::PositiveLength;

    let (base, _, history) = input_fixture();
    let mut topology = AsmHistoricalTopology {
        bodies: vec![1],
        faces: vec![10, 20, 30],
        edges: vec![1, 2],
        vertices: vec![4, 5],
        loops: vec![100, 200, 300],
        ..AsmHistoricalTopology::default()
    };
    for (face, loop_) in [(10, 100), (20, 200), (30, 300)] {
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![loop_],
        });
    }
    for (coedge, loop_, edge) in [(101, 100, 1), (201, 200, 1), (202, 200, 2), (301, 300, 2)] {
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge,
            owner_loop: loop_,
            edge,
            next: coedge,
            previous: coedge,
            radial_next: coedge,
        });
    }
    for edge in [1, 2] {
        topology.edge_vertices.push(AsmHistoricalEdge {
            edge,
            start_vertex: 4,
            end_vertex: 5,
        });
        topology
            .edge_samples
            .push(crate::test_support::history::edge_sample(
                edge,
                Point3::new(0.0, 0.0, f64::from(u8::try_from(edge).unwrap())),
                None,
            ));
    }
    let state = history.states()[0].clone();
    let mut histories = vec![crate::test_support::history::history(
        history.id.clone(),
        history.byte_offset,
        history.preamble,
        false,
        vec![(state, Some(topology), Vec::new())],
    )];

    let scope_for = |id: &str| {
        let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
            id,
            crate::records::feature::scope::DesignFeatureKind::WorkPoint,
            7,
        );
        scope
            .try_edit(|draft| {
                draft.previous_history_state_id = Some(4);
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    let mut hole = base.clone();
    hole.id = "f3d:model:feature#hole".to_owned().try_into().unwrap();
    hole.native_ref = Some("f3d:design:scope#hole".to_owned());
    let mut chamfer = base.clone();
    chamfer.id = "f3d:model:feature#chamfer".to_owned().try_into().unwrap();
    chamfer.native_ref = Some("f3d:design:scope#chamfer".to_owned());
    let scopes = [
        scope_for("f3d:design:scope#hole"),
        scope_for("f3d:design:scope#chamfer"),
    ];

    let hole_prefix = crate::ids::feature_input_prefix(&hole.id, 4);
    let chamfer_prefix = crate::ids::feature_input_prefix(&chamfer.id, 4);
    let mut hole_definition: cadmpeg_ir::features::FeatureDefinition =
        cadmpeg_core::serde_erase::json::from_value(cadmpeg_core::json!({"kind": "operation", "value": {"kind": "hole", "value": {"face": {"kind": "faces", "value": ["test:model:face#support"]},
            "placements": [{
                "kind": "directed",
                "position": {"x": 1.0, "y": 2.0, "z": 3.0},
                "direction": {"x": 0.0, "y": 0.0, "z": -1.0}
            }],
            "shape": {
                "construction": {
                    "kind": "form",
                    "value": {"kind": {"kind": "simple_drilled", "drill_point_angle": 2.0}}
                },
                "diameter": 5.0
            },
            "extent": {"kind": "blind", "length": 10.0}}}}))
        .unwrap();
    let cadmpeg_ir::features::FeatureDefinition::Operation(FeatureOperation::Hole { face, .. }) =
        &mut hole_definition
    else {
        panic!("Hole definition");
    };
    *face = Some(
        FaceSelection::historical(
            crate::ids::history_input_state_id(&hole_prefix),
            vec![crate::ids::history_input_face_id(&hole_prefix, 10)],
            "f3d:test:hole-face".to_owned(),
        )
        .unwrap(),
    );
    hole.evaluation = FeatureEvaluation::from_definition(hole_definition);
    chamfer.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups: NonEmptyMembers::one(ChamferGroup {
                edges: EdgeSelection::historical(
                    crate::ids::history_input_state_id(&chamfer_prefix),
                    vec![crate::ids::history_input_edge_id(&chamfer_prefix, 2)],
                    "f3d:test:chamfer-group".to_owned(),
                )
                .unwrap(),
                spec: ChamferSpec::Distance {
                    distance: PositiveLength::new(1.0).unwrap(),
                },
            }),
            flip_direction: false,
        },
    ));
    let features = [hole, chamfer];

    let mut topologies =
        super::super::input_topologies::project_feature_input_topologies_with_senses(
            &features,
            &scopes,
            &mut histories,
            &[],
            &std::collections::HashMap::new(),
            &crate::history::ChamferSenses::default(),
        );
    assert_eq!(topologies.len(), 2);
    crate::history::discard_projection_caches(&mut histories);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::input_topologies::retain_selected_edge_geometry(&ctx, &features, &mut topologies)
        .unwrap();

    let edges_of = |topology: &cadmpeg_ir::features::evaluation::FeatureInputTopology| {
        topology
            .edge_geometry
            .iter()
            .map(|geometry| geometry.edge.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    // The Hole selects face 10, so it keeps edge 1, the one edge that bounds it.
    assert_eq!(
        edges_of(&topologies[0]),
        vec![crate::ids::history_input_edge_id(&hole_prefix, 1)
            .as_str()
            .to_owned()]
    );
    // The Chamfer keeps the edge it selects.
    assert_eq!(
        edges_of(&topologies[1]),
        vec![crate::ids::history_input_edge_id(&chamfer_prefix, 2)
            .as_str()
            .to_owned()]
    );
}
