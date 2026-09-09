// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.

use super::super::*;
use crate::records::topology::DesignOperandRole;

#[test]
fn split_face_targets_bind_from_a_transition_predecessor() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
    use crate::records::feature::DesignParameterScope;
    use crate::records::topology::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignFaceOperand,
    };
    use crate::records::ConstructionRecipeKind;
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, SplitFaceTool,
    };
    use cadmpeg_ir::ids::FaceId;

    let scope_id = "f3d:Design/BulkStream.dat:scope#42".to_string();
    let group_id = "f3d:Design/BulkStream.dat:operand-group#100".to_string();
    let face_id = FaceId::mint("f3d:brep:entity#7").expect("identity grammar");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::DesignFeatureKind::SplitFace,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
        })
        .unwrap();

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: group_id.clone(),
            scope_record_index: 42,
            scope_reference_ordinal: 2,
            record_index: 100,
            byte_offset: 1000,
            class_tag: crate::records::DesignClassTag::try_from("297".to_owned()).unwrap(),
            members: vec![crate::records::Located {
                value: 200,
                offset: 1010,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1008,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1048,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1052,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                DesignOperandRole::ROLE_0X10,
            ),
            role_offset: 1030,
            paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned()).unwrap(),
            paired_byte_offset: 1100,
        },
    )
    .unwrap();
    let operand = DesignFaceOperand::try_new(crate::records::topology::DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-face-operand#200".into(),
        scope_record_index: 42,
        scope_reference_ordinal: 3,
        group: Some(crate::records::topology::DesignOperandGroup {
            group_record_index: 100,
            group_member_ordinal: 0,
        }),
        record_index: 200,
        byte_offset: 1200,
        class_tag: crate::records::DesignClassTag::try_from("297".to_owned()).unwrap(),
        paired_byte_offset: 1300,
        paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned()).unwrap(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 1400,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
        recipe_prefix_offset: 1411,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1420,
        recipe_program: Vec::new(),

        recipe_nodes: Vec::new(),
        candidate_faces: vec![face_id.clone()],
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: vec![face_id.clone()],
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 204,
        next_byte_offset: 1500,
    })
    .unwrap();
    let state = |state_id, transition| AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(
                2,
                Some(crate::history_records::AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: Default::default(),
                    topology: Default::default(),
                }),
            ),
            state(1, None),
        ],
    };
    let mut features = vec![Feature {
        id: FeatureId::mint("f3d:test:feature#42").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("SplitFace".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::SplitFace {
                targets: FaceSelection::Native(group_id.clone()),
                tool: SplitFaceTool::Plane {
                    plane: FeatureId::mint("f3d:test:feature#plane").expect("identity grammar"),
                },
            },
        ),
        native_ref: Some(scope_id),
    }];

    super::super::bind_feature_face_selections(
        &mut features,
        &mut [],
        &[scope],
        &[group],
        &[operand],
        &[],
        &[],
        &[history],
    )
    .unwrap();

    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::SplitFace {
            targets: FaceSelection::Resolved { faces, native },
            ..
        } if faces == &[face_id] && native == &group_id
    ));
}

