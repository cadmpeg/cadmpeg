// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::design::geometry::{region_containing_points, MAX_ARRANGEMENT_WALK_WORK};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
    AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory,
};
use crate::ids::{
    neutral_sketch_curve_id, neutral_sketch_id, neutral_spatial_sketch_curve_id,
    neutral_spatial_sketch_id,
};
use crate::records::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
    DesignEntitySelectionOperand, DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember,
    DesignSketchPlacement, DesignSketchProfileOperand, DesignSketchProfileRegion,
    DesignSketchProfileRegionMember, DesignSketchProfileRegionSelection, SketchCurveIdentity,
    SketchRelationOperand,
};
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::features::{Angle, Length, PathRef, ProfileRef, SketchProfileRegion};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    SketchPlacement, SpatialSketch, SpatialSketchEntity, SpatialSketchEntityUse,
    SpatialSketchGeometry, SpatialSketchProfile,
};

fn group() -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup {
        id: "stream:group".into(),
        scope_record_index: 7,
        scope_reference_ordinal: 0,
        record_index: 9,
        byte_offset: 0,
        class_tag: "277".into(),
        members: vec![crate::records::Located { value: 10, offset: 0 }, crate::records::Located { value: 11, offset: 0 }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x5_0000_0000,
        extrude_role: None,
        role_offset: 0,
        paired_class_tag: "277".into(),
        paired_byte_offset: 0,
    }
}

fn operand(
    record_index: u32,
    ordinal: u32,
    secondary_identity: u64,
) -> DesignEntitySelectionOperand {
    DesignEntitySelectionOperand {
        id: format!("stream:operand-{record_index}"),
        scope_record_index: 7,
        group_record_index: 9,
        group_member_ordinal: ordinal,
        record_index,
        byte_offset: 0,
        class_tag: "277".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        identity_record_index: record_index + 1,
        identity_record_offset: 0,
        primary_identity: 42,
        primary_identity_offset: 0,
        secondary_identity: Some(crate::records::Located { value: secondary_identity, offset: 0 }),
        curve_secondary_identity: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: record_index + 2,
        next_byte_offset: 0,
    }
}

fn placement() -> DesignSketchPlacement {
    DesignSketchPlacement {
        id: "stream:placement".into(),
        scope_record_index: Some(7),
        entity_id: "Sketch:42".into(),
        entity_suffix: 42,
        visibility: None,
        byte_offset: 0,
        class_tag: "277".into(),
        record_index: 20,
        frame_length: 0,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: None,
        paired_class_tag: "277".into(),
        paired_byte_offset: 0,
        member_run_head: false,
    }
}

fn curve(record_index: u32, primary_id: u64, secondary_id: u64) -> SketchCurveIdentity {
    SketchCurveIdentity {
        id: format!("stream:curve-{record_index}"),
        record_index,
        owner_reference: Some(42),
        class_tag: "450".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id,
        secondary_id,
        geometry: None,
    }
}

fn planar_resolution<'a>(
    operands: &'a [DesignEntitySelectionOperand],
    placements: &'a [DesignSketchPlacement],
    curve_identities: &'a [SketchCurveIdentity],
    sketches: &'a [Sketch],
    sketch_entities: &'a [SketchEntity],
) -> SketchProfileResolution<'a> {
    SketchProfileResolution {
        entities: &[],
        entity_selection_operands: operands,
        placements,
        curve_identities,
        sketches,
        sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
        linear_tolerance: 1.0e-7,
        angular_tolerance: 1.0e-9,
    }
}

fn profile_region_member(curve_primary_id: u64) -> DesignSketchProfileRegionMember {
    DesignSketchProfileRegionMember {
        kind: crate::records::DesignSketchProfileRegionMemberKind::Curve,
        kind_offset: 0,
        curve_primary_id,
        curve_primary_id_offset: 0,
        incidence_words: [0, 0, 0, 0, 1, 1, 0, 0],
        incidence_words_offset: 0,
    }
}

fn spatial_line(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary_id: u64,
    start: Point3,
    end: Point3,
) -> SpatialSketchEntity {
    SpatialSketchEntity::new(
        neutral_spatial_sketch_curve_id(sketch, primary_id, 0),
        sketch.clone(),
        SpatialSketchGeometry::Line { start, end },
    )
}

fn spatial_profile(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary_ids: &[u64],
) -> SpatialSketchProfile {
    SpatialSketchProfile {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        boundary: primary_ids
            .iter()
            .map(|primary_id| SpatialSketchEntityUse {
                entity: neutral_spatial_sketch_curve_id(sketch, *primary_id, 0),
                reversed: false,
            })
            .collect(),
    }
}

