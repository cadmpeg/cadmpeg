// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;
use crate::records::topology::DesignConstructionOperandGroupFrame;
use crate::records::topology::DesignOperandRole;

#[test]
fn edge_treatments_and_holes_project_typed_dimensions_and_native_selections() {
    use cadmpeg_ir::features::{ChamferGroup, ChamferSpec, EdgeSelection, RadiusSpec};

    let parameter = |owner_record_index,
                     record_index,
                     source_kind: &str,
                     name: &str,
                     expression: &str,
                     value| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(owner_record_index),
            expression,
            source_kind,
            Some("mm"),
            name,
            value,
        ))
        .expect("generated feature parameter is canonical");
        parameter.id = format!("f3d:native/BulkStream.dat:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, scope_record_index, parameter_record_index, local_ordinal| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .expect("generated parameter owner is canonical")
            .into_record("Design/BulkStream.dat", 0)
            .unwrap();
        {
            let mut wire = crate::records::DesignParameterOwnerWire::from(owner.clone());
            wire.id = format!("f3d:native/BulkStream.dat:owner#{record_index}");
            wire.record_index = record_index;
            wire.scope_record_index = scope_record_index;
            wire.parameter_record_index = parameter_record_index;
            wire.companion_record_index = parameter_record_index + 1;
            wire.local_ordinal = local_ordinal;
            owner = crate::records::DesignParameterOwner::try_from(wire).unwrap();
        }
        owner
    };
    let scope = |record_index, byte_offset, kind: &str| {
        DesignParameterScope::try_new(
            crate::records::feature::DesignParameterScopeDraft {
                id: format!("f3d:native/BulkStream.dat:scope#{record_index}"),
                byte_offset,
                class_tag: crate::records::DesignClassTag::try_from("301".to_owned()).unwrap(),
                record_index,
                frame_length: 200,
                kind_offset: byte_offset + 100,
                feature_ordinal: std::num::NonZeroU32::MIN,
                feature_ordinal_offset: 0,
                history_state_id: None,

                previous_history_state_id: None,
                previous_history_state_id_offset: None,
                reference_count_offset: byte_offset + 80,
                reference_members: crate::records::ReferenceRun::from_columns(
                    vec![record_index + 1],
                    vec![byte_offset + 85],
                    "reference_members",
                )
                .unwrap(),
                payload: crate::records::feature::DesignFeatureKind::try_from(kind.to_owned())
                    .expect("nonempty family name")
                    .try_into()
                    .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned())
                    .unwrap(),
                paired_byte_offset: byte_offset + 200,
            }
            .with_fixture_layout(),
        )
        .unwrap()
    };
    let mut scopes = vec![
        scope(12, 100, "Fillet"),
        scope(22, 400, "Chamfer"),
        scope(32, 700, "Hole"),
    ];
    if let crate::records::feature::DesignScopePayloadMut::Hole(slot) = scopes[2].payload_mut() {
        *slot = Some(DesignHoleConstruction {
            point_record_index: 378,
            point_record_byte_offset: 10,
            position: [1.25, -2.5, 3.75],
            position_offset: 35,
            direction: [0.0, 0.0, 1.0],
            direction_offset: 59,
            point_parameters: [0.125, -0.25],
            point_parameter_offsets: [83, 91],
            reference_type: 19,
            reference_type_offset: 99,
            tangent_point_data: Some(crate::records::feature::DesignHoleTangentPoint {
                prefix: 0,
                data: crate::records::Located {
                    value: [-1.0, -1.0, -1.0],
                    offset: 104,
                },
            }),
            input_records: vec![crate::records::Located {
                value: 378,
                offset: 129,
            }],
            face_selection: None,
        });
    }
    scopes[2]
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![0, 363, 0, 370, 0, 378]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let hole_face_operand = |record_index, scope_reference_ordinal| {
        DesignFaceOperand::try_new(crate::records::topology::DesignFaceOperandDraft {
            id: format!("f3d:native/BulkStream.dat:face-operand#{record_index}"),
            scope_record_index: 32,
            scope_reference_ordinal,
            group: None,
            record_index,
            byte_offset: 1200,
            class_tag: crate::records::DesignClassTag::try_from("297".to_owned()).unwrap(),
            paired_byte_offset: 1250,
            paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned()).unwrap(),
            recipe_record_index: record_index + 3,
            recipe_record_byte_offset: 1300,
            recipe_id: format!(
                "f3d:native/BulkStream.dat:construction-recipe#{}",
                record_index + 3
            ),
            recipe_prefix_offset: 1311,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::BoundedFace,
            recipe_program_offset: 1350,
            recipe_program: vec![0, -1],

            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: vec![282],
            resolved_active_face: None,
            next_record_index: record_index + 4,
            next_byte_offset: 1411,
        })
        .unwrap()
    };
    let hole_face_operands = [hole_face_operand(370, 3), hole_face_operand(378, 5)];
    let (features, _) = project_parameter_design(
        &[
            parameter(44, 45, "Radius", "d1", "5 mm", 0.5),
            parameter(54, 55, "Distance 1", "d2", "1 mm", 0.1),
            parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
        ],
        &[
            owner(44, 12, 45, 0),
            owner(54, 22, 55, 0),
            owner(64, 22, 65, 1),
        ],
        &scopes,
        &[],
        &[],
        &[],
        &[],
        &[],
    );

    let fillet = features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Fillet"))
        .expect("typed fillet");
    let FeatureDefinition::Fillet { groups } = fillet.evaluation.definition() else {
        panic!("expected typed fillet");
    };
    assert!(matches!(
        groups.as_slice(),
        [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Native(selection),
            radius: RadiusSpec::Constant { radius },
            tangency_weight: None,
        }] if selection == &scopes[0].id && radius.get() == 5.0
    ));
    let chamfer = features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Chamfer"))
        .expect("typed chamfer");
    assert!(matches!(
        chamfer.evaluation.definition(),
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                edges: EdgeSelection::Native(selection),
                spec: ChamferSpec::TwoDistances { first, second },
            }] if selection == &scopes[1].id && first.get() == 1.0 && second.get() == 2.0)
    ));

    let mut distance_angle_parameters = [
        parameter(54, 55, "Distance", "d2", "1.6 mm", 0.16),
        parameter(
            64,
            65,
            "Rotate Angle",
            "d3",
            "25 deg",
            25.0_f64.to_radians(),
        ),
    ];
    distance_angle_parameters[1]
        .try_set_unit_value("deg".to_owned())
        .unwrap();
    let (features, _) = project_parameter_design(
        &distance_angle_parameters,
        &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::DistanceAngle { distance, angle },
                ..
            }] if distance.get() == 1.6 && angle.get() == 25.0_f64.to_radians())
    ));

    distance_angle_parameters[0]
        .try_set_source(
            crate::records::DesignParameterSource::new(
                "leftDistance".into(),
                distance_angle_parameters[0].owner_record_index(),
                distance_angle_parameters[0].family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    distance_angle_parameters[1]
        .try_set_source(
            crate::records::DesignParameterSource::new(
                "rotateAngle".into(),
                distance_angle_parameters[1].owner_record_index(),
                distance_angle_parameters[1].family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    let (features, _) = project_parameter_design(
        &distance_angle_parameters,
        &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::DistanceAngle { distance, angle },
                ..
            }] if distance.get() == 1.6 && angle.get() == 25.0_f64.to_radians())
    ));

    let mut hole_parameters = [
        parameter(94, 95, "HoleDepth", "d4", "10 mm", 1.0),
        parameter(104, 105, "HoleDiameter", "d5", "4 mm", 0.4),
        parameter(114, 115, "TipAngle", "d6", "180 deg", std::f64::consts::PI),
    ];
    hole_parameters[2]
        .try_set_unit_value("deg".to_owned())
        .unwrap();
    let (features, _) = project_parameter_design(
        &hole_parameters,
        &[
            owner(94, 32, 95, 0),
            owner(104, 32, 105, 1),
            owner(114, 32, 115, 2),
        ],
        std::slice::from_ref(&scopes[2]),
        &[],
        &[],
        &[],
        &hole_face_operands,
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Hole {
            face: Some(FaceSelection::Resolved { faces, native }),
            placements: Some(placements),
            shape,

            extent: Some(cadmpeg_ir::features::LinearTermination::Blind { length: actual_length }),
            bottom: Some(cadmpeg_ir::features::HoleBottom::Flat),
            ..
        } if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Simple,
                ..
            }, Some(actual_diameter),) if (faces == &vec![FaceId::mint(crate::ids::brep_entity_id(282)).expect("identity grammar")]
            && native == &scopes[2].id
            && placements == &vec![cadmpeg_ir::features::HolePlacement::Directed {
                position: cadmpeg_ir::features::FinitePoint3::new(Point3 { x: 12.5, y: -25.0, z: 37.5 }).unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3 { x: 0.0, y: 0.0, z: 1.0 }).unwrap(),
            }]) && actual_diameter.get() == 4.0 && actual_length.get() == 10.0)));

    hole_parameters[2]
        .try_set_evaluated_value(118.0_f64.to_radians())
        .unwrap();
    let (features, _) = project_parameter_design(
        &hole_parameters,
        &[
            owner(94, 32, 95, 0),
            owner(104, 32, 105, 1),
            owner(114, 32, 115, 2),
        ],
        std::slice::from_ref(&scopes[2]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Hole {
            shape,
            bottom: None,
            ..
        } if matches!((shape.construction(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::SimpleDrilled { drill_point_angle },
                ..
            },) if drill_point_angle.get() == 118.0_f64.to_radians())));

    let mut counterbore_parameters = hole_parameters.to_vec();
    counterbore_parameters.extend([
        parameter(124, 125, "CBDepth", "d7", "3 mm", 0.3),
        parameter(134, 135, "CBDiameter", "d8", "8 mm", 0.8),
    ]);
    let (features, _) = project_parameter_design(
        &counterbore_parameters,
        &[
            owner(94, 32, 95, 0),
            owner(104, 32, 105, 1),
            owner(114, 32, 115, 2),
            owner(124, 32, 125, 3),
            owner(134, 32, 135, 4),
        ],
        std::slice::from_ref(&scopes[2]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Hole {
            shape,
            bottom: None,
            ..
        } if matches!((shape.construction(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::CounterboreDrilled {
                    diameter: actual_diameter,
                    depth: actual_depth,
                    drill_point_angle,
                },
                ..
            },) if (drill_point_angle.get() == 118.0_f64.to_radians()) && actual_diameter.get() == 8.0 && actual_depth.get() == 3.0)));

    counterbore_parameters[2]
        .try_set_evaluated_value(std::f64::consts::PI)
        .unwrap();
    let (features, _) = project_parameter_design(
        &counterbore_parameters,
        &[
            owner(94, 32, 95, 0),
            owner(104, 32, 105, 1),
            owner(114, 32, 115, 2),
            owner(124, 32, 125, 3),
            owner(134, 32, 135, 4),
        ],
        std::slice::from_ref(&scopes[2]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Hole {
            shape,
            bottom: Some(cadmpeg_ir::features::HoleBottom::Flat),
            ..
        } if matches!((shape.construction(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Counterbore {
                    diameter: actual_diameter,
                    depth: actual_depth,
                },
                ..
            },) if actual_diameter.get() == 8.0 && actual_depth.get() == 3.0)));

    let (features, _) = project_parameter_design(
        &[
            parameter(54, 55, "leftDistance", "d2", "1 mm", 0.1),
            parameter(64, 65, "rightDistance", "d3", "2 mm", 0.2),
        ],
        &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::TwoDistances { first, second },
                ..
            }] if first.get() == 1.0 && second.get() == 2.0)
    ));

    let (features, _) = project_parameter_design(
        &[parameter(54, 55, "leftDistance", "d2", "1 mm", 0.1)],
        &[owner(54, 22, 55, 0)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::Distance { distance },
                ..
            }] if distance.get() == 1.0)
    ));

    let (features, _) = project_parameter_design(
        &[
            parameter(44, 45, "Radius", "d1", "5 mm", 0.5),
            parameter(46, 47, "TangencyWeight", "w1", "0.5", 0.5),
        ],
        &[owner(44, 12, 45, 0), owner(46, 12, 47, 1)],
        std::slice::from_ref(&scopes[0]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Fillet,
            parameters,
        } if parameters.len() == 2
    ));

    let (features, _) = project_parameter_design(
        &[parameter(44, 45, "Radius", "d1", "0 mm", 0.0)],
        &[owner(44, 12, 45, 0)],
        std::slice::from_ref(&scopes[0]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Fillet,
            parameters,
        } if parameters.len() == 1
    ));

    let (features, _) = project_parameter_design(
        &[
            parameter(54, 55, "Distance 1", "d2", "1 mm", 0.1),
            parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
            parameter(74, 75, "Distance", "d4", "3 mm", 0.3),
        ],
        &[
            owner(54, 22, 55, 0),
            owner(64, 22, 65, 1),
            owner(74, 22, 75, 2),
        ],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Chamfer,
            parameters,
        } if parameters.len() == 3
    ));

    let (features, _) = project_parameter_design(
        &[
            parameter(54, 55, "Distance 1", "d2", "0 mm", 0.0),
            parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
        ],
        &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
        std::slice::from_ref(&scopes[1]),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Chamfer,
            parameters,
        } if parameters.len() == 2
    ));

    let construction_group = |record_index, scope_reference_ordinal| {
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::DesignConstructionOperandGroupDraft {
                id: format!("f3d:native/BulkStream.dat:construction-group#{record_index}"),
                scope_record_index: 22,
                scope_reference_ordinal,
                record_index,
                byte_offset: 1_000 + u64::from(scope_reference_ordinal),
                class_tag: crate::records::DesignClassTag::try_from("288".to_owned()).unwrap(),
                members: vec![crate::records::Located {
                    value: record_index + 100,
                    offset: 1_026 + u64::from(scope_reference_ordinal),
                }],
                lost_edge_references: Vec::new(),
                frame: DesignConstructionOperandGroupFrame::try_from(
                    crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: 1_021 + u64::from(scope_reference_ordinal),
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: vec![crate::records::Located {
                            value: record_index + 1,
                            offset: 1_050 + u64::from(scope_reference_ordinal),
                        }],
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 100,
                        opaque_index_offset: 1_078 + u64::from(scope_reference_ordinal),
                        opaque_scalar: 0.5,
                        opaque_scalar_offset: 1_082 + u64::from(scope_reference_ordinal),
                        variant: false,
                    },
                )
                .unwrap(),
                operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
                role_offset: 1_060 + u64::from(scope_reference_ordinal),
                paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned())
                    .unwrap(),
                paired_byte_offset: 1_100 + u64::from(scope_reference_ordinal),
            },
        )
        .unwrap()
    };
    let mut construction_groups = [construction_group(90, 17), construction_group(80, 4)];
    construction_groups[1]
        .lost_edge_references
        .push("f3d:native/BulkStream.dat:lost-edge-reference#1".into());
    let mut chamfer_scope = scopes[1].clone();
    chamfer_scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(21);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (features, _) = project_parameter_design(
        &[
            parameter(74, 75, "Distance", "d5", "2 mm", 0.2),
            parameter(84, 85, "Distance", "d4", "2.5 mm", 0.25),
        ],
        &[owner(74, 22, 75, 1), owner(84, 22, 85, 0)],
        std::slice::from_ref(&chamfer_scope),
        &construction_groups,
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [
                ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Distance { distance: actual_distance },
                },
                ChamferGroup {
                    edges: EdgeSelection::Native(selection),
                    spec: ChamferSpec::Distance { distance: actual_distance_2 },
                },
            ] if (selection == &construction_groups[0].id) && actual_distance.get() == 2.5 && actual_distance_2.get() == 2.0)
    ));
}

