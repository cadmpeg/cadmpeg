// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalCarrierBinding};
use crate::records::dimensions::DesignRecipeReference;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeSide, FeatureEvaluation, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::sketches::{SketchId, SketchPlacement};

const LINEAR_TOLERANCE: f64 = 1.0e-8;
const ANGULAR_TOLERANCE: f64 = 1.0e-9;

fn fixture() -> (
    Feature,
    Sketch,
    DesignParameterScope,
    DesignConstructionOperandGroup,
    DesignFaceOperand,
) {
    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#1424",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        1424,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(22);
        })
        .unwrap();
    let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
        "id":"f3d:test:group#1448","scope_record_index":1424,"scope_reference_ordinal":0,
        "record_index":1448,"byte_offset":0,"class_tag":"338","members":[1451],"member_offsets":[0],
        "frame":{"member_count_offset":0,"opaque_index":1,"opaque_index_offset":18,
            "opaque_scalar":0.0,"opaque_scalar_offset":22,"variant":false},
        "role":0x0000_0011_0000_0000_u64,"extrude_role":"faces","extrude_face_role":"termination",
        "role_offset":0,"paired_class_tag":"261","paired_byte_offset":0
    }))
    .unwrap();
    let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id":"f3d:test:face-operand#272639","scope_record_index":1424,"scope_reference_ordinal":1,
        "group_record_index":1448,"group_member_ordinal":0,"record_index":1451,"byte_offset":0,
        "class_tag":"283","paired_byte_offset":12,"paired_class_tag":"258",
        "recipe_record_index":1454,"recipe_record_byte_offset":24,"recipe_id":"f3d:test:recipe#305",
        "recipe_prefix_offset":35,"recipe_prefix_bytes":"","recipe_references":[],
        "recipe_kind":"bounded_face","recipe_program_offset":0,"recipe_program":[0,-1,2],
        "recipe_node_offsets":[],"recipe_nodes":[],"next_record_index":1456,"next_byte_offset":36
    }))
    .unwrap();
    operand.recipe_references = vec![DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "2".into(),
        token_offset: 0,
        design_reference: 305,
        design_reference_offset: 0,
        candidate_faces: vec![crate::ids::brep_face_id(3484)],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
        operand_offset: None,
    }];
    operand.candidate_faces = vec![
        crate::ids::brep_face_id(3465),
        crate::ids::brep_face_id(3484),
    ];
    operand.preceding_candidate_faces = vec![crate::ids::brep_face_id(3465)];
    operand.unreferenced_candidate_faces = operand.preceding_candidate_faces.clone();
    let sketch = Sketch {
        id: SketchId::mint("f3d:test:sketch#180").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: Default::default(),
        native_ref: None,
    };
    let feature = Feature {
        product_definition: None,
        id: crate::ids::neutral_feature_id(&scope),
        ordinal: 5,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch.id.clone())),
                direction: ExtrudeDirection::ProfileNormal {},
                start: ExtrudeStart::OffsetProfilePlane {
                    offset: cadmpeg_ir::scalar::Length::new(7.5).unwrap(),
                },
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: FaceSelection::Native(group.id.clone()),
                            offset: Some(cadmpeg_ir::scalar::Length::new(-1.5).unwrap()),
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::Join,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        )),
        native_ref: Some(scope.id.clone()),
    };
    (feature, sketch, scope, group, operand)
}

fn add_plane(topology: &mut AsmHistoricalTopology, face: i64, z: f64) {
    topology.faces.push(face);
    topology.surfaces.push(face + 10000);
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: face,
        carrier: face + 10000,
    });
    topology.surface_planes.push(AsmHistoricalPlane {
        surface: face + 10000,
        origin: Point3::new(0.0, 0.0, z),
        normal: Vector3::new(0.0, 0.0, 1.0),
    });
}