#[test]
fn spatial_extrude_profile_uses_persistent_curve_member_without_history() {
    let sketch_id =
        cadmpeg_ir::sketches::SpatialSketchId("f3d:model:spatial-sketch#selection".into());
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![
            spatial_profile(&sketch_id, &[100, 101, 102]),
            spatial_profile(&sketch_id, &[200, 201, 202]),
        ],
        native_ref: None,
    };
    let group = DesignExtrudeSelectionGroup {
        id: "f3d:Design/BulkStream.dat:selection-group#9".into(),
        scope_record_index: 7,
        scope_reference_ordinal: 0,
        record_index: 9,
        byte_offset: 0,
        class_tag: "277".into(),
        member_count_offset: 0,
        members: vec![crate::records::Located { value: 10, offset: 0 }],
        opaque_index: 1,
        opaque_index_offset: 0,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 0,
        variant: false,
        paired_class_tag: "277".into(),
        paired_byte_offset: 0,
    };
    let mut member = DesignExtrudeSelectionMember {
        id: "f3d:Design/BulkStream.dat:selection-member#10".into(),
        group_record_index: group.record_index,
        group_member_ordinal: 0,
        record_index: 10,
        byte_offset: 0,
        class_tag: "278".into(),
        local_id: 200,
        local_id_offset: 0,
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        tail_slot_present: false,
        tail_slot_offset: 0,
        resolved_geometry: Some(SketchRelationOperand::Curve {
            record_index: 20,
            primary_id: 200,
            secondary_id: 0,
        }),
        operand_identity_ids: Vec::new(),
        historical: None,
        next_record_index: 11,
        next_byte_offset: 0,
    };
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    let scope_histories = HashMap::new();
    let resolution = ExtrudeProfileResolution {
        entities: &[],
        spatial_sketches: &[],
        spatial_entities: &[],
        histories: &[],
        scope_histories: &scope_histories,
        linear_tolerance: 1.0e-6,
        angular_tolerance: 1.0e-9,
        arrangement_budget: &arrangement_budget,
    };
    let scoped_resolution = resolution.scoped(&[]);

    assert_eq!(
        resolved_spatial_extrude_profile_selection(
            &group,
            std::slice::from_ref(&member),
            &sketch,
            &[],
            scoped_resolution,
            None,
            None,
        ),
        Some(1)
    );

    let mut conflicting_group = group.clone();
    conflicting_group.members.push(crate::records::Located { value: 11, offset: 0 });
    let mut conflicting_member = member.clone();
    conflicting_member.id = "f3d:Design/BulkStream.dat:selection-member#11".into();
    conflicting_member.group_member_ordinal = 1;
    conflicting_member.record_index = 11;
    conflicting_member.local_id = 100;
    conflicting_member.resolved_geometry = Some(SketchRelationOperand::Curve {
        record_index: 21,
        primary_id: 100,
        secondary_id: 0,
    });
    assert_eq!(
        resolved_spatial_extrude_profile_selection(
            &conflicting_group,
            &[member.clone(), conflicting_member],
            &sketch,
            &[],
            scoped_resolution,
            None,
            None,
        ),
        None
    );

    member.resolved_geometry = None;
    assert_eq!(
        resolved_spatial_extrude_profile_selection(
            &group,
            std::slice::from_ref(&member),
            &sketch,
            &[],
            scoped_resolution,
            None,
            None,
        ),
        None
    );
    let single_profile = SpatialSketch {
        profiles: vec![sketch.profiles[0].clone()],
        ..sketch
    };
    assert_eq!(
        resolved_spatial_extrude_profile_selection(
            &group,
            &[member],
            &single_profile,
            &[],
            scoped_resolution,
            None,
            None,
        ),
        Some(0)
    );
}

#[test]
fn spatial_transition_does_not_select_a_translated_equal_length_profile() {
    let sketch_id =
        cadmpeg_ir::sketches::SpatialSketchId("f3d:model:spatial-sketch#transition".into());
    let entities = [
        spatial_line(
            &sketch_id,
            100,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            101,
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            102,
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            200,
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(12.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            201,
            Point3::new(12.0, 0.0, 0.0),
            Point3::new(10.0, 2.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            202,
            Point3::new(10.0, 2.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
        ),
    ];
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![
            spatial_profile(&sketch_id, &[100, 101, 102]),
            spatial_profile(&sketch_id, &[200, 201, 202]),
        ],
        native_ref: None,
    };
    let topology = AsmHistoricalTopology {
        faces: vec![1],
        loops: vec![10],
        coedges: vec![20, 21, 22],
        edges: vec![30, 31, 32],
        vertices: vec![40, 41, 42],
        points: vec![50, 51, 52],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![10],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![20, 21, 22],
        }],
        coedge_topology: [(20, 30, 21, 22), (21, 31, 22, 20), (22, 32, 20, 21)]
            .into_iter()
            .map(|(coedge, edge, next, previous)| AsmHistoricalCoedge {
                coedge,
                owner_loop: 10,
                edge,
                next,
                previous,
                radial_next: coedge,
            })
            .collect(),
        edge_vertices: [(30, 40, 41), (31, 41, 42), (32, 42, 40)]
            .into_iter()
            .map(|(edge, start_vertex, end_vertex)| AsmHistoricalEdge {
                edge,
                start_vertex,
                end_vertex,
            })
            .collect(),
        vertex_points: [(40, 50), (41, 51), (42, 52)]
            .into_iter()
            .map(|(entity, carrier)| AsmHistoricalCarrierBinding { entity, carrier })
            .collect(),
        point_positions: [
            (50, Point3::new(100.0, 0.0, 0.0)),
            (51, Point3::new(101.0, 0.0, 0.0)),
            (52, Point3::new(100.0, 1.0, 0.0)),
        ]
        .into_iter()
        .map(|(point, position)| AsmHistoricalPoint { point, position })
        .collect(),
        ..Default::default()
    };
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("history:state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 0,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition,
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(1, AsmHistoricalTopology::default(), None),
            state(
                2,
                topology,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: crate::history_records::AsmHistoricalEntityDelta::default(),
                    topology: AsmHistoricalTopologyDelta {
                        faces: crate::history_records::AsmHistoricalEntityDelta {
                            inserted: vec![1],
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                }),
            ),
        ],
    };

    assert_eq!(
        transition_spatial_profile_selection(&sketch, &entities, &[history], 2, 1, 1.0e-6,),
        None
    );
}

