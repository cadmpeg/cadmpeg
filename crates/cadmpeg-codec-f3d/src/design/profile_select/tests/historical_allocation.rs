// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder,
    AsmHistoricalEdge, AsmHistoricalEntityDelta, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology, AsmHistoricalTopologyDelta, AsmHistoricalTransition,
    AsmHistory, AsmTopologyCache,
};

fn transition_state(
    state_id: i64,
    topology: AsmHistoricalTopology,
    transition: Option<AsmHistoricalTransition>,
) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("synthetic:transition-state-{state_id}"),
        parent: "synthetic:history".into(),
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
        topology_cache: AsmTopologyCache::Complete(topology),
        transition,
    }
}

fn empty_sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:id#transition-allocation-sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(
            Vec::<Vec<SketchEntityUse>>::new()).unwrap(),
        native_ref: None,
    }
}

fn assert_transition_collection_refusal(operation: &'static str, deleted: bool) {
    let sketch = empty_sketch();
    let previous_topology = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding { entity: 10, carrier: 50 },
            AsmHistoricalCarrierBinding { entity: 11, carrier: 50 },
        ],
        ..AsmHistoricalTopology::default()
    };
    let transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta {
            faces: AsmHistoricalEntityDelta {
                inserted: if deleted { Vec::new() } else { vec![10] },
                deleted: if deleted { vec![10, 11] } else { Vec::new() },
                ..AsmHistoricalEntityDelta::default()
            },
            ..AsmHistoricalTopologyDelta::default()
        },
    };
    let histories = [AsmHistory {
        id: "synthetic:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            transition_state(1, previous_topology, None),
            transition_state(2, AsmHistoricalTopology::default(), Some(transition)),
        ],
    }];
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scope_histories = HashMap::new();
        let resolution = ExtrudeProfileResolution {
            entities: &[], spatial_sketches: &[], spatial_entities: &[],
            histories: &histories, scope_histories: &scope_histories,
            linear_tolerance: 0.000001, angular_tolerance: 0.000000001,
            arrangement_budget: &arrangement_budget, ctx: Some(&ctx),
        };
        match super::super::transition_profile_selection(
            &sketch, resolution.scoped(&histories), 2, 1,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected transition refusal at {operation}: {other:?}"),
        }
    }
    panic!("no transition refusal at {operation}");
}

#[test]
fn transition_inserted_selection_refuses_collection_limit() {
    assert_transition_collection_refusal("f3d transition inserted selection", false);
}

#[test]
fn transition_cylindrical_selection_refuses_collection_limit() {
    assert_transition_collection_refusal("f3d transition cylindrical selection", false);
}

#[test]
fn transition_deleted_selection_refuses_collection_limit() {
    assert_transition_collection_refusal("f3d transition deleted selection", true);
}

fn assert_deleted_carrier_refusal(operation: &'static str) {
    let topology = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding { entity: 10, carrier: 50 },
            AsmHistoricalCarrierBinding { entity: 11, carrier: 50 },
        ],
        ..AsmHistoricalTopology::default()
    };
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::unique_multi_face_deleted_carrier_family(
            &[10, 11], &topology, Some(&ctx),
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected deleted carrier refusal at {operation}: {other:?}"),
        }
    }
    panic!("no deleted carrier refusal at {operation}");
}

#[test]
fn deleted_face_uniqueness_refuses_collection_limit() {
    assert_deleted_carrier_refusal("f3d deleted face uniqueness index");
}

#[test]
fn deleted_carrier_family_refuses_collection_limit() {
    assert_deleted_carrier_refusal("f3d deleted carrier family index");
}

#[test]
fn deleted_carrier_face_refuses_collection_limit() {
    assert_deleted_carrier_refusal("f3d deleted carrier family face");
}