#[test]
fn draft_entity_neutral_selection_projects_a_unique_historical_face() {
    use crate::records::feature::{DesignDraftOperation, DesignParameterScope};
    use crate::records::topology::{
        AsmHistoricalEntityKind, DesignConstructionOperandGroup,
        DesignConstructionOperandGroupFrame, DesignEntitySelectionFaceCandidate,
        DesignEntitySelectionOperand,
    };
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let stream = "f3d:test/Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::DesignFeatureKind::Draft,
        100,
    );
    scope.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![101, 111, 102, 112]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::DesignScopePayloadMut::Draft(slot) = scope.payload_mut() {
        *slot = Some(DesignDraftOperation {
            angle: crate::records::feature::DesignFiniteScalar::new(-0.25).unwrap(),
            angle_record_index: 90,
            angle_offset: 0,
            opposite_angle_record_index: 91,
            opposite_angle_offset: 0,
        });
    }
    let group = |record_index, member, role| {
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::DesignConstructionOperandGroupDraft {
                id: format!("{stream}:design-construction-operand-group#{record_index}"),
                scope_record_index: 100,
                scope_reference_ordinal: 0,
                record_index,
                byte_offset: 0,
                class_tag: crate::records::DesignClassTag::try_from("000".to_owned()).unwrap(),
                members: vec![crate::records::Located {
                    value: member,
                    offset: 0,
                }],
                lost_edge_references: Vec::new(),
                frame: DesignConstructionOperandGroupFrame::try_from(
                    crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: 0,
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: Vec::new(),
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 1,
                        opaque_index_offset: 18,
                        opaque_scalar: 0.0,
                        opaque_scalar_offset: 22,
                        variant: false,
                    },
                )
                .unwrap(),
                operand_role: crate::records::topology::DesignConstructionOperandRole::Other(role),
                role_offset: 0,
                paired_class_tag: crate::records::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                paired_byte_offset: 0,
            },
        )
        .unwrap()
    };
    let groups = [
        group(101, 111, DesignOperandRole::ROLE_0X10),
        group(102, 112, DesignOperandRole::ROLE_0X21),
    ];
    let mut selection = DesignEntitySelectionOperand::try_new(
        crate::records::topology::DesignEntitySelectionOperandDraft {
            id: format!("{stream}:design-entity-selection-operand#112"),
            scope_record_index: 100,
            group_record_index: 102,
            group_member_ordinal: 0,
            record_index: 112,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("000".to_owned()).unwrap(),
            asset_id: crate::records::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 0,
            context_id: crate::records::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 0,
            identity_record_index: 115,
            identity_record_offset: 0,
            primary_identity: 225,
            primary_identity_offset: 21,
            secondary: None,
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: vec![DesignEntitySelectionFaceCandidate {
                history_id: "history".into(),
                historical: crate::records::topology::HistoricalBinding {
                    kind: AsmHistoricalEntityKind::Coedge,
                    entity_ref: 225,
                    state_ids: vec![7, 6],
                },
                face_slot: 158,
            }],
            resolved_edge_slot: None,
            next_record_index: 114,
            next_byte_offset: 29,
        },
    )
    .unwrap();
    let timeline = crate::records::DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::DesignTimelineFrame::test_items(
            0,
            vec![crate::records::Located {
                value: 100,
                offset: 0,
            }],
        ),
        crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let project = |selection: &DesignEntitySelectionOperand| {
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            &crate::design::feature_project::ProjectInputs {
                native: &[],
                owners: &[],
                scopes: std::slice::from_ref(&scope),
                timelines: std::slice::from_ref(&timeline),
                construction_groups: &groups,
                fillet_radius_groups: &[],
                edge_operands: &[],
                edge_identity_operands: &[],
                edge_treatment_vertex_operands: &[],
                entity_selection_operands: std::slice::from_ref(selection),
                curve_identities: &[],
                face_operands: &[],
                body_recipe_operands: &[],
                legacy_loft_body_carriers: &[],
                placements: &[],
                body_bindings: &[],
                component_naming_spaces: &[],
                histories: &[],
            },
        )
        .expect("authored Draft timeline")
    };

    let (features, _) = project(&selection);
    let FeatureDefinition::Draft {
        anchor:
            cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: neutral_plane,
                pull,
            },
        angle,
        outward,
        ..
    } = features[0].evaluation.definition()
    else {
        panic!("expected a typed Draft definition");
    };
    assert_eq!(angle.as_ref().map(|angle| angle.get()), Some(-0.25));
    assert_eq!(*outward, Some(true));
    assert!(pull.is_none());
    let FaceSelection::Historical {
        state,
        faces,
        native,
    } = neutral_plane
    else {
        panic!("expected a historical neutral face selection");
    };
    let feature = crate::ids::neutral_feature_id(&scope);
    let feature_key = feature
        .as_str()
        .split_once('#')
        .map_or(feature.as_str(), |(_, key)| key);
    let prefix = crate::ids::history_input_prefix(feature_key, 7);
    assert_eq!(
        state,
        &crate::design::edge_resolve::feature_input_topology_id(&feature, 7)
    );
    assert_eq!(
        faces.as_slice(),
        &[crate::ids::history_input_face_id(&prefix, 158)]
    );
    assert_eq!(native.as_str(), &groups[1].id);

    selection
        .historical_face_candidates
        .push(DesignEntitySelectionFaceCandidate {
            history_id: "other-history".into(),
            historical: crate::records::topology::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 225,
                state_ids: vec![7],
            },
            face_slot: 159,
        });
    let (features, _) = project(&selection);
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Draft,
            ..
        }
    ));
}