#[test]
fn spatial_transition_withholds_when_any_profile_boundary_is_nonlinear() {
    let sketch_id =
        cadmpeg_ir::sketches::SpatialSketchId("f3d:model:spatial-sketch#nonlinear".into());
    let mut entities = vec![
        spatial_line(
            &sketch_id,
            100,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            101,
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            102,
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ),
    ];
    let arc_id = neutral_spatial_sketch_curve_id(&sketch_id, 200, 0);
    entities.push(SpatialSketchEntity::new(
        arc_id.clone(),
        sketch_id.clone(),
        SpatialSketchGeometry::Arc {
            center: Point3::new(10.0, 10.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length(1.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
    ));
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![
            spatial_profile(&sketch_id, &[100, 101, 102]),
            SpatialSketchProfile {
                origin: Point3::new(10.0, 10.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
                boundary: vec![SpatialSketchEntityUse {
                    entity: arc_id,
                    reversed: false,
                }],
            },
        ],
        native_ref: None,
    };
    let points = [
        Point3::new(0.25, 0.25, 0.0),
        Point3::new(0.5, 0.25, 0.0),
        Point3::new(0.25, 0.5, 0.0),
    ];

    assert_eq!(
        spatial_polyline_profile_containing_points(&sketch, &entities, &points, 1.0e-6),
        None
    );
    let polyline_only = SpatialSketch {
        profiles: vec![sketch.profiles[0].clone()],
        ..sketch
    };
    assert_eq!(
        spatial_polyline_profile_containing_points(&polyline_only, &entities, &points, 1.0e-6,),
        Some(0)
    );
}

#[test]
fn loft_spatial_profile_regions_collapse_coincident_curve_revisions() {
    let placement = placement();
    let sketch_id = neutral_spatial_sketch_id(&placement);
    let curves = [curve(30, 100, 0), curve(31, 200, 0), curve(32, 201, 0)];
    let entity_id = |primary| neutral_spatial_sketch_curve_id(&sketch_id, primary, 0);
    let circle = |primary, radius, normal| {
        SpatialSketchEntity::new(
            entity_id(primary),
            sketch_id.clone(),
            SpatialSketchGeometry::Circle {
                center: Point3::new(0.0, 0.0, 0.0),
                normal,
                reference_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: Length(radius),
            },
        )
    };
    let spatial_entities = [
        circle(100, 2.0, Vector3::new(0.0, 0.0, 1.0)),
        circle(200, 1.0, Vector3::new(0.0, 0.0, 1.0)),
        circle(201, 1.0, Vector3::new(0.0, 0.0, -1.0)),
    ];
    let profile = |primary| SpatialSketchProfile {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        boundary: vec![SpatialSketchEntityUse {
            entity: entity_id(primary),
            reversed: false,
        }],
    };
    let spatial_sketches = [SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![profile(100), profile(200), profile(201)],
        native_ref: None,
    }];
    let profile_operand = DesignSketchProfileOperand {
        scope_reference_ordinal: 0,
        record_index: 10,
        byte_offset: 0,
        class_tag: "300".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        entity_id: placement.entity_id.clone(),
        entity_suffix: placement.entity_suffix,
        entity_reference_offset: 0,
        region_selection: Some(DesignSketchProfileRegionSelection {
            record_index: 13,
            byte_offset: 0,
            class_tag: "303".into(),
            region_count_offset: 0,
            regions: vec![
                DesignSketchProfileRegion {
                    member_count_offset: 0,
                    members: vec![profile_region_member(100)],
                },
                DesignSketchProfileRegion {
                    member_count_offset: 0,
                    members: vec![
                        profile_region_member(200),
                        profile_region_member(201),
                        profile_region_member(200),
                    ],
                },
            ],
            companion_class_tag: "304".into(),
            companion_byte_offset: 0,
        }),
        paired_class_tag: "301".into(),
        paired_byte_offset: 0,
    };
    let placements = [placement];
    let resolution = SketchProfileResolution {
        entities: &[],
        entity_selection_operands: &[],
        placements: &placements,
        curve_identities: &curves,
        sketches: &[],
        sketch_entities: &[],
        spatial_sketches: &spatial_sketches,
        spatial_sketch_entities: &spatial_entities,
        linear_tolerance: 1.0e-7,
        angular_tolerance: 1.0e-9,
    };

    assert_eq!(
        resolved_spatial_sketch_profile_regions(
            "stream",
            &profile_operand,
            &spatial_sketches[0],
            &resolution,
        ),
        Some(vec![0, 1])
    );

    let whole_sketch_operand = DesignSketchProfileOperand {
        region_selection: None,
        ..profile_operand.clone()
    };
    assert_eq!(
        resolved_spatial_sketch_profile_regions(
            "stream",
            &whole_sketch_operand,
            &spatial_sketches[0],
            &resolution,
        ),
        Some(vec![0, 1, 2])
    );
    assert_eq!(
        spatial_profile_containing_entity(&spatial_sketches[0], &entity_id(100)),
        Some(0)
    );
    let repeated_profile = SpatialSketch {
        profiles: vec![profile(100), profile(100)],
        ..spatial_sketches[0].clone()
    };
    assert_eq!(
        spatial_profile_containing_entity(&repeated_profile, &entity_id(100)),
        None
    );

    let mut noncoincident_entities = spatial_entities.to_vec();
    let SpatialSketchGeometry::Circle { center, .. } = &mut noncoincident_entities[2].geometry
    else {
        unreachable!()
    };
    center.x = 0.1;
    let noncoincident_resolution = SketchProfileResolution {
        spatial_sketch_entities: &noncoincident_entities,
        ..resolution
    };
    assert_eq!(
        resolved_spatial_sketch_profile_regions(
            "stream",
            &profile_operand,
            &spatial_sketches[0],
            &noncoincident_resolution,
        ),
        None
    );
}

#[test]
fn loft_multi_member_planar_entity_path_preserves_order_and_requires_complete_proof() {
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let sketch_entities = [
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch, 100, 101),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        ),
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch, 200, 201),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(1.0, 0.0),
                end: Point2::new(1.0, 1.0),
            },
        ),
    ];
    let sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles: Vec::new(),
        native_ref: None,
    }];
    let group = group();
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let placement_list = [placement];
    let resolution = planar_resolution(
        &operands,
        &placement_list,
        &curves,
        &sketches,
        &sketch_entities,
    );
    assert_eq!(
        resolved_loft_entity_selection_path(&group, &resolution),
        Some(PathRef::SketchCurves {
            sketch: sketch.clone(),
            curves: vec![
                neutral_sketch_curve_id(&sketch, 100, 101),
                neutral_sketch_curve_id(&sketch, 200, 201),
            ],
        })
    );

    let mut mixed_operands = operands.to_vec();
    mixed_operands[1].primary_identity = 43;
    let mixed_resolution = planar_resolution(
        &mixed_operands,
        &placement_list,
        &curves,
        &sketches,
        &sketch_entities,
    );
    assert!(resolved_loft_entity_selection_path(&group, &mixed_resolution).is_none());

    let incomplete_resolution = planar_resolution(
        &operands,
        &placement_list,
        &curves[..1],
        &sketches,
        &sketch_entities,
    );
    assert!(resolved_loft_entity_selection_path(&group, &incomplete_resolution).is_none());
}

