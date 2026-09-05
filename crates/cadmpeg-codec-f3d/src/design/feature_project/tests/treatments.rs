// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

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
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, scope_record_index, parameter_record_index, local_ordinal| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .expect("generated parameter owner is canonical");
        owner.id = format!("f3d:native:owner#{record_index}");
        owner.record_index = record_index;
        owner.scope_record_index = scope_record_index;
        owner.parameter_record_index = parameter_record_index;
        owner.companion_record_index = parameter_record_index + 1;
        owner.local_ordinal = local_ordinal;
        owner
    };
    let scope = |record_index, byte_offset, kind: &str| DesignParameterScope {
        id: format!("f3d:native:scope#{record_index}"),
        byte_offset,
        class_tag: "301".into(),
        record_index,
        frame_length: 200,
        kind_offset: byte_offset + 100,
        feature_ordinal: std::num::NonZeroU32::MIN,
        feature_ordinal_offset: 0,
        history_state_id: None,

        previous_history_state_id: None,
        previous_history_state_id_offset: None,
        reference_count_offset: byte_offset + 80,
        reference_members: crate::records::ReferenceRun::from_columns(vec![record_index + 1], vec![byte_offset + 85], "reference_members").unwrap(),
        payload: crate::records::DesignFeatureKind::try_from(kind.to_owned()).expect("nonempty family name").into(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: "261".into(),
        paired_byte_offset: byte_offset + 200,
    };
    let mut scopes = vec![
        scope(12, 100, "Fillet"),
        scope(22, 400, "Chamfer"),
        scope(32, 700, "Hole"),
    ];
    if let crate::records::DesignScopePayload::Hole(slot) = &mut scopes[2].payload {
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
            tangent_point_data: Some(crate::records::DesignHoleTangentPoint { prefix: 0, data: crate::records::Located { value: [-1.0, -1.0, -1.0], offset: 104 } }),
            input_records: vec![crate::records::Located { value: 378, offset: 129 }],
            face_selection: None,
        });
    }
    scopes[2].reference_members = crate::records::ReferenceRun::Unlocated(vec![0, 363, 0, 370, 0, 378]);
    let hole_face_operand = |record_index, scope_reference_ordinal| DesignFaceOperand {
        id: format!("f3d:native:face-operand#{record_index}"),
        scope_record_index: 32,
        scope_reference_ordinal,
        group: None,
        record_index,
        byte_offset: 1200,
        class_tag: "297".into(),
        paired_byte_offset: 1400,
        paired_class_tag: "259".into(),
        recipe_record_index: record_index + 3,
        recipe_record_byte_offset: 1300,
        recipe_id: format!("f3d:native:construction-recipe#{}", record_index + 3),
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
    let FeatureDefinition::Fillet { groups } = &fillet.definition else {
        panic!("expected typed fillet");
    };
    assert!(matches!(
        groups.as_slice(),
        [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Native(selection),
            radius: RadiusSpec::Constant { radius },
            tangency_weight: None,
        }] if selection == &scopes[0].id && radius.0 == 5.0
    ));
    let chamfer = features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Chamfer"))
        .expect("typed chamfer");
    assert!(matches!(
        &chamfer.definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                edges: EdgeSelection::Native(selection),
                spec: ChamferSpec::TwoDistances { first, second },
            }] if selection == &scopes[1].id && first.0 == 1.0 && second.0 == 2.0)
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
    distance_angle_parameters[1].unit.as_mut().expect("parameter unit").value = "deg".into();
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
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::DistanceAngle { distance, angle },
                ..
            }] if distance.0 == 1.6 && angle.0 == 25.0_f64.to_radians())
    ));

    distance_angle_parameters[0].source = crate::records::DesignParameterSource::new("leftDistance".into(), distance_angle_parameters[0].owner_record_index(), distance_angle_parameters[0].family_discriminator()).unwrap();
    distance_angle_parameters[1].source = crate::records::DesignParameterSource::new("rotateAngle".into(), distance_angle_parameters[1].owner_record_index(), distance_angle_parameters[1].family_discriminator()).unwrap();
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
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::DistanceAngle { distance, angle },
                ..
            }] if distance.0 == 1.6 && angle.0 == 25.0_f64.to_radians())
    ));

    let mut hole_parameters = [
        parameter(94, 95, "HoleDepth", "d4", "10 mm", 1.0),
        parameter(104, 105, "HoleDiameter", "d5", "4 mm", 0.4),
        parameter(114, 115, "TipAngle", "d6", "180 deg", std::f64::consts::PI),
    ];
    hole_parameters[2].unit.as_mut().expect("parameter unit").value = "deg".into();
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
        &features[0].definition,
        FeatureDefinition::Hole {
            face: Some(FaceSelection::Resolved { faces, native }),
            placements: Some(placements),
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Simple,
                ..
            },
            diameter: Some(Length(4.0)),
            extent: Some(cadmpeg_ir::features::LinearTermination::Blind { length: Length(10.0) }),
            bottom: Some(cadmpeg_ir::features::HoleBottom::Flat),
            ..
        } if faces == &vec![FaceId::mint(crate::ids::brep_entity_id(282)).expect("identity grammar")]
            && native == &scopes[2].id
            && placements == &vec![cadmpeg_ir::features::HolePlacement::Directed {
                position: Point3 { x: 12.5, y: -25.0, z: 37.5 },
                direction: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
            }]
    ));

    hole_parameters[2].evaluated_value = 118.0_f64.to_radians();
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
        &features[0].definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::SimpleDrilled { drill_point_angle },
                ..
            },
            bottom: None,
            ..
        } if drill_point_angle.0 == 118.0_f64.to_radians()
    ));

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
        &features[0].definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::CounterboreDrilled {
                    diameter: Length(8.0),
                    depth: Length(3.0),
                    drill_point_angle,
                },
                ..
            },
            bottom: None,
            ..
        } if drill_point_angle.0 == 118.0_f64.to_radians()
    ));

    counterbore_parameters[2].evaluated_value = std::f64::consts::PI;
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
        &features[0].definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: cadmpeg_ir::features::HoleKind::Counterbore {
                    diameter: Length(8.0),
                    depth: Length(3.0),
                },
                ..
            },
            bottom: Some(cadmpeg_ir::features::HoleBottom::Flat),
            ..
        }
    ));

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
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::TwoDistances { first, second },
                ..
            }] if first.0 == 1.0 && second.0 == 2.0)
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
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::Distance { distance },
                ..
            }] if distance.0 == 1.0)
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
        &features[0].definition,
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
        &features[0].definition,
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
        &features[0].definition,
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
        &features[0].definition,
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Chamfer,
            parameters,
        } if parameters.len() == 2
    ));

    let construction_group =
        |record_index, scope_reference_ordinal| DesignConstructionOperandGroup {
            id: format!("f3d:native:construction-group#{record_index}"),
            scope_record_index: 22,
            scope_reference_ordinal,
            record_index,
            byte_offset: 1_000 + u64::from(scope_reference_ordinal),
            class_tag: "288".into(),
            members: vec![crate::records::Located { value: record_index + 100, offset: 1_026 + u64::from(scope_reference_ordinal) }],
            lost_edge_references: Vec::new(),
            frame: crate::records::DesignConstructionOperandGroupFrame {
                member_count_offset: 1_021 + u64::from(scope_reference_ordinal),
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: vec![crate::records::Located { value: record_index + 1, offset: 1_050 + u64::from(scope_reference_ordinal) }],
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 100,
                opaque_index_offset: 1_068 + u64::from(scope_reference_ordinal),
                opaque_scalar: 0.5,
                opaque_scalar_offset: 1_072 + u64::from(scope_reference_ordinal),
                variant: false,
            },
            role: 0x0000_0008_0000_0000,
            extrude_role: None,
            role_offset: 1_060 + u64::from(scope_reference_ordinal),
            paired_class_tag: "259".into(),
            paired_byte_offset: 1_100 + u64::from(scope_reference_ordinal),
        };
    let mut construction_groups = [construction_group(90, 17), construction_group(80, 4)];
    construction_groups[1]
        .lost_edge_references
        .push("f3d:native:lost-edge-reference#1".into());
    let mut chamfer_scope = scopes[1].clone();
    chamfer_scope.previous_history_state_id = Some(21);
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
        &features[0].definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(groups.as_slice(), [
                ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Distance { distance: Length(2.5) },
                },
                ChamferGroup {
                    edges: EdgeSelection::Native(selection),
                    spec: ChamferSpec::Distance { distance: Length(2.0) },
                },
            ] if selection == &construction_groups[0].id)
    ));
}