#[test]
fn draft_outward_is_derived_from_the_signed_angle() {
    assert!(crate::design::feature_project::draft_outward(-0.25));
    assert!(!crate::design::feature_project::draft_outward(0.0));
    assert!(!crate::design::feature_project::draft_outward(0.25));
}

#[test]
fn variable_fillet_law_orders_endpoint_and_midpoint_parameters() {
    use cadmpeg_ir::features::Length;

    let parameter = |record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(record_index + 100),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("variable Fillet parameter");
        parameter.record_index = record_index;
        parameter
    };
    let start = parameter(1, "StartRadius", Some("mm"), 0.0);
    let end = parameter(2, "EndRadius", Some("mm"), 0.0);
    let radius = parameter(3, "MidRadius", Some("mm"), 0.4);
    let position = parameter(4, "MidParams", None, 0.25);
    let weight = parameter(5, "TangencyWeight", None, 0.75);
    let (points, tangency_weight) = crate::design::feature_project::variable_fillet_law(&[
        (0, &start),
        (1, &end),
        (2, &radius),
        (3, &position),
        (4, &weight),
    ])
    .expect("complete variable Fillet law");
    assert_eq!(
        points.as_slice(),
        [
            cadmpeg_ir::features::VariableRadius {
                parameter: 0.0,
                radius: Length::ZERO,
            },
            cadmpeg_ir::features::VariableRadius {
                parameter: 0.25,
                radius: Length::new(4.0).unwrap(),
            },
            cadmpeg_ir::features::VariableRadius {
                parameter: 1.0,
                radius: Length::ZERO,
            },
        ]
    );
    assert_eq!(
        tangency_weight.map(cadmpeg_ir::features::FiniteReal::get),
        Some(0.75)
    );
}