#[test]
fn entity_selection_path_uses_spatial_sketch_for_nonplanar_owner() {
    let placement = placement();
    let spatial_sketch = neutral_spatial_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let spatial_entities = curves
        .iter()
        .map(|curve| {
            SpatialSketchEntity::new(
                neutral_spatial_sketch_curve_id(
                    &spatial_sketch,
                    curve.primary_id,
                    curve.secondary_id,
                ),
                spatial_sketch.clone(),
                SpatialSketchGeometry::Line {
                    start: Point3::new(0.0, 0.0, 0.0),
                    end: Point3::new(1.0, 0.0, 0.0),
                },
            )
            .with_native_ref(Some(curve.id.clone()))
        })
        .collect::<Vec<_>>();
    let group = group();
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let sketches = [];
    let spatial_sketches = [SpatialSketch {
        id: spatial_sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    }];
    let resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &sketches,
        sketch_entities: &[],
        spatial_sketches: &spatial_sketches,
        spatial_sketch_entities: &spatial_entities,
    };

    assert_eq!(
        resolve_entity_selection_path(&group, &resolution),
        Some(PathRef::SpatialSketchCurves {
            sketch: spatial_sketch.clone(),
            curves: curves
                .iter()
                .map(|curve| {
                    neutral_spatial_sketch_curve_id(
                        &spatial_sketch,
                        curve.primary_id,
                        curve.secondary_id,
                    )
                })
                .collect(),
        })
    );
}