#[test]
fn draft_entity_neutral_selection_projects_a_unique_historical_face() {
    use crate::records::{
        AsmHistoricalEntityKind, DesignConstructionOperandGroup,
        DesignConstructionOperandGroupFrame, DesignDraftOperation,
        DesignEntitySelectionFaceCandidate, DesignEntitySelectionOperand, DesignParameterScope,
        DesignScopePayload,
    };
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let stream = "f3d:test/Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::DesignFeatureKind::Draft,
        100,
    );
    scope.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    scope.previous_history_state_id = Some(7);
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![101, 111, 102, 112]);
    if let crate::records::DesignScopePayload::Draft(slot) = &mut scope.payload {
        *slot = Some(DesignDraftOperation {
            angle: -0.25,
            angle_record_index: 90,
            angle_offset: 0,
            opposite_angle_record_index: 91,
            opposite_angle_offset: 0,
        });
    }
    let group = |record_index, member, role| DesignConstructionOperandGroup {
        id: format!("{stream}:design-construction-operand-group#{record_index}"),
        scope_record_index: 100,
        scope_reference_ordinal: 0,
        record_index,
        byte_offset: 0,
        class_tag: "000".into(),
        members: vec![crate::records::Located { value: member, offset: 0 }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 0,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role,
        extrude_role: None,
        role_offset: 0,
        paired_class_tag: "000".into(),
        paired_byte_offset: 0,
    };
    let groups = [
        group(101, 111, 0x0000_0010_0000_0000),
        group(102, 112, 0x0000_0021_0000_0000),
    ];
    let mut selection = DesignEntitySelectionOperand {
        id: format!("{stream}:design-entity-selection-operand#112"),
        scope_record_index: 100,
        group_record_index: 102,
        group_member_ordinal: 0,
        record_index: 112,
        byte_offset: 0,
        class_tag: "000".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        identity_record_index: 113,
        identity_record_offset: 0,
        primary_identity: 225,
        primary_identity_offset: 0,
        secondary: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: vec![DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical: crate::records::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 225,
                state_ids: vec![7, 6],
            },
            face_slot: 158,
        }],
        resolved_edge_slot: None,
        next_record_index: 114,
        next_byte_offset: 0,
    };
    let timeline = crate::records::DesignFeatureTimeline {
frame: crate::records::DesignTimelineFrame::test_items(0, vec![crate::records::Located { value: 100, offset: 0 }]),
id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
class_tag: crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
record_index: std::num::NonZeroU64::new(1).unwrap(),
source_ordinal: 0,
context_record_index: std::num::NonZeroU64::new(1).unwrap(),
};
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
    } = &features[0].definition
    else {
        panic!("expected a typed Draft definition");
    };
    assert_eq!(angle.as_ref().map(|angle| angle.0), Some(-0.25));
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
        .0
        .split_once('#')
        .map_or(feature.0.as_str(), |(_, key)| key);
    let prefix = crate::ids::history_input_prefix(feature_key, 7);
    assert_eq!(
        state,
        &crate::design::edge_resolve::feature_input_topology_id(&feature, 7)
    );
    assert_eq!(
        faces.as_slice(),
        &[crate::ids::history_input_face_id(&prefix, 158)]
    );
    assert_eq!(native, &groups[1].id);

    selection
        .historical_face_candidates
        .push(DesignEntitySelectionFaceCandidate {
            history_id: "other-history".into(),
            historical: crate::records::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 225,
                state_ids: vec![7],
            },
            face_slot: 159,
        });
    let (features, _) = project(&selection);
    assert!(matches!(
        &features[0].definition,
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
        points,
        [
            cadmpeg_ir::features::VariableRadius {
                parameter: 0.0,
                radius: Length(0.0),
            },
            cadmpeg_ir::features::VariableRadius {
                parameter: 0.25,
                radius: Length(4.0),
            },
            cadmpeg_ir::features::VariableRadius {
                parameter: 1.0,
                radius: Length(0.0),
            },
        ]
    );
    assert_eq!(tangency_weight, Some(0.75));
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
        points,
        [
            cadmpeg_ir::features::VariableRadius {
                parameter: 0.0,
                radius: Length(2.0),
            },
            cadmpeg_ir::features::VariableRadius {
                parameter: 1.0,
                radius: Length(4.0),
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
    let scope = DesignParameterScope {
        id: "f3d:native:scope#12".into(),
        byte_offset: 100,
        class_tag: "301".into(),
        record_index: 12,
        frame_length: 200,
        kind_offset: 210,
        feature_ordinal: std::num::NonZeroU32::MIN,
        feature_ordinal_offset: 0,
        history_state_id: None,

        previous_history_state_id: None,
        previous_history_state_id_offset: None,
        reference_count_offset: 180,
        reference_members: crate::records::ReferenceRun::from_columns(vec![100, 101], vec![185, 196], "reference_members").unwrap(),
        payload: crate::records::DesignFeatureKind::Conge.into(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: "261".into(),
        paired_byte_offset: 300,
    };
    let group = |record_index, ordinal, members: Vec<u32>| DesignConstructionOperandGroup {
        id: format!("f3d:native:construction-group#{record_index}"),
        scope_record_index: 12,
        scope_reference_ordinal: ordinal,
        record_index,
        byte_offset: 1000 + u64::from(ordinal) * 200,
        class_tag: "288".into(),


        members: members.into_iter().enumerate().map(|(index, value)| crate::records::Located { value, offset: 1026 + u64::from(ordinal) * 200 + index as u64 * 11 }).collect(),
        lost_edge_references: Vec::new(),
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 1021 + u64::from(ordinal) * 200,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: vec![crate::records::Located { value: 300 + ordinal, offset: 1100 + u64::from(ordinal) * 200 }],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 100,
            opaque_index_offset: 1128 + u64::from(ordinal) * 200,
            opaque_scalar: 0.5,
            opaque_scalar_offset: 1132 + u64::from(ordinal) * 200,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        role_offset: 1110 + u64::from(ordinal) * 200,

        paired_class_tag: "259".into(),
        paired_byte_offset: 1200 + u64::from(ordinal) * 200,
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
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter
    };
    let owner = |record_index, parameter_record_index, local_ordinal| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame()).unwrap();
        owner.id = format!("f3d:native:owner#{record_index}");
        owner.record_index = record_index;
        owner.scope_record_index = 12;
        owner.parameter_record_index = parameter_record_index;
        owner.local_ordinal = local_ordinal;
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
    if let crate::records::DesignScopePayload::Fillet(slot)
    | crate::records::DesignScopePayload::Conge(slot)
    | crate::records::DesignScopePayload::Abrundung(slot)
    | crate::records::DesignScopePayload::Arredondamento(slot) = &mut indexed_scope.payload
    {
        *slot = Some(crate::records::DesignFixedFilletParameters {
            groups: vec![crate::records::DesignFixedFilletGroup {
                tangency_weight: Some(crate::records::DesignFixedFilletScalar {
                    value: 1.0,
                    record_index: 10,
                    value_offset: 100,
                }),
                law: crate::records::DesignFixedFilletLaw::Constant(crate::records::DesignFixedFilletScalar { value: 0.5, record_index: 20, value_offset: 200 }),
            }],
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
        crate::records::DesignFilletRadiusLaw::Constant {
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
        crate::records::DesignFilletRadiusLaw::Constant {
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
        crate::records::DesignFilletRadiusLaw::Variable {
            start_radius_parameter_record_index: 51,
            end_radius_parameter_record_index: 61,
            middle: vec![crate::records::DesignFilletMidpoint { radius_parameter_record_index: 71, parameter_record_index: 81 }],
        }
    );
    assert_eq!(
        variable_assignments[0].tangency_weight_parameter_record_index,
        Some(91)
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
        crate::records::DesignFilletRadiusLaw::Chordal {
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
        &chord_features[0].definition,
        FeatureDefinition::Fillet { groups }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::FilletGroup {
                    radius: cadmpeg_ir::features::RadiusSpec::Chordal {
                        chord_length: cadmpeg_ir::features::Length(2.5),
                    },
                    tangency_weight: Some(1.0),
                    ..
                }]
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
        crate::records::DesignFilletRadiusLaw::Chordal {
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
        &chord_only_features[0].definition,
        FeatureDefinition::Fillet { groups }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::FilletGroup {
                    radius: cadmpeg_ir::features::RadiusSpec::Chordal {
                        chord_length: cadmpeg_ir::features::Length(2.5),
                    },
                    tangency_weight: None,
                    ..
                }]
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
        crate::records::DesignFilletRadiusLaw::Asymmetric {
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
        &asymmetric_features[0].definition,
        FeatureDefinition::Fillet { groups }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::FilletGroup {
                    radius: cadmpeg_ir::features::RadiusSpec::Asymmetric {
                        offset_one: cadmpeg_ir::features::Length(2.0),
                        offset_two: cadmpeg_ir::features::Length(7.0),
                    },
                    tangency_weight: Some(1.0),
                    ..
                }]
            )
    ));
    operand_groups[0]
        .lost_edge_references
        .push("f3d:native:lost-edge-reference#1".into());

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
    let FeatureDefinition::Fillet { groups } = &features[0].definition else {
        panic!("expected typed localized Fillet");
    };
    assert_eq!(groups.len(), 2);
    assert!(matches!(
        &groups[0],
        cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(5.0),
            },
            tangency_weight: Some(1.0),
        }
    ));
    assert!(matches!(
        &groups[1],
        cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(3.0),
            },
            tangency_weight: Some(0.75),
        } if selection == &operand_groups[1].id
    ));

    let mut patch_scope = scope.clone();
    patch_scope.payload = crate::records::DesignFeatureKind::SurfacePatch.into();
    patch_scope.frame_length = 354;
    patch_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 300, 301]);
    let patch_boundary = |scope_reference_ordinal, record_index, model_reference| {
        crate::records::DesignSurfacePatchBoundary {
            scope_reference_ordinal,
            record_index,
            is_seed_selection: false,
            continuity: crate::records::DesignPatchContinuity::Connected,
            flip: 2,
            scale: -1.0,
            model_reference,
        }
    };
    if let crate::records::DesignScopePayload::SurfacePatch(slot) = &mut patch_scope.payload {
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    let mut patch_group = group(100, 0, vec![200]);
    patch_group.role = 0x0000_0004_0000_0000;
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

    patch_scope.frame_length = 398;
    patch_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 300, 101, 201, 301, 102]);
    if let crate::records::DesignScopePayload::SurfacePatch(slot) = &mut patch_scope.payload {
        *slot = vec![patch_boundary(2, 300, 100), patch_boundary(5, 301, 101)];
    }
    let mut second_patch_group = group(101, 3, vec![201]);
    second_patch_group.role = 0x0000_0004_0000_0000;
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

    patch_scope.previous_history_state_id = Some(8);
    let edge_identity =
        |record_index, group_record_index, edge| crate::records::DesignEdgeIdentityOperand {
            id: format!("f3d:native:edge-identity#{record_index}"),
            scope_record_index: patch_scope.record_index,
            group_record_index,
            group_member_ordinal: 0,
            record_index,
            byte_offset: 0,
            class_tag: "297".into(),
            compact_layout: false,
            local_id: u64::from(record_index),
            local_id_offset: 0,
            asset_id: "asset".into(),
            asset_id_offset: 0,
            context_id: "context".into(),
            context_id_offset: 0,
            historical: None,
            treatment_radius_candidates: Vec::new(),
            transition_edge_candidates: Vec::new(),
            resolved_edge_slots: Vec::new(),
            resolved_edge_slot: Some(edge),
            resolution_identity_id: None,
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
    assert_eq!(native, patch_scope.id);
    patch_scope.previous_history_state_id = None;

    patch_scope.frame_length = 339;
    patch_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 300]);
    if let crate::records::DesignScopePayload::SurfacePatch(slot) = &mut patch_scope.payload {
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    patch_group.role = 0x0000_0041_0000_0000;
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
    patch_scope.frame_length = 325;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface { .. })
    ));
    patch_scope.frame_length = 340;
    patch_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 300, 301]);
    if let crate::records::DesignScopePayload::SurfacePatch(slot) = &mut patch_scope.payload {
        *slot = vec![patch_boundary(2, 300, 100)];
    }
    patch_group.role = 0x0000_0004_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_surface_patch(
            &patch_scope,
            std::slice::from_ref(&patch_group),
            &[],
            &[],
        ),
        Some(FeatureDefinition::FilledSurface { .. })
    ));
    patch_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 300, 301, 302]);
    assert!(crate::design::feature_project::project_surface_patch(
        &patch_scope,
        std::slice::from_ref(&patch_group),
        &[],
        &[],
    )
    .is_none());

    patch_scope.frame_length = 343;
    patch_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 201, 202, 203, 300]);
    patch_scope.payload = crate::records::DesignScopePayload::SurfacePatch(Vec::new());
    patch_group.members = vec![200, 201, 202, 203].into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect();
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
    fill_scope.payload = crate::records::DesignFeatureKind::BoundaryFill.into();
    fill_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 201, 300, 301, 400]);
    let mut tools = group(100, 0, vec![200, 201]);
    tools.role = 0x0000_0004_0000_0000;
    let mut cell = group(300, 3, vec![301]);
    cell.role = 0x0000_0005_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_boundary_fill(&fill_scope, &[tools.clone(), cell.clone()]),
        Some(FeatureDefinition::BoundaryFill {
            tools: cadmpeg_ir::features::BodySelection::Native(ref tool_selection),
            cells: ref cell_selections,
        }) if tool_selection == &tools.id
            && cell_selections == &[cadmpeg_ir::features::BodySelection::Native(cell.id)]
    ));
}