#[test]
fn variable_fillet_law_accepts_omitted_tangency_weight() {
    use cadmpeg_ir::features::Length;

    let parameter = |record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(record_index + 100),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("variable Fillet parameter");
        parameter.record_index = record_index;
        parameter
    };
    let start = parameter(1, "StartRadius", Some("mm"), 0.2);
    let end = parameter(2, "EndRadius", Some("mm"), 0.4);
    let (points, tangency_weight) =
        crate::design::feature_project::variable_fillet_law(&[(0, &start), (1, &end)])
            .expect("variable Fillet law without an explicit weight");
    assert_eq!(
        points.as_slice(),
        [
            cadmpeg_ir::features::VariableRadius {
                parameter: 0.0,
                radius: Length::new(2.0).unwrap(),
            },
            cadmpeg_ir::features::VariableRadius {
                parameter: 1.0,
                radius: Length::new(4.0).unwrap(),
            },
        ]
    );
    assert_eq!(tangency_weight, None);
}

#[test]
fn variable_fillet_law_rejects_duplicate_tangency_weights() {
    let parameter = |record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(record_index + 100),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("variable Fillet parameter");
        parameter.record_index = record_index;
        parameter
    };
    let start = parameter(1, "StartRadius", Some("mm"), 0.2);
    let end = parameter(2, "EndRadius", Some("mm"), 0.4);
    let weight_one = parameter(3, "TangencyWeight", None, 0.5);
    let weight_two = parameter(4, "TangencyWeight", None, 0.75);
    assert!(crate::design::feature_project::variable_fillet_law(&[
        (0, &start),
        (1, &end),
        (2, &weight_one),
        (3, &weight_two),
    ])
    .is_none());
}