#[test]
fn entity_selection_profile_requires_unique_profile_membership() {
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let curve_ids = curves
        .iter()
        .map(|curve| neutral_sketch_curve_id(&sketch, curve.primary_id, curve.secondary_id))
        .collect::<Vec<_>>();
    let sketch_entities = [
        SketchEntity::new(
            curve_ids[0].clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        ),
        SketchEntity::new(
            curve_ids[1].clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(1.0, 0.0),
                end: Point2::new(0.0, 0.0),
            },
        ),
    ];
    let mut sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles: vec![
            Vec::new(),
            curve_ids
                .iter()
                .cloned()
                .map(|entity| SketchEntityUse {
                    entity,
                    reversed: false,
                })
                .collect(),
        ],
        native_ref: None,
    }];
    let mut group = group();
    group.role = 0x41_0000_0000;
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &sketches,
        sketch_entities: &sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
    };
    assert_eq!(
        resolve_entity_selection_profile(&group, &resolution),
        Some(ProfileRef::SketchProfiles {
            sketch: sketch.clone(),
            profiles: vec![1],
        })
    );

    sketches[0].profiles[0].push(SketchEntityUse {
        entity: curve_ids[0].clone(),
        reversed: false,
    });
    let ambiguous_resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &sketches,
        sketch_entities: &sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
    };
    assert!(resolve_entity_selection_profile(&group, &ambiguous_resolution).is_none());
}

#[test]
fn entity_selection_profile_retains_an_open_curve_as_ordered_entities() {
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curve = curve(30, 100, 101);
    let entity_id = neutral_sketch_curve_id(&sketch, curve.primary_id, curve.secondary_id);
    let sketch_entities = [SketchEntity::new(
        entity_id.clone(),
        sketch.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    )];
    let sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles: Vec::new(),
        native_ref: None,
    }];
    let mut group = group();
    group.role = 0x41_0000_0000;
    group.members = vec![10].into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect();
    let operands = [operand(10, 0, 100)];
    let resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: std::slice::from_ref(&curve),
        sketches: &sketches,
        sketch_entities: &sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
    };

    assert_eq!(
        resolve_entity_selection_profile(&group, &resolution),
        Some(ProfileRef::SketchEntities {
            sketch,
            entities: vec![entity_id],
        })
    );
}

#[test]
fn planar_profile_regions_resolve_by_persistent_curve_members() {
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let first_entity = neutral_sketch_curve_id(&sketch, 100, 101);
    let second_entity = neutral_sketch_curve_id(&sketch, 200, 201);
    let sketch_entities = [
        SketchEntity::new(
            first_entity.clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        ),
        SketchEntity::new(
            second_entity.clone(),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(1.0, 1.0),
            },
        ),
    ];
    let mut source = Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles: vec![
            vec![SketchEntityUse {
                entity: first_entity.clone(),
                reversed: false,
            }],
            vec![SketchEntityUse {
                entity: second_entity.clone(),
                reversed: false,
            }],
        ],
        native_ref: None,
    };
    let operand = DesignSketchProfileOperand {
        scope_reference_ordinal: 0,
        record_index: 10,
        byte_offset: 0,
        class_tag: "300".into(),
        asset_id: placement.entity_id.clone(),
        asset_id_offset: 0,
        entity_id: placement.entity_id,
        entity_suffix: 42,
        entity_reference_offset: 0,
        region_selection: Some(DesignSketchProfileRegionSelection {
            record_index: 11,
            byte_offset: 0,
            class_tag: "301".into(),
            region_count_offset: 0,
            regions: vec![
                DesignSketchProfileRegion {
                    member_count_offset: 0,
                    members: vec![profile_region_member(200)],
                },
                DesignSketchProfileRegion {
                    member_count_offset: 0,
                    members: vec![profile_region_member(100)],
                },
            ],
            companion_class_tag: "302".into(),
            companion_byte_offset: 0,
        }),
        paired_class_tag: "303".into(),
        paired_byte_offset: 0,
    };

    assert_eq!(
        resolved_sketch_profile_regions("stream", &operand, &source, &curves, &sketch_entities,),
        Some(vec![1, 0])
    );

    let mut mixed_region = operand.clone();
    mixed_region
        .region_selection
        .as_mut()
        .expect("region selection")
        .regions[0]
        .members
        .push(profile_region_member(100));
    assert!(resolved_sketch_profile_regions(
        "stream",
        &mixed_region,
        &source,
        &curves,
        &sketch_entities,
    )
    .is_none());

    source.profiles[1].push(SketchEntityUse {
        entity: first_entity,
        reversed: false,
    });
    assert!(resolved_sketch_profile_regions(
        "stream",
        &operand,
        &source,
        &curves,
        &sketch_entities,
    )
    .is_none());
}