fn state(id: i64, mut topology: AsmHistoricalTopology) -> crate::test_support::history::State {
    topology.faces.sort_unstable();
    topology.surfaces.sort_unstable();
    topology.face_surfaces.sort_by_key(|binding| binding.entity);
    topology.surface_planes.sort_by_key(|plane| plane.surface);
    let versions = topology
        .faces
        .iter()
        .chain(&topology.surfaces)
        .map(|slot| AsmEntityVersion {
            entity_ref: *slot,
            record_ref: *slot,
        })
        .collect();
    (
        AsmDeltaState {
            id: format!("f3d:history:state#{id}"),
            parent: "f3d:history".into(),
            byte_offset: 0,
            state_id: id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(topology),
        versions,
    )
}

fn run(ambiguous: bool, cap: f64) -> (FaceSelection, Vec<i64>, Vec<LossNote>) {
    run_configured(ambiguous, cap, None, |_, _, _| {})
}

fn run_configured(
    ambiguous: bool,
    cap: f64,
    terminal_cap: Option<f64>,
    configure: impl FnOnce(&mut Feature, &mut DesignFaceOperand, &mut AsmHistoricalTopology),
) -> (FaceSelection, Vec<i64>, Vec<LossNote>) {
    let (mut feature, sketch, scope, group, mut operand) = fixture();
    feature.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided { side },
            ..
        }) = definition
        else {
            panic!("Extrude")
        };
        let LinearTermination::ToFace { face, .. } = &mut side.termination else {
            panic!("face termination")
        };
        *face = FaceSelection::Resolved {
            faces: vec![crate::ids::brep_face_id(3465)],
            native: group.id.clone(),
        };
    });
    let mut before = AsmHistoricalTopology::default();
    add_plane(&mut before, 3465, 0.0);
    add_plane(&mut before, 3484, 20.0);
    if ambiguous {
        add_plane(&mut before, 3486, 20.0);
        operand.recipe_references[0]
            .candidate_faces
            .push(crate::ids::brep_face_id(3486));
    }
    configure(&mut feature, &mut operand, &mut before);
    let mut after = before.clone();
    add_plane(&mut after, 394, cap);
    if let Some(cap) = terminal_cap {
        add_plane(&mut after, 395, cap);
    }
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![state(22, after), state(6, before)],
    );
    let losses = bind(
        std::slice::from_mut(&mut feature),
        &[sketch],
        &mut Inputs {
            scopes: &[scope],
            groups: &[group],
            operands: std::slice::from_mut(&mut operand),
            histories: &[history],
            scope_histories: &HashMap::new(),
            linear_tolerance: LINEAR_TOLERANCE,
            angular_tolerance: ANGULAR_TOLERANCE,
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("Extrude")
    };
    let LinearTermination::ToFace { face, .. } = &side.termination else {
        panic!("face termination")
    };
    (face.clone(), operand.resolved_face_slots, losses)
}

#[test]
fn recipe_candidate_with_stored_offset_cap_wins_over_preceding_candidate() {
    let (face, slots, losses) = run(false, 18.5);
    assert!(matches!(face, FaceSelection::Historical { .. }));
    assert_eq!(selection_slots(&face), Some(vec![3484]));
    assert_eq!(slots, [3484]);
    assert!(losses.is_empty());
}

#[test]
fn conflicting_candidates_without_unique_cap_agreement_stay_native() {
    for (ambiguous, cap) in [(true, 18.5), (false, 17.0)] {
        let (face, slots, losses) = run(ambiguous, cap);
        assert_eq!(face, FaceSelection::Native("f3d:test:group#1448".into()));
        assert!(slots.is_empty());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.extrude-target-disputed"
        );
    }
}

#[test]
fn termination_binding_uses_resolved_recipe_slot_over_derived_predecessor() {
    let (_, _, scope, group, mut operand) = fixture();
    let mut selection = FaceSelection::Native(group.id.clone());
    super::super::selection::bind_face_selection(
        &mut selection,
        &scope,
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[],
    );
    assert_eq!(selection, FaceSelection::Native(group.id.clone()));
    operand.resolved_face_slots = vec![3484];
    super::super::selection::bind_face_selection(
        &mut selection,
        &scope,
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[],
    );
    assert_eq!(selection_slots(&selection), Some(vec![3484]));
}

#[test]
fn recipe_target_planes_match_stored_signed_offset_caps() {
    for (target, offset, cap) in [(8.0, 16.0, 24.0), (20.0, -4.0, 16.0), (5.0, 0.0, 5.0)] {
        let mut before = AsmHistoricalTopology::default();
        add_plane(&mut before, 10, target);
        let cap_plane = AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, cap),
            normal: Vector3::new(0.0, 0.0, 1.0),
        };
        assert_eq!(
            select_target(
                &BTreeSet::from([10]),
                &before,
                &[&cap_plane],
                Some(Point3::new(0.0, 0.0, 0.0)),
                Vector3::new(0.0, 0.0, 1.0),
                offset,
                (LINEAR_TOLERANCE, ANGULAR_TOLERANCE)
            ),
            Some(10)
        );
    }
}