#[test]
fn localized_fillet_radius_parameters_pair_with_counted_edge_groups_in_order() {
    let scope = DesignParameterScope::try_new(
        crate::records::feature::DesignParameterScopeDraft {
            id: "f3d:native/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: crate::records::DesignClassTag::try_from("301".to_owned()).unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,

            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::ReferenceRun::from_columns(
                vec![100, 101],
                vec![185, 196],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::DesignFeatureKind::Conge
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let group = |record_index, ordinal, members: Vec<u32>| {
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::DesignConstructionOperandGroupDraft {
                id: format!("f3d:native/BulkStream.dat:construction-group#{record_index}"),
                scope_record_index: 12,
                scope_reference_ordinal: ordinal,
                record_index,
                byte_offset: 1000 + u64::from(ordinal) * 200,
                class_tag: crate::records::DesignClassTag::try_from("288".to_owned()).unwrap(),

                members: members
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| crate::records::Located {
                        value,
                        offset: 1026 + u64::from(ordinal) * 200 + index as u64 * 11,
                    })
                    .collect(),
                lost_edge_references: Vec::new(),
                frame: DesignConstructionOperandGroupFrame::try_from(
                    crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: 1021 + u64::from(ordinal) * 200,
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: vec![crate::records::Located {
                            value: 300 + ordinal,
                            offset: 1100 + u64::from(ordinal) * 200,
                        }],
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 100,
                        opaque_index_offset: 1128 + u64::from(ordinal) * 200,
                        opaque_scalar: 0.5,
                        opaque_scalar_offset: 1132 + u64::from(ordinal) * 200,
                        variant: false,
                    },
                )
                .unwrap(),
                operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
                role_offset: 1110 + u64::from(ordinal) * 200,

                paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned())
                    .unwrap(),
                paired_byte_offset: 1200 + u64::from(ordinal) * 200,
            },
        )
        .unwrap()
    };
    let mut operand_groups = [group(100, 0, vec![200]), group(101, 1, vec![201, 202])];
    let parameter = |owner_index, record_index, source_kind: &str, unit, value| {
        let mut parameter = parse_design_parameter(&parameter_record(
            Some(owner_index),
            "value",
            source_kind,
            unit,
            "d1",
            value,
        ))
        .expect("canonical localized Fillet parameter");
        parameter.id = format!("f3d:native/BulkStream.dat:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter
    };
    let owner = |record_index, parameter_record_index, local_ordinal| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .unwrap()
            .into_record("Design/BulkStream.dat", 0)
            .unwrap();
        {
            let mut wire = crate::records::DesignParameterOwnerWire::from(owner.clone());
            wire.id = format!("f3d:native/BulkStream.dat:owner#{record_index}");
            wire.record_index = record_index;
            wire.scope_record_index = 12;
            wire.parameter_record_index = parameter_record_index;
            wire.companion_record_index = parameter_record_index + 1;
            wire.local_ordinal = local_ordinal;
            owner = crate::records::DesignParameterOwner::try_from(wire).unwrap();
        }
        owner
    };
    let parameters = [
        parameter(10, 11, "Radius", Some("mm"), 0.5),
        parameter(20, 21, "Radius", Some("mm"), 0.3),
        parameter(30, 31, "TangencyWeight", None, 1.0),
        parameter(40, 41, "TangencyWeight", None, 0.75),
    ];
    let owners = [
        owner(10, 11, 0),
        owner(20, 21, 1),
        owner(30, 31, 2),
        owner(40, 41, 3),
    ];
    let mut indexed_scope = scope.clone();
    if let crate::records::feature::DesignScopePayloadMut::Fillet(slot)
    | crate::records::feature::DesignScopePayloadMut::Conge(slot)
    | crate::records::feature::DesignScopePayloadMut::Abrundung(slot)
    | crate::records::feature::DesignScopePayloadMut::Arredondamento(slot) =
        indexed_scope.payload_mut()
    {
        *slot = Some(crate::records::feature::DesignFixedFilletParameters {
            groups: vec![crate::records::feature::DesignFixedFilletGroup::try_new(
                Some(crate::records::feature::DesignFixedFilletScalar {
                    value: 1.0,
                    record_index: 10,
                    value_offset: 100,
                }),
                crate::records::feature::DesignFixedFilletLaw::Constant(
                    crate::records::feature::DesignFixedFilletScalar {
                        value: 0.5,
                        record_index: 20,
                        value_offset: 200,
                    },
                ),
            )
            .unwrap()],
        });
    }
    crate::design::decode::operands::disambiguate_fixed_fillet_parameters(
        std::slice::from_mut(&mut indexed_scope),
        &owners,
    );
    assert_eq!(indexed_scope.fixed_fillet_parameters(), None);

    let assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups,
        &owners,
        &parameters,
    );
    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0].edge_operand_record_indices, [200]);
    assert_eq!(
        assignments[0].law,
        crate::records::topology::DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 11,
        }
    );
    assert_eq!(
        assignments[0].tangency_weight_parameter_record_index,
        Some(31)
    );
    assert_eq!(assignments[1].edge_operand_record_indices, [201, 202]);
    assert_eq!(
        assignments[1].law,
        crate::records::topology::DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 21,
        }
    );
    assert_eq!(
        assignments[1].tangency_weight_parameter_record_index,
        Some(41)
    );
    let variable_parameters = [
        parameter(50, 51, "StartRadius", Some("mm"), 0.2),
        parameter(60, 61, "EndRadius", Some("mm"), 0.6),
        parameter(70, 71, "MidRadius", Some("mm"), 0.4),
        parameter(80, 81, "MidParams", None, 0.25),
        parameter(90, 91, "TangencyWeight", None, 0.75),
    ];
    let variable_owners = [
        owner(50, 51, 0),
        owner(60, 61, 1),
        owner(70, 71, 2),
        owner(80, 81, 3),
        owner(90, 91, 4),
    ];
    let variable_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &variable_owners,
        &variable_parameters,
    );
    assert_eq!(variable_assignments.len(), 1);
    assert_eq!(
        variable_assignments[0].law,
        crate::records::topology::DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index: 51,
            end_radius_parameter_record_index: 61,
            middle: vec![crate::records::topology::DesignFilletMidpoint {
                radius_parameter_record_index: 71,
                parameter_record_index: 81
            }],
        }
    );
    assert_eq!(
        variable_assignments[0].tangency_weight_parameter_record_index,
        Some(91)
    );
    let (variable_features, _) = project_parameter_design(
        &variable_parameters,
        &variable_owners,
        std::slice::from_ref(&scope),
        &operand_groups[1..],
        &variable_assignments,
        &[],
        &[],
        &[],
    );
    let FeatureDefinition::Fillet { groups } = variable_features[0].evaluation.definition() else {
        panic!("expected assigned variable Fillet");
    };
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].edges,
        cadmpeg_ir::features::EdgeSelection::Native(variable_assignments[0].id.clone())
    );
    assert_eq!(
        groups[0]
            .tangency_weight
            .map(cadmpeg_ir::features::FiniteReal::get),
        Some(0.75)
    );
    assert!(matches!(
        &groups[0].radius,
        cadmpeg_ir::features::RadiusSpec::Variable { points } if points.as_slice().len() == 3
    ));
    let cadmpeg_ir::features::RadiusSpec::Variable { points } = &groups[0].radius else {
        panic!("expected assigned variable radius controls");
    };
    assert_eq!(
        points
            .as_slice()
            .iter()
            .map(|point| (point.parameter, point.radius.get()))
            .collect::<Vec<_>>(),
        vec![(0.0, 2.0), (0.25, 4.0), (1.0, 6.0)]
    );
    let (unassigned_features, _) = project_parameter_design(
        &variable_parameters,
        &variable_owners,
        std::slice::from_ref(&scope),
        &operand_groups[1..],
        &[],
        &[],
        &[],
        &[],
    );
    let FeatureDefinition::Fillet {
        groups: unassigned_groups,
    } = unassigned_features[0].evaluation.definition()
    else {
        panic!("expected unassigned variable Fillet");
    };
    assert_eq!(unassigned_groups.len(), 1);
    assert_eq!(
        unassigned_groups[0].edges,
        cadmpeg_ir::features::EdgeSelection::Native(operand_groups[1].id.clone())
    );
    assert_eq!(unassigned_groups[0].radius, groups[0].radius);
    assert_eq!(
        unassigned_groups[0].tangency_weight,
        groups[0].tangency_weight
    );
    let variable_without_weight_parameters = [
        parameter(50, 51, "StartRadius", Some("mm"), 0.2),
        parameter(60, 61, "EndRadius", Some("mm"), 0.6),
        parameter(70, 71, "MidRadius", Some("mm"), 0.4),
        parameter(80, 81, "MidParams", None, 0.25),
    ];
    let variable_without_weight_owners = [
        owner(50, 51, 0),
        owner(60, 61, 1),
        owner(70, 71, 2),
        owner(80, 81, 3),
    ];
    let variable_without_weight_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &variable_without_weight_owners,
        &variable_without_weight_parameters,
    );
    assert_eq!(variable_without_weight_assignments.len(), 1);
    assert_eq!(
        variable_without_weight_assignments[0].tangency_weight_parameter_record_index,
        None
    );
    let mut incomplete_parameters = variable_parameters.to_vec();
    incomplete_parameters.push(parameter(100, 101, "UnknownLawInput", None, 1.0));
    let mut incomplete_owners = variable_owners.to_vec();
    incomplete_owners.push(owner(100, 101, 5));
    assert!(decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &incomplete_owners,
        &incomplete_parameters,
    )
    .is_empty());
    let chord_parameters = [
        parameter(110, 111, "TangencyWeight", None, 1.0),
        parameter(120, 121, "ChordLen", Some("in"), 0.25),
    ];
    let chord_owners = [owner(110, 111, 0), owner(120, 121, 1)];
    let chord_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &chord_owners,
        &chord_parameters,
    );
    assert_eq!(chord_assignments.len(), 1);
    assert_eq!(
        chord_assignments[0].law,
        crate::records::topology::DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index: 121,
        }
    );
    let (chord_features, _) = project_parameter_design(
        &chord_parameters,
        &chord_owners,
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &chord_assignments,
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        chord_features[0].evaluation.definition(),
        FeatureDefinition::Fillet { groups }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::FilletGroup {
                    radius: cadmpeg_ir::features::RadiusSpec::Chordal {
                        chord_length: actual_chord_length,
                    },
                    tangency_weight: Some(weight),
                    ..
                }] if weight.get() == 1.0 && actual_chord_length.get() == 2.5
            )
    ));
    let chord_only_parameters = [parameter(120, 121, "ChordLen", Some("in"), 0.25)];
    let chord_only_owners = [owner(120, 121, 0)];
    let chord_only_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &chord_only_owners,
        &chord_only_parameters,
    );
    assert_eq!(chord_only_assignments.len(), 1);
    assert_eq!(
        chord_only_assignments[0].law,
        crate::records::topology::DesignFilletRadiusLaw::Chordal {
            chord_length_parameter_record_index: 121,
        }
    );
    assert_eq!(
        chord_only_assignments[0].tangency_weight_parameter_record_index,
        None
    );
    let (chord_only_features, _) = project_parameter_design(
        &chord_only_parameters,
        &chord_only_owners,
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &chord_only_assignments,
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        chord_only_features[0].evaluation.definition(),
        FeatureDefinition::Fillet { groups }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::FilletGroup {
                    radius: cadmpeg_ir::features::RadiusSpec::Chordal {
                        chord_length: actual_chord_length,
                    },
                    tangency_weight: None,
                    ..
                }] if actual_chord_length.get() == 2.5
            )
    ));
    let asymmetric_parameters = [
        parameter(130, 131, "TangencyWeight", None, 1.0),
        parameter(140, 141, "EdgeOffset1", Some("mm"), 0.2),
        parameter(150, 151, "EdgeOffset2", Some("mm"), 0.7),
    ];
    let asymmetric_owners = [owner(130, 131, 0), owner(140, 141, 1), owner(150, 151, 2)];
    let asymmetric_assignments = decode_fillet_radius_groups(
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &asymmetric_owners,
        &asymmetric_parameters,
    );
    assert_eq!(asymmetric_assignments.len(), 1);
    assert_eq!(
        asymmetric_assignments[0].law,
        crate::records::topology::DesignFilletRadiusLaw::Asymmetric {
            offset_one_parameter_record_index: 141,
            offset_two_parameter_record_index: 151,
        }
    );
    let (asymmetric_features, _) = project_parameter_design(
        &asymmetric_parameters,
        &asymmetric_owners,
        std::slice::from_ref(&scope),
        &operand_groups[..1],
        &asymmetric_assignments,
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        asymmetric_features[0].evaluation.definition(),
        FeatureDefinition::Fillet { groups }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::FilletGroup {
                    radius: cadmpeg_ir::features::RadiusSpec::Asymmetric {
                        offset_one: actual_offset_one,
                        offset_two: actual_offset_two,
                    },
                    tangency_weight: Some(weight),
                    ..
                }] if weight.get() == 1.0 && actual_offset_one.get() == 2.0 && actual_offset_two.get() == 7.0
            )
    ));
    operand_groups[0]
        .lost_edge_references
        .push("f3d:native/BulkStream.dat:lost-edge-reference#1".into());

    let (features, _) = project_parameter_design(
        &parameters,
        &owners,
        std::slice::from_ref(&scope),
        &operand_groups,
        &assignments,
        &[],
        &[],
        &[],
    );
    let FeatureDefinition::Fillet { groups } = features[0].evaluation.definition() else {
        panic!("expected typed localized Fillet");
    };
    assert_eq!(groups.len(), 2);
    assert!(matches!(
        &groups[0],
        cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: actual_radius,
            },
            tangency_weight: Some(weight),
        } if weight.get() == 1.0 && actual_radius.get() == 5.0
    ));
    assert!(matches!(
        &groups[1],
        cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: actual_radius,
            },
            tangency_weight: Some(weight),
        } if weight.get() == 0.75 && (selection == &operand_groups[1].id) && actual_radius.get() == 3.0
    ));

    let mut patch_scope = scope.clone();
    patch_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::SurfacePatch
                .try_into()
                .unwrap();
            draft.frame_length = 354;
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 300, 301]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let patch_boundary = |scope_reference_ordinal, record_index, model_reference| {
        crate::records::feature::DesignSurfacePatchBoundary {
            scope_reference_ordinal,
            record_index,
            is_seed_selection: false,
            continuity: crate::records::feature::DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference,
        }
    };
    if let crate::records::feature::DesignScopePayloadMut::SurfacePatch(slot) =
        patch_scope.payload_mut()
    {
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    let mut patch_group = group(100, 0, vec![200]);
    patch_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            support_faces: cadmpeg_ir::features::FaceSelection::Faces(ref faces),
            ref continuity,
            merge_result: Some(false),
        }) if matches!(continuity.resolved(), Some(
            cadmpeg_ir::features::FilledSurfaceContinuity::PerBoundary {
                first: cadmpeg_ir::features::SurfaceContinuity::Contact,
                rest,
            }
        ) if rest.is_empty()) && native == &patch_group.id && faces.is_empty()
    ));

    patch_scope
        .try_edit(|draft| {
            draft.frame_length = 398;
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 300, 101, 201, 301, 102]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::DesignScopePayloadMut::SurfacePatch(slot) =
        patch_scope.payload_mut()
    {
        *slot = vec![patch_boundary(2, 300, 100), patch_boundary(5, 301, 101)];
    }
    let mut second_patch_group = group(101, 3, vec![201]);
    second_patch_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            &[patch_group.clone(), second_patch_group.clone()],
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ..
        }) if native == &patch_scope.id
    ));

    patch_scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(8);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let edge_identity = |record_index, group_record_index, edge| {
        crate::records::topology::DesignEdgeIdentityOperand::try_new(
            crate::records::topology::DesignEdgeIdentityOperandDraft {
                id: format!("f3d:native/BulkStream.dat:edge-identity#{record_index}"),
                scope_record_index: patch_scope.record_index,
                group_record_index,
                group_member_ordinal: 0,
                record_index,
                byte_offset: 0,
                class_tag: crate::records::DesignClassTag::try_from("297".to_owned()).unwrap(),
                layout: crate::records::topology::DesignEdgeIdentityLayout::Full,
                local_id: u64::from(record_index),
                asset_id: crate::records::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 42,
                context_id: crate::records::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 118,
                historical: None,
                treatment_radius_candidates: Vec::new(),
                transition_edge_candidates: Vec::new(),
                resolved_edge_slots: Vec::new(),
                resolved_edge_slot: Some(edge),
                resolution_identity_id: None,
            },
        )
        .unwrap()
    };
    let identities = vec![edge_identity(200, 100, 17), edge_identity(201, 101, 18)];
    let resolved = crate::design::feature_project::project_surface_patch(
        &patch_scope,
        &[patch_group.clone(), second_patch_group],
        &[],
        &identities,
    )
    .expect("resolved multi-group SurfacePatch path");
    let FeatureDefinition::FilledSurface {
        boundary:
            cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::HistoricalEdges { edges, native, .. },
            ),
        ..
    } = resolved
    else {
        panic!("expected historical multi-group SurfacePatch path");
    };
    assert_eq!(edges.len(), 2);
    assert_eq!(native.as_str(), patch_scope.id);
    patch_scope
        .try_edit(|draft| {
            draft.previous_history_state_id = None;

            draft.frame_length = 339;
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![100, 200, 300]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::DesignScopePayloadMut::SurfacePatch(slot) =
        patch_scope.payload_mut()
    {
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    patch_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ..
        }) if native == &patch_group.id
    ));

    // The earlier scope-envelope generation is fourteen bytes shorter in both
    // forms and projects the same feature from the same reference shape.
    patch_scope
        .try_edit(|draft| {
            draft.frame_length = 325;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface { .. })
    ));
    patch_scope
        .try_edit(|draft| {
            draft.frame_length = 340;
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 300, 301]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::DesignScopePayloadMut::SurfacePatch(slot) =
        patch_scope.payload_mut()
    {
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    patch_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface { .. })
    ));
    patch_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 300, 301, 302]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(crate::design::feature_project::project_surface_patch(
        &patch_scope,
        std::slice::from_ref(&patch_group),
        &[],
        &[],
    )
    .is_none());

    patch_scope
        .try_edit(|draft| {
            draft.frame_length = 343;
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 201, 202, 203, 300]);
            draft.payload = crate::records::feature::DesignScopePayload::SurfacePatch(Vec::new());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    patch_group
        .try_set_members(
            vec![200, 201, 202, 203]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::Located {
                    value,
                    offset: index as u64 * 11,
                })
                .collect(),
        )
        .unwrap();
    let grouped_projection = crate::design::feature_project::project_surface_patch(
        &patch_scope,
        std::slice::from_ref(&patch_group),
        &[],
        &[],
    );
    assert!(matches!(
        grouped_projection,
        Some(FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(
                cadmpeg_ir::features::PathRef::Native(ref native)
            ),
            ref continuity,
            ..
        }) if continuity.uniform_value()
            == Some(cadmpeg_ir::features::SurfaceContinuity::Contact)
            && native == &patch_group.id
    ));

    let mut fill_scope = scope.clone();
    fill_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::BoundaryFill
                .try_into()
                .unwrap();
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 201, 300, 301, 400]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut tools = group(100, 0, vec![200, 201]);
    tools.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    let mut cell = group(300, 3, vec![301]);
    cell.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
    assert!(matches!(
        crate::design::feature_project::project_boundary_fill(&fill_scope, &[tools.clone(), cell.clone()]),
        Some(FeatureDefinition::BoundaryFill {
            tools: cadmpeg_ir::features::BodySelection::Native(ref tool_selection),
            cells: ref cell_selections,
        }) if tool_selection == &tools.id
            && cell_selections.as_slice() == [cadmpeg_ir::features::BodySelection::Native(cell.id)]
    ));
}