fn assert_inserted_selection_refusal(operation: &'static str, region: bool) {
    use super::super::ResolvedProfileSelection;

    let sketch = empty_sketch();
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let selections = if region {
            let region = cadmpeg_ir::features::SketchProfileRegion::loops(0, vec![1]).unwrap();
            vec![
                Some(ResolvedProfileSelection::Regions(vec![region])),
                Some(ResolvedProfileSelection::Loops(vec![0])),
                Some(ResolvedProfileSelection::Loops(vec![1])),
            ]
        } else {
            vec![
                Some(ResolvedProfileSelection::Loops(vec![0])),
                Some(ResolvedProfileSelection::Loops(vec![1])),
            ]
        };
        match super::super::transition_inserted_profile_selection(
            &sketch, &[], 0.000001, selections, Some(&ctx),
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected inserted selection refusal at {operation}: {other:?}"),
        }
    }
    panic!("no inserted selection refusal at {operation}");
}

#[test]
fn inserted_transition_loop_refuses_collection_limit() {
    assert_inserted_selection_refusal("f3d inserted transition profile loop", false);
}

#[test]
fn inserted_transition_hole_refuses_collection_limit() {
    assert_inserted_selection_refusal("f3d inserted transition region hole", true);
}

#[test]
fn inserted_transition_region_refuses_collection_limit() {
    assert_inserted_selection_refusal("f3d inserted transition region", true);
}

#[test]
fn cylindrical_profile_projected_points_refuse_collection_limit() {
    let sketch_id = SketchId::mint("synthetic:test:id#cylinder-allocation-sketch").unwrap();
    let circle_id = SketchEntityId::mint("synthetic:test:id#cylinder-allocation-circle").unwrap();
    let circle = SketchEntity::new(
        circle_id.clone(), sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0), radius: Length::new(2.0).unwrap(),
        }).unwrap(),
    );
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![
            SketchEntityUse { entity: circle_id, reversed: false },
        ]]).unwrap(),
        native_ref: None,
    };
    let topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation { owner_ref: 10, member_refs: vec![11] }],
        loop_coedges: vec![AsmHistoricalRelation { owner_ref: 11, member_refs: vec![12, 13, 14] }],
        coedge_topology: vec![
            AsmHistoricalCoedge { coedge: 12, owner_loop: 11, edge: 20, next: 13, previous: 14, radial_next: 12 },
            AsmHistoricalCoedge { coedge: 13, owner_loop: 11, edge: 21, next: 14, previous: 12, radial_next: 13 },
            AsmHistoricalCoedge { coedge: 14, owner_loop: 11, edge: 22, next: 12, previous: 13, radial_next: 14 },
        ],
        edge_vertices: vec![
            AsmHistoricalEdge { edge: 20, start_vertex: 30, end_vertex: 31 },
            AsmHistoricalEdge { edge: 21, start_vertex: 31, end_vertex: 32 },
            AsmHistoricalEdge { edge: 22, start_vertex: 32, end_vertex: 30 },
        ],
        vertex_points: vec![
            AsmHistoricalCarrierBinding { entity: 30, carrier: 40 },
            AsmHistoricalCarrierBinding { entity: 31, carrier: 41 },
            AsmHistoricalCarrierBinding { entity: 32, carrier: 42 },
        ],
        point_positions: vec![
            AsmHistoricalPoint { point: 40, position: Point3::new(2.0, 0.0, 0.0) },
            AsmHistoricalPoint { point: 41, position: Point3::new(0.0, 2.0, 1.0) },
            AsmHistoricalPoint { point: 42, position: Point3::new(-2.0, 0.0, 0.0) },
        ],
        face_surfaces: vec![AsmHistoricalCarrierBinding { entity: 10, carrier: 50 }],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 50, origin: Point3::new(0.0, 0.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0), radius: 2.0,
        }],
        ..AsmHistoricalTopology::default()
    };
    for limit in 0..32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::inserted_cylindrical_profile_selection(
            &sketch, std::slice::from_ref(&circle), &topology, 10, 0.000001, 0.000000001,
            Some(&ctx),
        ) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d cylindrical profile projected point" => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected cylindrical projected point refusal: {other:?}"),
        }
    }
    panic!("no cylindrical projected point refusal");
}