#[test]
fn historical_points_on_profile_boundaries_are_ambiguous() {
    let sketch_id = SketchId("sketch".into());
    let entity_id = SketchEntityId("line".into());
    let mut sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 20.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    };
    let entity = SketchEntity::new(
        entity_id,
        sketch_id,
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    );
    let point = Point3::new(11.0, 20.0, 9.0);
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    assert_eq!(
        region_containing_points(&sketch, std::slice::from_ref(&entity), &[point], 1.0e-6),
        None
    );
    assert_eq!(
        crate::design::profile_select::selection_containing_points(
            &sketch,
            std::slice::from_ref(&entity),
            &[point],
            1.0e-6,
            &arrangement_budget,
        ),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );

    let mut branched_sketch = sketch.clone();
    let start_branch_id = SketchEntityId("start-branch".into());
    let end_branch_id = SketchEntityId("end-branch".into());
    branched_sketch.profiles.extend([
        vec![SketchEntityUse {
            entity: start_branch_id.clone(),
            reversed: false,
        }],
        vec![SketchEntityUse {
            entity: end_branch_id.clone(),
            reversed: false,
        }],
    ]);
    let branch_entity = |id, start, end| {
        SketchEntity::new(
            id,
            branched_sketch.id.clone(),
            SketchGeometry::Line { start, end },
        )
    };
    let branched_entities = [
        entity.clone(),
        branch_entity(
            start_branch_id,
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
        ),
        branch_entity(end_branch_id, Point2::new(2.0, 0.0), Point2::new(2.0, 1.0)),
    ];
    let endpoints = [Point3::new(10.0, 20.0, 5.0), Point3::new(12.0, 20.0, 5.0)];
    assert_eq!(
        crate::design::profile_select::selection_containing_points(
            &branched_sketch,
            &branched_entities,
            &endpoints,
            1.0e-6,
            &arrangement_budget,
        ),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );

    sketch.profiles.push(sketch.profiles[0].clone());
    assert_eq!(
        region_containing_points(&sketch, std::slice::from_ref(&entity), &[point], 1.0e-6),
        None
    );
    assert_eq!(
        crate::design::profile_select::selection_containing_points(
            &sketch,
            std::slice::from_ref(&entity),
            &[point],
            1.0e-6,
            &arrangement_budget,
        ),
        None
    );
}

#[test]
fn historical_selection_preserves_first_member_region_order() {
    let region = |outer| SketchProfileRegion::Loops {
        outer,
        holes: Vec::new(),
    };
    assert_eq!(
        crate::design::profile_select::ordered_unique_profile_selections([
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(3)])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(1)])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(3)])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(2)])),
        ]),
        Some(
            crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                region(3),
                region(1),
                region(2),
            ])
        )
    );
    assert_eq!(
        crate::design::profile_select::ordered_unique_profile_selections([
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(vec![region(3)])),
            None,
        ]),
        None
    );
}

#[test]
fn multiple_extrude_profile_groups_merge_only_exact_same_kind_selections() {
    let sketch = SketchId("f3d:model:sketch#multi-profile".into());
    let loops = [
        ProfileRef::SketchProfiles {
            sketch: sketch.clone(),
            profiles: vec![3, 1],
        },
        ProfileRef::SketchProfiles {
            sketch: sketch.clone(),
            profiles: vec![1, 2],
        },
    ];
    assert_eq!(
        crate::design::profile_select::merge_resolved_profile_selections(&sketch, &loops),
        Some(ProfileRef::SketchProfiles {
            sketch: sketch.clone(),
            profiles: vec![3, 1, 2],
        })
    );

    let regions = [
        ProfileRef::SketchRegions {
            sketch: sketch.clone(),
            regions: vec![SketchProfileRegion::Loops {
                outer: 4,
                holes: vec![5],
            }],
        },
        ProfileRef::SketchRegions {
            sketch: sketch.clone(),
            regions: vec![SketchProfileRegion::Loops {
                outer: 2,
                holes: Vec::new(),
            }],
        },
    ];
    assert_eq!(
        crate::design::profile_select::merge_resolved_profile_selections(&sketch, &regions),
        Some(ProfileRef::SketchRegions {
            sketch: sketch.clone(),
            regions: vec![
                SketchProfileRegion::Loops {
                    outer: 4,
                    holes: vec![5],
                },
                SketchProfileRegion::Loops {
                    outer: 2,
                    holes: Vec::new(),
                },
            ],
        })
    );

    assert_eq!(
        crate::design::profile_select::merge_resolved_profile_selections(
            &sketch,
            &[loops[0].clone(), regions[0].clone()]
        ),
        None
    );
    assert_eq!(
        crate::design::profile_select::merge_resolved_profile_selections(
            &sketch,
            &[
                loops[0].clone(),
                ProfileRef::SketchSelection {
                    sketch: sketch.clone(),
                    selections: vec!["native-group".into()],
                },
            ]
        ),
        None
    );
}