#[test]
fn thread_face_group_uses_first_reference_transition_candidates() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalTopology,
        AsmHistoricalTransition, AsmHistory,
    };
    use crate::records::feature::{
        DesignParameterScope, DesignThreadConstruction, DesignThreadForm,
    };
    use crate::records::topology::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame, DesignFaceOperand,
    };
    use crate::records::{ConstructionRecipeKind, DesignRecipeReference};
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar");
    let scope_id = "f3d:Design/BulkStream.dat:scope#42";
    let mut scope = DesignParameterScope::empty(
        scope_id,
        crate::records::feature::DesignFeatureKind::Thread,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
            scope_record_index: 42,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 1_000,
            class_tag: crate::records::DesignClassTag::try_from("297".to_owned()).unwrap(),
            members: vec![crate::records::Located {
                value: 200,
                offset: 1_010,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_008,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_048,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_052,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                DesignOperandRole::ROLE_0X10,
            ),
            role_offset: 1_030,
            paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned()).unwrap(),
            paired_byte_offset: 1_100,
        },
    )
    .unwrap();
    let reference = |token: &str, design_reference, candidates: &[i64]| DesignRecipeReference {
        selector: 1,
        selector_offset: 1_411,
        token: token.into(),
        token_offset: 1_415,
        design_reference,
        design_reference_offset: 1_420,
        candidate_faces: candidates.iter().copied().map(face).collect(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let operand = DesignFaceOperand::try_new(crate::records::topology::DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-face-operand#200".into(),
        scope_record_index: 42,
        scope_reference_ordinal: 1,
        group: Some(crate::records::topology::DesignOperandGroup {
            group_record_index: 100,
            group_member_ordinal: 0,
        }),
        record_index: 200,
        byte_offset: 1_200,
        class_tag: crate::records::DesignClassTag::try_from("297".to_owned()).unwrap(),
        paired_byte_offset: 1_300,
        paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned()).unwrap(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 1_400,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
        recipe_prefix_offset: 1_411,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: vec![reference("3", 203, &[7, 8]), reference("-1", 199, &[9, 10])],
        recipe_kind: ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 1_430,
        recipe_program: vec![0, -1, 2],

        recipe_nodes: Vec::new(),
        candidate_faces: [7, 8, 9, 10].into_iter().map(face).collect(),
        unreferenced_candidate_faces: [9, 10].into_iter().map(face).collect(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 204,
        next_byte_offset: 1_500,
    })
    .unwrap();

    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: Default::default(),
    };
    transition.topology.faces.updated.push(7);
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: (state_id == 2).then_some(1),
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(2, AsmHistoricalTopology::default(), Some(transition)),
            state(
                1,
                AsmHistoricalTopology {
                    faces: vec![7, 8, 9, 10],
                    persistent_subentity_tags: [
                        (7, "3", 203),
                        (8, "3", 203),
                        (9, "-1", 199),
                        (10, "-1", 199),
                    ]
                    .into_iter()
                    .map(|(entity_ref, token, design_reference)| {
                        crate::history_records::AsmHistoricalPersistentSubentityTag {
                            entity_kind: AsmHistoricalEntityKind::Face,
                            entity_ref,
                            selector: 17,
                            token: token.into(),
                            design_references: vec![design_reference],
                            ordinal: 0,
                        }
                    })
                    .collect(),
                    ..AsmHistoricalTopology::default()
                },
                None,
            ),
        ],
    };

    let mut operands = vec![operand.clone()];
    bind_face_operand_history_candidates(
        &mut operands,
        std::slice::from_ref(&scope),
        std::slice::from_ref(&group),
        &[],
        std::slice::from_ref(&history),
        &HashMap::new(),
    );
    assert_eq!(operands[0].preceding_candidate_faces, [face(7), face(8)]);
    assert_eq!(operands[0].changed_candidate_faces, [face(7)]);
    assert_eq!(operands[0].resolved_face_slots, [7]);

    let mut cylinder_scope = scope.clone();
    if let crate::records::feature::DesignScopePayloadMut::Thread(slot) =
        cylinder_scope.payload_mut()
    {
        *slot = Some(DesignThreadConstruction {
            form: DesignThreadForm::Standard,
            designation_offset: 0,
            designation: cadmpeg_ir::NonEmptyString::new("M4x0.7").unwrap(),
            nominal_size: crate::records::feature::DesignThreadNominalSize::try_from(
                "4.0".to_owned(),
            )
            .expect("nominal size"),
            profile: cadmpeg_ir::NonEmptyString::new("ISO Metric profile").unwrap(),
            pitch: crate::records::feature::DesignPositiveScalar::new(0.07).unwrap(),
            face_group_record_indices: vec![100],
            diameters: crate::records::feature::DesignThreadDiameters::new(0.4, 0.2, 0.3).unwrap(),
        });
    }
    let mut cylinder_operand = operand.clone();
    cylinder_operand.recipe_references[0].candidate_faces =
        vec![FaceId::mint("f3d:brep/input/brep:entity#999").expect("identity grammar")];
    cylinder_operand.candidate_faces = cylinder_operand.recipe_references[0]
        .candidate_faces
        .clone();
    let mut cylinder_history = history.clone();
    cylinder_history.id = "f3d:Breps.BlobParts/BREP.input:asm-history#1".into();
    let cylinder_topology = cylinder_history.states[1]
        .topology_mut()
        .expect("preceding topology");
    cylinder_topology.face_surfaces = vec![
        AsmHistoricalCarrierBinding {
            entity: 7,
            carrier: 70,
        },
        AsmHistoricalCarrierBinding {
            entity: 8,
            carrier: 80,
        },
    ];
    cylinder_topology.surface_cylinders = vec![
        AsmHistoricalCylinder {
            surface: 70,
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 1.5,
        },
        AsmHistoricalCylinder {
            surface: 80,
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 3.0,
        },
    ];
    let mut cylinder_operands = vec![cylinder_operand];
    bind_face_operand_history_candidates(
        &mut cylinder_operands,
        std::slice::from_ref(&cylinder_scope),
        std::slice::from_ref(&group),
        &[],
        std::slice::from_ref(&cylinder_history),
        &HashMap::new(),
    );
    assert_eq!(cylinder_operands[0].resolved_face_slots, [7]);

    let mut stale_active_operand = cylinder_operands[0].clone();
    stale_active_operand.recipe_references[0]
        .candidate_faces
        .push(FaceId::mint("f3d:brep/input/brep:entity#998").expect("identity grammar"));
    let mut stale_active_operands = vec![stale_active_operand];
    bind_face_operand_history_candidates(
        &mut stale_active_operands,
        std::slice::from_ref(&cylinder_scope),
        std::slice::from_ref(&group),
        &[],
        std::slice::from_ref(&cylinder_history),
        &HashMap::new(),
    );
    assert_eq!(stale_active_operands[0].resolved_face_slots, [7]);

    let mut ambiguous_geometry_history = cylinder_history;
    ambiguous_geometry_history.states[0]
        .transition
        .as_mut()
        .expect("result transition")
        .topology
        .faces
        .updated
        .push(8);
    ambiguous_geometry_history.states[1]
        .topology_mut()
        .expect("preceding topology")
        .surface_cylinders[1]
        .radius = 1.6;
    let mut ambiguous_geometry_operands = vec![cylinder_operands.remove(0)];
    bind_face_operand_history_candidates(
        &mut ambiguous_geometry_operands,
        &[cylinder_scope],
        std::slice::from_ref(&group),
        &[],
        &[ambiguous_geometry_history],
        &HashMap::new(),
    );
    assert!(ambiguous_geometry_operands[0]
        .resolved_face_slots
        .is_empty());

    let mut unrelated_group = group;
    unrelated_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::FACES);
    let mut rejected = vec![operand];
    bind_face_operand_history_candidates(
        &mut rejected,
        &[scope],
        &[unrelated_group],
        &[],
        &[history],
        &HashMap::new(),
    );
    assert_eq!(rejected[0].preceding_candidate_faces, [face(9), face(10)]);
    assert!(rejected[0].changed_candidate_faces.is_empty());
    assert!(rejected[0].resolved_face_slots.is_empty());
}