#[test]
fn stored_caps_preserve_prior_targets_outside_recipe_references() {
    for (target, reversed, native) in [(0.0, false, true), (-10.8, true, false), (0.0, true, false)]
    {
        let (face, slots, losses) =
            run_configured(false, target, None, |feature, operand, before| {
                feature.evaluation.edit(|definition, _| {
                    let FeatureDefinition::Operation(FeatureOperation::Extrude {
                        start,
                        direction,
                        extent: ExtrudeExtent::OneSided { side },
                        ..
                    }) = definition
                    else {
                        panic!("Extrude")
                    };
                    *start = ExtrudeStart::ProfilePlane {};
                    *direction = if reversed {
                        ExtrudeDirection::ReversedProfileNormal {}
                    } else {
                        ExtrudeDirection::ProfileNormal {}
                    };
                    let LinearTermination::ToFace { face, offset } = &mut side.termination else {
                        panic!("face")
                    };
                    *offset = None;
                    if native {
                        *face = FaceSelection::Native("f3d:test:group#1448".into());
                    }
                });
                before
                    .surface_planes
                    .iter_mut()
                    .find(|plane| plane.surface == 13465)
                    .unwrap()
                    .origin = Point3::new(0.0, 0.0, target);
                operand
                    .preceding_candidate_faces
                    .push(crate::ids::brep_face_id(3484));
                operand.changed_candidate_faces = vec![crate::ids::brep_face_id(3465)];
            });
        assert!(matches!(face, FaceSelection::Historical { .. }));
        assert_eq!(selection_slots(&face), Some(vec![3465]));
        assert_eq!(slots, [3465]);
        assert!(losses.is_empty());
    }
}

#[test]
fn a_cap_proven_selection_survives_a_second_cap_proven_predecessor() {
    let (face, slots, losses) = run_configured(false, 0.0, None, |feature, operand, before| {
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                start,
                extent: ExtrudeExtent::OneSided { side },
                ..
            }) = definition
            else {
                panic!("Extrude")
            };
            *start = ExtrudeStart::ProfilePlane {};
            let LinearTermination::ToFace { offset, .. } = &mut side.termination else {
                panic!("face")
            };
            *offset = None;
        });
        before
            .surface_planes
            .iter_mut()
            .find(|plane| plane.surface == 13484)
            .unwrap()
            .origin = Point3::new(0.0, 0.0, 0.0);
        operand.preceding_candidate_faces = vec![crate::ids::brep_face_id(3484)];
    });
    assert_eq!(selection_slots(&face), Some(vec![3465]));
    assert_eq!(slots, [3465]);
    assert!(losses.is_empty());
}

#[test]
fn termination_binding_preserves_agreeing_recipe_candidates() {
    let (_, _, scope, group, mut operand) = fixture();
    operand.recipe_references[0].candidate_faces = operand.preceding_candidate_faces.clone();
    let mut selection = FaceSelection::Native(group.id.clone());
    super::super::selection::bind_face_selection(
        &mut selection,
        &scope,
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[],
    );
    assert_eq!(selection_slots(&selection), Some(vec![3465]));
}

#[test]
fn a_start_cap_does_not_prove_a_target_when_a_separate_terminal_cap_exists() {
    let (face, slots, losses) = run_configured(false, 0.0, Some(20.0), |feature, _, before| {
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                start,
                extent: ExtrudeExtent::OneSided { side },
                ..
            }) = definition
            else {
                panic!("Extrude")
            };
            *start = ExtrudeStart::ProfilePlane {};
            let LinearTermination::ToFace { offset, .. } = &mut side.termination else {
                panic!("face")
            };
            *offset = None;
        });
        before
            .surface_planes
            .iter_mut()
            .find(|plane| plane.surface == 13484)
            .unwrap()
            .origin = Point3::new(0.0, 0.0, 30.0);
    });
    assert_eq!(face, FaceSelection::Native("f3d:test:group#1448".into()));
    assert!(slots.is_empty());
    assert_eq!(losses.len(), 1);
}

#[test]
fn a_lateral_plane_with_its_origin_on_a_cap_is_not_a_cap_match() {
    let (face, slots, losses) = run_configured(false, 0.0, None, |feature, _, before| {
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                start,
                extent: ExtrudeExtent::OneSided { side },
                ..
            }) = definition
            else {
                panic!("Extrude")
            };
            *start = ExtrudeStart::ProfilePlane {};
            let LinearTermination::ToFace { offset, .. } = &mut side.termination else {
                panic!("face")
            };
            *offset = None;
        });
        before
            .surface_planes
            .iter_mut()
            .find(|plane| plane.surface == 13465)
            .unwrap()
            .normal = Vector3::new(1.0, 0.0, 0.0);
    });
    assert_eq!(face, FaceSelection::Native("f3d:test:group#1448".into()));
    assert!(slots.is_empty());
    assert_eq!(losses.len(), 1);
}