#[test]
fn fillet_projection_rejects_mistyped_assignment_records_without_panicking() {
    use crate::records::topology::{DesignFilletRadiusGroup, DesignFilletRadiusLaw};
    let mut weight = parse_design_parameter(&parameter_record(
        Some(1),
        "1",
        "TangencyWeight",
        None,
        "weight",
        1.0,
    ))
    .unwrap();
    weight.record_index = 11;
    let mut radius = parse_design_parameter(&parameter_record(
        Some(2),
        "1 mm",
        "Radius",
        Some("mm"),
        "radius",
        1.0,
    ))
    .unwrap();
    radius.record_index = 12;
    let scope = DesignParameterScope::empty(
        "f3d:native:scope#10",
        crate::records::feature::DesignFeatureKind::Fillet,
        10,
    );
    let laws = [
        (
            DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index: 11,
            },
            vec![(0, &weight)],
        ),
        (
            DesignFilletRadiusLaw::Chordal {
                chord_length_parameter_record_index: 11,
            },
            vec![(0, &weight)],
        ),
        (
            DesignFilletRadiusLaw::Asymmetric {
                offset_one_parameter_record_index: 11,
                offset_two_parameter_record_index: 12,
            },
            vec![(0, &weight), (1, &radius)],
        ),
        (
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index: 11,
                end_radius_parameter_record_index: 12,
                middle: Vec::new(),
            },
            vec![(0, &weight), (1, &radius)],
        ),
    ];
    for (law, parameters) in laws {
        let assignment = DesignFilletRadiusGroup {
            id: "f3d:native:assignment#20".into(),
            scope_record_index: 10,
            group_ordinal: 0,
            group_record_index: 20,
            edge_operand_record_indices: Vec::new(),
            law,
            tangency_weight_parameter_record_index: None,
        };
        let inputs = crate::design::feature_project::ProjectInputs {
            native: &[],
            owners: &[],
            scopes: &[],
            timelines: &[],
            construction_groups: &[],
            fillet_radius_groups: std::slice::from_ref(&assignment),
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &[],
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            component_naming_spaces: &[],
            histories: &[],
        };
        assert!(matches!(
            crate::design::feature_project::project_fillet_arm(
                &inputs,
                &scope,
                &parameters,
                "f3d:native"
            ),
            FeatureDefinition::Native { .. }
        ));
    }
}

#[test]
fn fillet_unit_conversion_rejects_finite_overflow() {
    let parameter = |kind, value| {
        parse_design_parameter(&parameter_record(
            Some(1),
            "1 mm",
            kind,
            Some("mm"),
            "radius",
            value,
        ))
        .unwrap()
    };
    let start = parameter("StartRadius", f64::MAX);
    let end = parameter("EndRadius", 1.0);
    assert!(crate::design::feature_project::design_length(&start).is_none());
    assert!(
        crate::design::feature_project::variable_fillet_law(&[(0, &start), (1, &end)]).is_none()
    );
}