#[test]
fn history_binding_budget_charges_materialized_state_tables() {
    let mut limits = cadmpeg_core::decode::ResourceLimits::desktop();
    limits.max_materialized_bytes = 1920;
    assert!(!complete_table_binding_budget_exceeded([5, 5], &limits));
    assert!(complete_table_binding_budget_exceeded([10, 1], &limits));
    assert!(complete_table_binding_budget_exceeded(
        [usize::MAX, 1],
        &limits,
    ));

    let desktop = cadmpeg_core::decode::ResourceLimits::desktop();
    let service = cadmpeg_core::decode::ResourceLimits::service();
    assert!(!complete_table_binding_budget_exceeded(
        [18_000_000],
        &desktop,
    ));
    assert!(complete_table_binding_budget_exceeded(
        [18_000_000],
        &service,
    ));
}

#[test]
fn unresolved_new_body_sweep_mode_follows_output_body_kind() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, SweepMode, SweepSection};
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};

    let body = |id: &str, kind| Body {
        id: BodyId::mint(id).expect("identity grammar"),
        kind,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let sweep = |id: &str, outputs| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Sweep {
                shape: cadmpeg_ir::features::SweepShape::new(
                    SweepSection::Unresolved(None),
                    Vec::new(),
                    SweepMode::Unresolved,
                )
                .unwrap(),

                path: None,

                orientation: None,
                transition: None,
                transformation: None,
                path_tangent: false,
                linearize: false,
                twist: None,
                path_extent: None,
                guide_rail: None,
                taper: None,
                scale: None,
                allow_multi_profile_faces: None,
            },
            outputs,
        )
        .unwrap(),
        native_ref: None,
    };
    let bodies = [
        body("test:model:body#sheet", BodyKind::Sheet),
        body("test:model:body#solid", BodyKind::Solid),
    ];
    let mut features = [
        sweep(
            "synthetic:test:id#sheet-sweep",
            vec![BodyId::mint("test:model:body#sheet").expect("identity grammar")],
        ),
        sweep(
            "synthetic:test:id#solid-sweep",
            vec![BodyId::mint("test:model:body#solid").expect("identity grammar")],
        ),
        sweep(
            "synthetic:test:id#mixed-sweep",
            vec![
                BodyId::mint("test:model:body#sheet").expect("identity grammar"),
                BodyId::mint("test:model:body#solid").expect("identity grammar"),
            ],
        ),
        sweep(
            "synthetic:test:id#missing-sweep",
            vec![BodyId::mint("test:model:body#missing").expect("identity grammar")],
        ),
    ];

    bind_sweep_result_modes(&mut features, &bodies).unwrap();

    let modes = features.map(|feature| match feature.evaluation.definition() {
        FeatureDefinition::Sweep { shape, .. } => shape.mode(),
        _ => unreachable!(),
    });
    assert_eq!(modes[0], SweepMode::Surface);
    assert_eq!(modes[1], SweepMode::NewBody);
    assert_eq!(modes[2], SweepMode::Unresolved);
    assert_eq!(modes[3], SweepMode::Unresolved);
}