#[test]
fn historical_profile_members_resolve_through_topology_ownership() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalOptionalCarrierBinding,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::AsmHistoricalEntityKind;

    let topology = AsmHistoricalTopology {
        faces: vec![10, 20],
        loops: vec![11, 21],
        coedges: vec![12, 22],
        edges: vec![30],
        surfaces: vec![40],
        pcurves: vec![50],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![11],
            },
            AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![21],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 30,
                previous: 12,
                next: 12,
                radial_next: 22,
            },
            AsmHistoricalCoedge {
                coedge: 22,
                owner_loop: 21,
                edge: 30,
                previous: 22,
                next: 22,
                radial_next: 12,
            },
        ],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 40,
        }],
        coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 12,
            carrier: Some(50),
        }],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        historical_profile_face_candidates(Some(AsmHistoricalEntityKind::Pcurve), 50, &topology,),
        HashSet::from([10])
    );
    assert_eq!(
        historical_profile_face_candidates(Some(AsmHistoricalEntityKind::Surface), 40, &topology,),
        HashSet::from([10])
    );
    assert_eq!(
        historical_profile_face_candidates(Some(AsmHistoricalEntityKind::Edge), 30, &topology,),
        HashSet::from([10, 20])
    );
}

#[test]
fn historical_face_points_require_complete_boundary_topology() {
    let mut topology = crate::history_records::AsmHistoricalTopology {
        faces: vec![10],
        loops: vec![11],
        coedges: vec![12, 13, 14],
        edges: vec![20, 21, 22],
        vertices: vec![30, 31, 32],
        points: vec![40, 41, 42],
        face_loops: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12, 13, 14],
        }],
        coedge_topology: vec![
            crate::history_records::AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 20,
                next: 13,
                previous: 14,
                radial_next: 12,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 13,
                owner_loop: 11,
                edge: 21,
                next: 14,
                previous: 12,
                radial_next: 13,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 14,
                owner_loop: 11,
                edge: 22,
                next: 12,
                previous: 13,
                radial_next: 14,
            },
        ],
        edge_vertices: vec![
            crate::history_records::AsmHistoricalEdge {
                edge: 20,
                start_vertex: 30,
                end_vertex: 31,
            },
            crate::history_records::AsmHistoricalEdge {
                edge: 21,
                start_vertex: 31,
                end_vertex: 32,
            },
            crate::history_records::AsmHistoricalEdge {
                edge: 22,
                start_vertex: 32,
                end_vertex: 30,
            },
        ],
        vertex_points: vec![
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 40,
            },
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 31,
                carrier: 41,
            },
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 32,
                carrier: 42,
            },
        ],
        point_positions: vec![
            crate::history_records::AsmHistoricalPoint {
                point: 40,
                position: Point3::new(0.0, 0.0, 0.0),
            },
            crate::history_records::AsmHistoricalPoint {
                point: 41,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            crate::history_records::AsmHistoricalPoint {
                point: 42,
                position: Point3::new(0.0, 1.0, 0.0),
            },
        ],
        ..crate::history_records::AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::design::profile_select::historical_face_points(10, &topology),
        Some(vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ])
    );

    topology.point_positions.pop();
    assert_eq!(
        crate::design::profile_select::historical_face_points(10, &topology),
        None
    );
}

#[test]
fn inserted_cylinder_selects_its_exact_circular_sketch_profile() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
        AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology,
    };

    let sketch_id = SketchId("sketch".into());
    let circle_id = SketchEntityId("circle".into());
    let circle = SketchEntity::new(
        circle_id.clone(),
        sketch_id.clone(),
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    );
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: circle_id,
            reversed: false,
        }]],
        native_ref: None,
    };
    let topology = AsmHistoricalTopology {
        faces: vec![10],
        loops: vec![11],
        coedges: vec![12, 13, 14],
        edges: vec![20, 21, 22],
        vertices: vec![30, 31, 32],
        points: vec![40, 41, 42],
        surfaces: vec![50],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12, 13, 14],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 20,
                next: 13,
                previous: 14,
                radial_next: 12,
            },
            AsmHistoricalCoedge {
                coedge: 13,
                owner_loop: 11,
                edge: 21,
                next: 14,
                previous: 12,
                radial_next: 13,
            },
            AsmHistoricalCoedge {
                coedge: 14,
                owner_loop: 11,
                edge: 22,
                next: 12,
                previous: 13,
                radial_next: 14,
            },
        ],
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 20,
                start_vertex: 30,
                end_vertex: 31,
            },
            AsmHistoricalEdge {
                edge: 21,
                start_vertex: 31,
                end_vertex: 32,
            },
            AsmHistoricalEdge {
                edge: 22,
                start_vertex: 32,
                end_vertex: 30,
            },
        ],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 50,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 40,
            },
            AsmHistoricalCarrierBinding {
                entity: 31,
                carrier: 41,
            },
            AsmHistoricalCarrierBinding {
                entity: 32,
                carrier: 42,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 40,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 41,
                position: Point3::new(0.0, 2.0, 1.0),
            },
            AsmHistoricalPoint {
                point: 42,
                position: Point3::new(-2.0, 0.0, 0.0),
            },
        ],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 50,
            origin: Point3::new(0.0, 0.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 2.0,
        }],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        crate::design::profile_select::inserted_cylindrical_profile_selection(
            &sketch,
            std::slice::from_ref(&circle),
            &topology,
            10,
            1.0e-6,
            1.0e-9,
        ),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0]))
    );
    let mut tilted = topology;
    tilted.surface_cylinders[0].axis = Vector3::new(0.0, 1.0, 0.0);
    assert_eq!(
        crate::design::profile_select::inserted_cylindrical_profile_selection(
            &sketch,
            std::slice::from_ref(&circle),
            &tilted,
            10,
            1.0e-6,
            1.0e-9,
        ),
        None
    );
}

