// SPDX-License-Identifier: Apache-2.0

use super::{scope_between, state};
use crate::history::bind_feature_outputs;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalPlane,
    AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology,
};
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
};
use cadmpeg_ir::math::{Point3, Vector3};

fn split_body_test_feature(scope_id: &str) -> Feature {
    Feature {
        id: FeatureId::mint("test:model:feature#split").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Split Body".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::SplitBody {
                targets: cadmpeg_ir::features::BodySelection::Unresolved,
                tool: cadmpeg_ir::features::SplitBodyTool::Faces(
                    cadmpeg_ir::features::FaceSelection::Faces(vec![]),
                ),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(scope_id.into()),
        product_definition: None,
    }
}

fn two_solid_boxes_with_planes(
    b1: i64,
    min1: Point3,
    max1: Point3,
    b2: i64,
    min2: Point3,
    max2: Point3,
    planes: Vec<AsmHistoricalPlane>,
) -> AsmHistoricalTopology {
    let mut faces = vec![4, 104];
    let mut face_surfaces = Vec::new();
    for (i, p) in planes.iter().enumerate() {
        let fid = 200 + i as i64;
        faces.push(fid);
        face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: fid,
            carrier: p.surface,
        });
    }
    AsmHistoricalTopology {
        bodies: vec![b1, b2],
        body_regions: vec![
            AsmHistoricalRelation {
                owner_ref: b1,
                member_refs: vec![2],
            },
            AsmHistoricalRelation {
                owner_ref: b2,
                member_refs: vec![102],
            },
        ],
        region_shells: vec![
            AsmHistoricalRelation {
                owner_ref: 2,
                member_refs: vec![3],
            },
            AsmHistoricalRelation {
                owner_ref: 102,
                member_refs: vec![103],
            },
        ],
        shell_faces: vec![
            AsmHistoricalRelation {
                owner_ref: 3,
                member_refs: vec![4],
            },
            AsmHistoricalRelation {
                owner_ref: 103,
                member_refs: vec![104],
            },
        ],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 4,
                member_refs: vec![5],
            },
            AsmHistoricalRelation {
                owner_ref: 104,
                member_refs: vec![105],
            },
        ],
        loop_coedges: vec![
            AsmHistoricalRelation {
                owner_ref: 5,
                member_refs: vec![6, 7],
            },
            AsmHistoricalRelation {
                owner_ref: 105,
                member_refs: vec![106, 107],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 6,
                owner_loop: 5,
                edge: 8,
                next: 7,
                previous: 7,
                radial_next: 7,
            },
            AsmHistoricalCoedge {
                coedge: 7,
                owner_loop: 5,
                edge: 8,
                next: 6,
                previous: 6,
                radial_next: 6,
            },
            AsmHistoricalCoedge {
                coedge: 106,
                owner_loop: 105,
                edge: 108,
                next: 107,
                previous: 107,
                radial_next: 107,
            },
            AsmHistoricalCoedge {
                coedge: 107,
                owner_loop: 105,
                edge: 108,
                next: 106,
                previous: 106,
                radial_next: 106,
            },
        ],
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 8,
                start_vertex: 10,
                end_vertex: 11,
            },
            AsmHistoricalEdge {
                edge: 108,
                start_vertex: 110,
                end_vertex: 111,
            },
        ],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 21,
            },
            AsmHistoricalCarrierBinding {
                entity: 110,
                carrier: 120,
            },
            AsmHistoricalCarrierBinding {
                entity: 111,
                carrier: 121,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 20,
                position: min1,
            },
            AsmHistoricalPoint {
                point: 21,
                position: max1,
            },
            AsmHistoricalPoint {
                point: 120,
                position: min2,
            },
            AsmHistoricalPoint {
                point: 121,
                position: max2,
            },
        ],
        faces,
        face_surfaces,
        surface_planes: planes,
        ..AsmHistoricalTopology::default()
    }
}

#[test]
fn split_plane_only_emitted_for_split_body() {
    let scope = scope_between(1, 0);

    let state1_topo = AsmHistoricalTopology {
        faces: vec![10],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 20,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
        ..AsmHistoricalTopology::default()
    };

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_topo),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );

    // 1. SplitBody: emits split_plane
    let mut feature_split = split_body_test_feature(&scope.id);

    bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history.clone()],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();

    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"1,2,3;0,0,1".to_string())
    );

    // 2. Extrude: does NOT emit split_plane
    let mut feature_extrude = Feature {
        id: FeatureId::mint("test:model:feature#extrude").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Planar(
                    cadmpeg_ir::features::PlanarProfileRef::Native("scope".into()),
                ),
                direction: cadmpeg_ir::features::ExtrudeDirection::default(),
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
                extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                    side: cadmpeg_ir::features::ExtrudeSide {
                        termination: cadmpeg_ir::features::LinearTermination::ThroughAll {},
                        draft: None,
                    },
                },
                op: cadmpeg_ir::features::BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: Some(false),
                allow_multi_profile_faces: Some(false),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(scope.id.clone()),
        product_definition: None,
    };

    bind_feature_outputs(
        std::slice::from_mut(&mut feature_extrude),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();

    assert_eq!(
        feature_extrude
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        None
    );
}

#[test]
fn split_plane_unresolved_when_multiple_inserted_planes_ambiguous() {
    let scope = scope_between(1, 0);

    let state1_topo = AsmHistoricalTopology {
        faces: vec![10, 11],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 21,
            },
        ],
        surface_planes: vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
            },
        ],
        ..AsmHistoricalTopology::default()
    };

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_topo),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );

    let mut feature_split = split_body_test_feature(&scope.id);

    bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();

    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        None
    );
}

#[test]
fn split_plane_emitted_when_multiple_inserted_planes_are_equivalent() {
    let scope = scope_between(1, 0);

    let state1_topo = AsmHistoricalTopology {
        faces: vec![10, 11],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 21,
            },
        ],
        surface_planes: vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(1.0, 1.0, 0.0),
                normal: Vector3::new(0.0, 0.0, -1.0),
            },
        ],
        ..AsmHistoricalTopology::default()
    };

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_topo),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );

    let mut feature_split = split_body_test_feature(&scope.id);

    bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();

    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"0,0,0;0,0,1".to_string())
    );
}

#[test]
fn split_plane_separates_body_centers_when_bodies_exist() {
    let scope = scope_between(1, 0);

    // 1. Plane at z=1.5 separates body 1 at z=1 and body 2 at z=2
    let topo_separating = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, 1.5),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
    );
    let history_sep = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), topo_separating),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );
    let mut feature_split = split_body_test_feature(&scope.id);
    bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history_sep],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"0,0,1.5;0,0,1".to_string())
    );

    // 2. Plane at z=0 does NOT separate body 1 at z=1 and body 2 at z=2
    let topo_non_separating = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
    );
    let history_non_sep = crate::test_support::history::history(
        "f3d:asm-history#2".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), topo_non_separating),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );
    let mut feature_split2 = split_body_test_feature(&scope.id);
    bind_feature_outputs(
        std::slice::from_mut(&mut feature_split2),
        std::slice::from_ref(&scope),
        &mut [history_non_sep],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feature_split2
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        None
    );
}