#[test]
fn historical_brep_source_qualifies_state_local_candidates() {
    assert_eq!(
        historical_brep_source("f3d:asset/Breps.BlobParts/BREP.example.smbh:asm-delta-state#42"),
        Some("example.smbh")
    );
    assert_eq!(historical_brep_source("f3d:unqualified:state#42"), None);
}

#[test]
fn legacy_extrude_face_lane_prefers_history_then_source_identity() {
    use crate::history_records::AsmHistoricalTopology;
    use cadmpeg_ir::ids::FaceId;
    use std::collections::HashSet;

    let source_face = |source: &str, slot| {
        FaceId::mint(format!("f3d:brep/{source}/brep:entity#{slot}")).expect("identity grammar")
    };
    let active_candidates = vec![source_face("old", 10), source_face("new", 10)];
    assert_eq!(
        select_legacy_extrude_face_candidate(
            &active_candidates,
            &AsmHistoricalTopology::default(),
            &HashSet::new(),
            Some("old"),
        ),
        Some(LegacyFaceResolution::Active(source_face("old", 10)))
    );
    assert_eq!(
        select_legacy_extrude_face_candidate(
            &active_candidates,
            &AsmHistoricalTopology::default(),
            &HashSet::new(),
            Some("missing"),
        ),
        None
    );

    let historical_candidates = vec![
        FaceId::mint("f3d:brep:entity#20").expect("identity grammar"),
        source_face("new", 21),
    ];
    let topology = AsmHistoricalTopology {
        faces: vec![20, 21],
        ..AsmHistoricalTopology::default()
    };
    let mut changed = HashSet::new();
    changed.insert(21);
    assert_eq!(
        select_legacy_extrude_face_candidate(
            &historical_candidates,
            &topology,
            &changed,
            Some("new"),
        ),
        Some(LegacyFaceResolution::Historical(21))
    );
    assert_eq!(
        select_legacy_extrude_face_candidate(
            &[FaceId::mint("f3d:brep:entity#20").expect("identity grammar")],
            &topology,
            &changed,
            None,
        ),
        Some(LegacyFaceResolution::Historical(20))
    );
}