#[test]
fn deleted_profile_family_requires_one_complete_multi_face_carrier() {
    use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};

    let topology = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::design::profile_select::unique_multi_face_deleted_carrier_family(
            &[20, 11, 10],
            &topology
        ),
        Some(vec![10, 11])
    );
    assert_eq!(
        crate::design::profile_select::unique_multi_face_deleted_carrier_family(
            &[10, 10],
            &topology
        ),
        None
    );

    let mut ambiguous = topology.clone();
    ambiguous.face_surfaces.extend([
        AsmHistoricalCarrierBinding {
            entity: 30,
            carrier: 300,
        },
        AsmHistoricalCarrierBinding {
            entity: 31,
            carrier: 300,
        },
    ]);
    assert_eq!(
        crate::design::profile_select::unique_multi_face_deleted_carrier_family(
            &[10, 11, 30, 31],
            &ambiguous
        ),
        None
    );

    let mut incomplete = topology;
    incomplete
        .face_surfaces
        .retain(|binding| binding.entity != 20);
    assert_eq!(
        crate::design::profile_select::unique_multi_face_deleted_carrier_family(
            &[10, 11, 20],
            &incomplete
        ),
        None
    );
}

#[test]
fn transition_profile_prefers_consistent_side_loops_and_combines_cap_boundaries() {
    use cadmpeg_ir::features::SketchProfileRegion;

    let sketch_id = SketchId("sketch".into());
    let mut profiles = Vec::new();
    let mut entities = Vec::new();
    for (profile_index, corners) in [
        [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
        [[6.0, 0.0], [8.0, 0.0], [8.0, 2.0], [6.0, 2.0]],
        [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]],
        [[3.0, 1.0], [5.0, 1.0], [5.0, 3.0], [3.0, 3.0]],
    ]
    .into_iter()
    .enumerate()
    {
        let mut profile = Vec::new();
        for edge_index in 0..corners.len() {
            let id = SketchEntityId(format!("profile-{profile_index}-edge-{edge_index}"));
            profile.push(SketchEntityUse {
                entity: id.clone(),
                reversed: false,
            });
            let [start_u, start_v] = corners[edge_index];
            let [end_u, end_v] = corners[(edge_index + 1) % corners.len()];
            entities.push(SketchEntity::new(
                id,
                sketch_id.clone(),
                SketchGeometry::Line {
                    start: Point2::new(start_u, start_v),
                    end: Point2::new(end_u, end_v),
                },
            ));
        }
        profiles.push(profile);
    }
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles,
        native_ref: None,
    };
    let transition_selection = |selections| {
        crate::design::profile_select::transition_inserted_profile_selection(
            &sketch, &entities, 1.0e-6, selections,
        )
    };

    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), Some(3), Some(3)]),
        Some(3)
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), None, Some(3)]),
        Some(3)
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), Some(4)]),
        None
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection(std::iter::empty::<Option<u32>>()),
        None
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([None::<u32>, None]),
        None
    );
    let region = crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
        SketchProfileRegion::Loops {
            outer: 0,
            holes: vec![1],
        },
    ]);
    assert_eq!(
        transition_selection(vec![
            Some(region.clone()),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0, 1])),
        ]),
        Some(region.clone())
    );
    assert_eq!(
        transition_selection(vec![
            Some(region.clone()),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2])),
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2]))
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(Vec::new())),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1]))
    );
    assert_eq!(
        transition_selection(vec![Some(region)]),
        Some(
            crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                SketchProfileRegion::Loops {
                    outer: 0,
                    holes: vec![1],
                },
            ])
        )
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            None,
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0, 1]))
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2])),
        ]),
        None
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![3])),
        ]),
        None
    );
    assert_eq!(transition_selection(vec![None]), None);
}