#[test]
fn hole_face_selection_binds_to_the_feature_input_topology() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory,
    };
    use crate::records::feature::{
        DesignHoleConstruction, DesignHoleFaceSelection, DesignParameterScope,
    };
    use crate::records::topology::DesignEntitySelectionFaceCandidate;
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureInputTopology, HoleKind,
        LinearTermination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let feature_id = FeatureId::mint("f3d:test:feature#42").expect("identity grammar");
    let scope_id = "f3d:Design/BulkStream.dat:scope#42";
    let mut scope = DesignParameterScope::empty(
        scope_id,
        crate::records::feature::DesignFeatureKind::Hole,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        *slot = Some(DesignHoleConstruction {
            point_record_index: 55,
            point_record_byte_offset: 0,
            position: [0.0; 3],
            position_offset: 0,
            direction: [0.0, 0.0, 1.0],
            direction_offset: 0,
            point_parameters: [0.0; 2],
            point_parameter_offsets: [0, 0],
            reference_type: 0,
            reference_type_offset: 0,
            tangent_point_data: None,
            input_records: vec![crate::records::Located {
                value: 55,
                offset: 0,
            }],
            face_selection: Some(DesignHoleFaceSelection {
                record_index: 100,
                byte_offset: 0,
                class_tag: crate::records::DesignClassTag::try_from("333".to_owned()).unwrap(),
                asset_id: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                asset_id_offset: 0,
                context_id: "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                context_id_offset: 0,
                identity_record_index: 103,
                identity_record_offset: 0,
                primary_identity: 18044,
                primary_identity_offset: 0,
                secondary: None,
                historical_face_candidates: vec![DesignEntitySelectionFaceCandidate {
                    history_id: "f3d:asset/Breps.BlobParts/BREP.example.smbh:asm-delta-state#2"
                        .into(),
                    historical: crate::records::topology::HistoricalBinding {
                        kind: AsmHistoricalEntityKind::Pcurve,
                        entity_ref: 18044,
                        state_ids: vec![1],
                    },
                    face_slot: 30,
                }],
                next_record_index: 104,
                next_byte_offset: 0,
            }),
        });
    }
    let mut feature = Feature::new(
        feature_id.clone(),
        0,
        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: Some(FaceSelection::Native(scope_id.into())),
            direction: None,
            placements: Some(vec![cadmpeg_ir::features::HolePlacement::Directed {
                position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                    0.0, 0.0, 1.0,
                ))
                .unwrap(),
            }]),
            shape: cadmpeg_ir::features::HoleShape::new(
                cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
                None,
                Some(cadmpeg_ir::features::PositiveLength::new(5.0).unwrap()),
            )
            .unwrap(),

            extent: Some(LinearTermination::Blind {
                length: cadmpeg_ir::features::NonZeroLength::new(10.0).unwrap(),
            }),
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        },
    );
    feature.native_ref = Some(scope_id.into());
    let mut input_topologies = vec![FeatureInputTopology {
        id: crate::design::edge_resolve::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id.clone(),
        bodies: (Vec::new()).try_into().unwrap(),
        faces: (Vec::new()).try_into().unwrap(),
        edges: (Vec::new()).try_into().unwrap(),
        vertices: (Vec::new()).try_into().unwrap(),
        native_ref: None,
    }];
    let state = |state_id, transition| AsmDeltaState {
        id: format!("history:state#{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(
                2,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: Default::default(),
                    topology: Default::default(),
                }),
            ),
            state(1, None),
        ],
    };

    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        &[scope],
        &[],
        &[],
        &[],
        &[],
        &[history],
    )
    .unwrap();

    let FeatureDefinition::Hole {
        face:
            Some(FaceSelection::Historical {
                state,
                faces,
                native,
            }),
        ..
    } = feature.evaluation.definition()
    else {
        panic!("Hole support face remains unresolved");
    };
    assert_eq!(native, scope_id);
    assert_eq!(
        state,
        &crate::design::edge_resolve::feature_input_topology_id(&feature_id, 1)
    );
    assert_eq!(faces.len(), 1);
    assert_eq!(input_topologies[0].faces.as_slice(), faces.as_slice());
}
