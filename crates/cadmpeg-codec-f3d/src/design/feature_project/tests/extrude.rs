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

fn set_extrude_operation(scope: &mut DesignParameterScope, operation: DesignExtrudeOperation) {
    let Some(
        DesignExtrudePrologue::LegacyDistance {
            operation: value, ..
        }
        | DesignExtrudePrologue::ReferenceAware {
            operation: value, ..
        }
        | DesignExtrudePrologue::ShiftedReferenceAware {
            operation: value, ..
        }
        | DesignExtrudePrologue::LegacyShifted {
            operation: value, ..
        },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *value = operation;
}

fn set_extrude_extent(scope: &mut DesignParameterScope, extent: DesignExtrudeExtent) {
    let Some(prologue) = scope.extrude_prologue_mut() else {
        panic!("test scope must carry an Extrude prologue");
    };
    match prologue {
        DesignExtrudePrologue::LegacyDistance { .. } => {
            assert_eq!(extent, DesignExtrudeExtent::OneSidedDistance);
        }
        DesignExtrudePrologue::ReferenceAware {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            first_side_target_ordinal,
            ..
        } => {
            *value = extent;
            (direction_face_extend_values[0], *side_extent_discriminators) = match extent {
                DesignExtrudeExtent::OneSidedDistance => (1, [1, 0]),
                DesignExtrudeExtent::OneSidedToFace => (1, [2, 0]),
                DesignExtrudeExtent::OneSidedThroughNext => (1, [3, 0]),
                DesignExtrudeExtent::OneSidedThroughAll => (1, [4, 0]),
                DesignExtrudeExtent::TwoSidedToFaces => (2, [2, 0]),
                DesignExtrudeExtent::TwoSidedDistance => (2, [1, 1]),
                DesignExtrudeExtent::TwoSidedDistanceToFace => (2, [1, 2]),
                DesignExtrudeExtent::SymmetricDistance => (3, [1, 0]),
                DesignExtrudeExtent::SymmetricThroughAll => (3, [4, 4]),
            };
            if extent != DesignExtrudeExtent::OneSidedToFace {
                *first_side_target_ordinal = None;
            }
        }
        DesignExtrudePrologue::LegacyShifted {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            ..
        } => {
            *value = Some(extent);
            (*direction_face_extend_values, *side_extent_discriminators) = match extent {
                DesignExtrudeExtent::OneSidedDistance => ([1, 0], [1, 0]),
                DesignExtrudeExtent::OneSidedToFace => ([1, 0], [2, 0]),
                DesignExtrudeExtent::OneSidedThroughNext => ([1, 0], [3, 0]),
                DesignExtrudeExtent::OneSidedThroughAll => ([1, 0], [4, 0]),
                DesignExtrudeExtent::TwoSidedToFaces => ([2, 0], [2, 0]),
                DesignExtrudeExtent::TwoSidedDistance => ([2, 0], [1, 1]),
                DesignExtrudeExtent::TwoSidedDistanceToFace => ([2, 0], [1, 2]),
                DesignExtrudeExtent::SymmetricDistance => ([3, 0], [1, 0]),
                DesignExtrudeExtent::SymmetricThroughAll => ([3, 0], [4, 4]),
            };
        }
        DesignExtrudePrologue::ShiftedReferenceAware {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            ..
        } => {
            assert_eq!(extent, DesignExtrudeExtent::TwoSidedToFaces);
            *value = extent;
            (*direction_face_extend_values, *side_extent_discriminators) = ([2, 1], [2, 0]);
        }
    }
}

fn set_extrude_direction_reversed(scope: &mut DesignParameterScope, reversed: bool) {
    let Some(
        DesignExtrudePrologue::LegacyDistance {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::ReferenceAware {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::ShiftedReferenceAware {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::LegacyShifted {
            direction_reversed, ..
        },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *direction_reversed = reversed;
}

fn set_extrude_start(scope: &mut DesignParameterScope, start: DesignExtrudeStart) {
    let Some(
        DesignExtrudePrologue::ReferenceAware { start: value, .. }
        | DesignExtrudePrologue::ShiftedReferenceAware { start: value, .. }
        | DesignExtrudePrologue::LegacyShifted { start: value, .. },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *value = start;
}

#[test]
fn extrude_parameters_project_blind_two_sided_and_reversed_extents() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart,
        FaceSelection, LinearTermination, ProfileRef,
    };

    let parameter = |source_kind: &str, unit: &str, value| {
        parse_design_parameter(&parameter_record(
            Some(44),
            "value",
            source_kind,
            Some(unit),
            "d1",
            value,
        ))
        .expect("generated feature parameter is canonical")
    };
    let mut scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:scope#12".into(),
        byte_offset: 100,
        class_tag: "301".into(),
        record_index: 12,
        frame_length: 200,
        kind: "Extrude".into(),
        kind_offset: 210,
        payload: DesignScopePayload::Extrude(crate::records::DesignExtrudeScope {
            extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 128,
                direction_face_extend_values: [1, 2],
                side_extent_discriminators: [1, 0],
                side_extent_discriminator_offsets: [177, 190],
                first_side_target_ordinal: None,
                extent: DesignExtrudeExtent::OneSidedDistance,
                direction_face_extend_offsets: [132, 136],
                direction_reversed: false,
                direction_reversed_offset: 140,
                solid_operation: true,
                solid_operation_offset: 141,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 142,
            }),
            extrude_profile: Some(DesignSketchProfileOperand {
                scope_reference_ordinal: 0,
                record_index: 100,
                byte_offset: 300,
                class_tag: "308".into(),
                asset_id: "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".into(),
                asset_id_offset: 330,
                entity_id: "0_172".into(),
                entity_suffix: 172,
                entity_reference_offset: 420,
                region_selection: None,
                paired_class_tag: "259".into(),
                paired_byte_offset: 520,
            }),
            ..crate::records::DesignExtrudeScope::default()
        }),
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: None,
        reference_count_offset: 180,
        reference_members: vec![100],
        reference_member_offsets: vec![185],
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: "261".into(),
        paired_byte_offset: 300,
    };
    let placement = DesignSketchPlacement {
        member_run_head: false,
        id: "f3d:Design/BulkStream.dat:placement#200".into(),
        scope_record_index: Some(11),
        entity_id: "0_172".into(),
        entity_suffix: 172,
        visibility: None,
        byte_offset: 600,
        class_tag: "300".into(),
        record_index: 200,
        frame_length: 329,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(655),
        paired_class_tag: "260".into(),
        paired_byte_offset: 929,
    };
    let along = parameter("AlongDistance", "mm", 0.55);
    let taper = parameter("TaperAngle", "deg", 0.2);
    let blind = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed blind Extrude");
    assert!(matches!(
        &blind,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(profile),
            direction: ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length: Length(5.5) },
                    draft: Some(Angle(0.2)),
                },
            },
            op: BooleanOp::NewBody,
            solid: Some(true),
            ..
        } if profile == &neutral_sketch_id(&placement)
    ));
    let reference_aware_prologue = scope.extrude_prologue();
    let Some(DesignExtrudePrologue::ReferenceAware {
        solid_operation, ..
    }) = scope.extrude_prologue_mut()
    else {
        panic!("reference-aware Extrude prologue");
    };
    *solid_operation = false;
    let sheet = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed sheet Extrude");
    assert!(matches!(
        sheet,
        FeatureDefinition::Extrude {
            solid: Some(false),
            ..
        }
    ));
    scope.ensure_extrude().extrude_prologue = Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker: None,
        operation_prefix_marker_offset: None,
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: 127,
        direction_face_extend_values: [3, 2],
        side_extent_discriminators: [1, 0],
        side_extent_discriminator_offsets: [206, 210],
        extent: Some(DesignExtrudeExtent::SymmetricDistance),
        direction_face_extend_offsets: [131, 135],
        direction_reversed: false,
        direction_reversed_offset: 139,
        solid_operation: true,
        solid_operation_offset: 140,
        start: DesignExtrudeStart::ProfilePlane,
        start_offset: 141,
    });
    let symmetric = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed symmetric Extrude");
    assert!(matches!(
        symmetric,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(5.5)
                    },
                    draft: Some(Angle(0.2)),
                },
            },
            ..
        }
    ));
    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedThroughAll);
    set_extrude_direction_reversed(&mut scope, true);
    let through_all = project_extrude(
        &scope,
        &[(1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed through-all Extrude");
    assert!(matches!(
        through_all,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ThroughAll,
                    draft: Some(Angle(0.2)),
                },
            },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, false);
    set_extrude_extent(&mut scope, DesignExtrudeExtent::SymmetricThroughAll);
    let symmetric_through_all = project_extrude(
        &scope,
        &[(1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed symmetric through-all Extrude");
    assert!(matches!(
        symmetric_through_all,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::ThroughAll,
                    draft: Some(Angle(0.2)),
                },
            },
            ..
        }
    ));
    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
    let selection = DesignExtrudeSelectionGroup {
        id: "f3d:Design/BulkStream.dat:selection#300".into(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal: 0,
        record_index: 300,
        byte_offset: 700,
        class_tag: "308".into(),
        member_count_offset: 720,
        members: vec![301],
        member_offsets: vec![724],
        opaque_index: 1,
        opaque_index_offset: 735,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 739,
        variant: false,
        paired_class_tag: "259".into(),
        paired_byte_offset: 760,
    };
    let mut feature = Feature {
        id: FeatureId("f3d:model:feature#extrude".into()),
        ordinal: 0,
        name: Some("Extrude".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: blind,
        native_ref: Some(scope.id.clone()),
    };
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    bind_extrude_profile_selections(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        std::slice::from_ref(&selection),
        &[],
        &[],
        &crate::design::profile_select::SketchCurveSelectionResolution {
            scopes: &[],
            groups: &[],
            operands: &[],
            placements: &[],
            curve_identities: &[],
            sketches: &[],
            sketch_entities: &[],
            spatial_sketches: &[],
            spatial_sketch_entities: &[],
        },
        crate::design::profile_select::ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &std::collections::HashMap::new(),
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-9,
            arrangement_budget: &arrangement_budget,
        },
    );
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(ref native),
            ..
        } if native == &selection.id
    ));
    set_extrude_direction_reversed(&mut scope, true);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    set_extrude_direction_reversed(&mut scope, false);
    let unsupported = parameter("UnclassifiedControl", "mm", 1.0);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &unsupported)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    let side_two_taper = parameter("Side2TaperAngle", "deg", -0.3);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    let invalid_taper = parameter("TaperAngle", "native-unit", 0.2);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &invalid_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    let mut owned_along = along.clone();
    owned_along.id = "f3d:Design/BulkStream.dat:parameter#45".into();
    owned_along.record_index = 45;
    owned_along.owner = crate::records::DesignParameterOwnerKind::Feature {
        owner_record_index: 44,
    };
    let mut owner = parse_parameter_owner(&parameter_owner_frame())
        .expect("generated parameter owner is canonical");
    owner.id = "f3d:Design/BulkStream.dat:owner#44".into();
    owner.record_index = 44;
    owner.scope_record_index = scope.record_index;
    owner.parameter_record_index = owned_along.record_index;
    let mut sketch_scope = scope.clone();
    sketch_scope.id = "f3d:Design/BulkStream.dat:scope#11".into();
    sketch_scope.record_index = placement
        .scope_record_index
        .expect("test placement carries a scope record index");
    sketch_scope.kind = "Sketch".into();
    sketch_scope.ensure_extrude().extrude_prologue = None;
    sketch_scope.ensure_extrude().extrude_profile = None;
    let scopes = vec![sketch_scope, scope.clone()];
    let (mut features, _) = project_parameter_design(
        std::slice::from_ref(&owned_along),
        std::slice::from_ref(&owner),
        &scopes,
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&placement),
    );
    let sketches = [cadmpeg_ir::sketches::Sketch {
        id: neutral_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    }];
    crate::design::feature_project::bind_sketch_feature_geometry(
        &mut features,
        &scopes,
        std::slice::from_ref(&placement),
        &sketches,
        &[],
    );
    let sketch_feature = features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .expect("neutral Sketch feature");
    let extrude_feature = features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::Extrude { .. }))
        .expect("neutral Extrude feature");
    assert_eq!(extrude_feature.dependencies, [sketch_feature.id.clone()]);

    let (mut spatial_features, _) = project_parameter_design(
        std::slice::from_ref(&owned_along),
        std::slice::from_ref(&owner),
        &scopes,
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&placement),
    );
    let spatial_sketch = cadmpeg_ir::sketches::SpatialSketch {
        id: neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![cadmpeg_ir::sketches::SpatialSketchProfile {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            boundary: Vec::new(),
        }],
        native_ref: Some(placement.id.clone()),
    };
    crate::design::feature_project::bind_sketch_feature_geometry(
        &mut spatial_features,
        &scopes,
        std::slice::from_ref(&placement),
        &[],
        std::slice::from_ref(&spatial_sketch),
    );
    let spatial_feature = spatial_features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::SpatialSketch { .. }))
        .expect("neutral spatial Sketch feature");
    let spatial_extrude = spatial_features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::Extrude { .. }))
        .expect("spatial-profile Extrude feature");
    assert!(matches!(
        spatial_extrude.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::SpatialSketchProfiles {
                ref sketch,
                ref profiles
            },
            ..
        } if sketch == &spatial_sketch.id && profiles == &[0]
    ));
    assert_eq!(spatial_extrude.dependencies, [spatial_feature.id.clone()]);

    let (mut open_spatial_features, _) = project_parameter_design(
        std::slice::from_ref(&owned_along),
        std::slice::from_ref(&owner),
        &scopes,
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&placement),
    );
    let open_spatial_sketch = cadmpeg_ir::sketches::SpatialSketch {
        id: neutral_spatial_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some(placement.id.clone()),
    };
    crate::design::feature_project::bind_sketch_feature_geometry(
        &mut open_spatial_features,
        &scopes,
        std::slice::from_ref(&placement),
        &[],
        std::slice::from_ref(&open_spatial_sketch),
    );
    let open_spatial_extrude = open_spatial_features
        .iter()
        .find(|feature| matches!(feature.definition, FeatureDefinition::Extrude { .. }))
        .expect("open spatial-profile Extrude feature");
    assert!(matches!(
        open_spatial_extrude.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::SpatialSketchSelection {
                ref sketch,
                ref selections
            },
            ..
        } if sketch == &open_spatial_sketch.id
            && selections == &[format!(
                "f3d:Design/BulkStream.dat:design-record-header#{}",
                scope
                    .extrude_profile()
                    .as_ref()
                    .expect("test profile operand")
                    .byte_offset
            )]
    ));

    let body_group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#101".into(),
        scope_record_index: 12,
        scope_reference_ordinal: 1,
        record_index: 101,
        byte_offset: 1000,
        class_tag: "332".into(),
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![1026],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 1021,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![300],
            trailing_record_offsets: vec![1044],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 180,
            opaque_index_offset: 1072,
            opaque_scalar: 0.125,
            opaque_scalar_offset: 1076,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Bodies),
        role_offset: 1054,

        paired_class_tag: "259".into(),
        paired_byte_offset: 1125,
    };
    set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);
    let target_body = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed target-body Extrude");
    assert!(matches!(
        target_body,
        FeatureDefinition::Extrude {
            op: BooleanOp::Join,
            ..
        }
    ));

    scope.ensure_extrude().extrude_prologue = reference_aware_prologue;
    set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);
    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedToFace);
    let mut target_shape_group = body_group.clone();
    target_shape_group.id = "f3d:Design/BulkStream.dat:operand-group#105".into();
    target_shape_group.record_index = 105;
    target_shape_group.scope_reference_ordinal = 2;
    target_shape_group.members = vec![201];
    target_shape_group.member_offsets = vec![1026];
    target_shape_group.role = 0x0000_0005_0000_0000;
    target_shape_group.extrude_role = None;
    let Some(DesignExtrudePrologue::ReferenceAware {
        first_side_target_ordinal,
        ..
    }) = scope.extrude_prologue_mut()
    else {
        panic!("reference-aware target-shape Extrude prologue");
    };
    *first_side_target_ordinal = Some(DesignExtrudeTargetOrdinal {
        scope_reference_ordinal: target_shape_group.scope_reference_ordinal,
        scope_reference_ordinal_offset: 187,
    });
    let mut unrelated_target_group = target_shape_group.clone();
    unrelated_target_group.id = "f3d:Design/BulkStream.dat:operand-group#106".into();
    unrelated_target_group.record_index = 106;
    unrelated_target_group.scope_reference_ordinal = 3;
    unrelated_target_group.members = vec![202];
    let mut target_shape_operand = DesignBodyRecipeOperand {
        id: "f3d:Design/BulkStream.dat:body-recipe-operand#201".into(),
        scope_record_index: scope.record_index,
        owner: DesignOperandOwner::Group {
            group_record_index: target_shape_group.record_index,
            group_member_ordinal: 0,
        },
        record_index: 201,
        byte_offset: 0,
        class_tag: "295".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        selector_tail: None,
        selector_tail_offset: None,
        references: vec![DesignBodyRecipeReference {
            design_reference: 301,
            design_reference_offset: 0,
            form: 33,
            form_offset: 0,
            candidate_faces: vec![
                FaceId::mint("f3d:brep:entity#12").expect("identity grammar"),
                FaceId::mint("f3d:brep:entity#19").expect("identity grammar"),
            ],
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 204,
        nested_record_index_offset: 0,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#205".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 205,
        next_byte_offset: 0,
    };
    let unresolved_target_shape = project_extrude(
        &scope,
        &[(0, &taper)],
        &[
            body_group.clone(),
            target_shape_group.clone(),
            unrelated_target_group.clone(),
        ],
        &[],
        std::slice::from_ref(&placement),
        std::slice::from_ref(&target_shape_operand),
    )
    .expect("typed target-shape Extrude");
    assert!(matches!(
        unresolved_target_shape,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToShape {
                        target: FaceSelection::Native(ref native),
                    },
                    ..
                },
            },
            ..
        } if native == &target_shape_group.id
    ));

    target_shape_operand.resolved_body_state_id = Some(7);
    target_shape_operand.resolved_body_slot = Some(3);
    target_shape_operand.resolved_body_face_slots = vec![12, 19, 27];
    let target_shape = project_extrude(
        &scope,
        &[(0, &taper)],
        &[
            body_group.clone(),
            target_shape_group.clone(),
            unrelated_target_group,
        ],
        &[],
        std::slice::from_ref(&placement),
        std::slice::from_ref(&target_shape_operand),
    )
    .expect("resolved target-shape Extrude");
    let feature = crate::ids::neutral_feature_id(&scope);
    let feature_key = feature
        .0
        .split_once('#')
        .map_or(feature.0.as_str(), |(_, key)| key);
    let prefix = crate::ids::history_input_prefix(feature_key, 7);
    assert!(matches!(
        target_shape,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToShape {
                        target: FaceSelection::Historical {
                            ref state,
                            ref faces,
                            ref native,
                        },
                    },
                    ..
                },
            },
            ..
        } if state == &crate::design::edge_resolve::feature_input_topology_id(&feature, 7)
            && faces == &[
                crate::ids::history_input_face_id(&prefix, 12),
                crate::ids::history_input_face_id(&prefix, 19),
                crate::ids::history_input_face_id(&prefix, 27),
            ]
            && native == &target_shape_group.id
    ));

    let mut multi_target_group = target_shape_group.clone();
    multi_target_group.members.push(202);
    multi_target_group.member_offsets.push(1030);
    let mut second_target_operand = target_shape_operand.clone();
    second_target_operand.id = "f3d:Design/BulkStream.dat:body-recipe-operand#202".into();
    second_target_operand.owner = DesignOperandOwner::Group {
        group_record_index: multi_target_group.record_index,
        group_member_ordinal: 1,
    };
    second_target_operand.record_index = 202;
    second_target_operand.resolved_body_slot = Some(4);
    second_target_operand.resolved_body_face_slots = vec![30, 31];
    let operands = [target_shape_operand.clone(), second_target_operand.clone()];
    assert!(matches!(
        resolved_body_recipe_shape(&scope, &multi_target_group, &operands),
        Some(FaceSelection::Historical { faces, .. })
            if faces == [
                crate::ids::history_input_face_id(&prefix, 12),
                crate::ids::history_input_face_id(&prefix, 19),
                crate::ids::history_input_face_id(&prefix, 27),
                crate::ids::history_input_face_id(&prefix, 30),
                crate::ids::history_input_face_id(&prefix, 31),
            ]
    ));
    second_target_operand.resolved_body_state_id = Some(8);
    assert!(resolved_body_recipe_shape(
        &scope,
        &multi_target_group,
        &[target_shape_operand.clone(), second_target_operand],
    )
    .is_none());

    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
    set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
    let sketch_profile = scope.extrude_profile().cloned();
    scope.ensure_extrude().extrude_profile = None;
    let mut first_profile_group = body_group.clone();
    first_profile_group.id = "f3d:Design/BulkStream.dat:operand-group#102".into();
    first_profile_group.record_index = 102;
    first_profile_group.scope_reference_ordinal = 0;
    first_profile_group.extrude_role = Some(DesignExtrudeOperandRole::Profile);
    let mut second_profile_group = first_profile_group.clone();
    second_profile_group.id = "f3d:Design/BulkStream.dat:operand-group#103".into();
    second_profile_group.record_index = 103;
    second_profile_group.scope_reference_ordinal = 1;
    let multiple_profiles = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[first_profile_group.clone(), second_profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed multi-profile Extrude");
    assert!(matches!(
        multiple_profiles,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(ref native),
            op: BooleanOp::NewBody,
            ..
        } if native == &scope.id
    ));
    second_profile_group.scope_reference_ordinal = 0;
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[first_profile_group, second_profile_group],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    scope.ensure_extrude().extrude_profile = sketch_profile;
    set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);

    let mut profile_group = body_group.clone();
    profile_group.id = "f3d:Design/BulkStream.dat:operand-group#104".into();
    profile_group.record_index = 104;
    profile_group.extrude_role = Some(DesignExtrudeOperandRole::Profile);
    profile_group.role = 0x0000_0041_0000_0000;
    let direct_profile_with_selection_group = project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("direct sketch profile with a scoped selection group");
    assert!(matches!(
        direct_profile_with_selection_group,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(ref profile),
            ..
        } if profile == &neutral_sketch_id(&placement)
    ));
    scope.ensure_extrude().fixed_extrude_parameters = Some(DesignFixedExtrudeParameters {
        along_distance: Some(DesignFixedExtrudeDistance::DistanceConstruction(
            DesignFixedExtrudeScalar {
                value: 0.55,
                record_index: 105,
                value_offset: 600,
            },
        )),
        taper_angle: None,
    });
    let zero_side_offset = parameter("Side1Offset", "mm", 0.0);
    let hybrid = project_extrude(
        &scope,
        &[(0, &zero_side_offset), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed hybrid fixed-distance Extrude");
    assert!(matches!(
        hybrid,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(5.5)
                    },
                    ..
                },
            },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, true);
    let reversed_hybrid = project_extrude(
        &scope,
        &[(0, &zero_side_offset), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed reversed hybrid fixed-distance Extrude");
    assert!(matches!(
        reversed_hybrid,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(5.5)
                    },
                    ..
                },
            },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, false);
    scope.ensure_extrude().fixed_extrude_parameters = None;
    let mut native_profile_scope = scope.clone();
    native_profile_scope.ensure_extrude().extrude_profile = None;
    let reversed_native_profile = project_extrude(
        &native_profile_scope,
        &[(0, &parameter("AlongDistance", "mm", -0.2)), (1, &taper)],
        &[body_group.clone(), profile_group.clone()],
        &[],
        &[],
        &[],
    )
    .expect("typed reversed Extrude with a native profile");
    assert!(matches!(
        reversed_native_profile,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(ref native),
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(2.0)
                    },
                    ..
                },
            },
            op: BooleanOp::Join,
            ..
        } if native == &profile_group.id
    ));

    let mut face_group = body_group.clone();
    face_group.id = "f3d:Design/BulkStream.dat:operand-group#102".into();
    face_group.extrude_role = Some(DesignExtrudeOperandRole::Faces(None));
    face_group.role = 0x0000_0011_0000_0000;
    let mut ordered_faces = [face_group.clone(), face_group.clone()];
    set_extrude_start(&mut scope, DesignExtrudeStart::FromFace);
    assign_extrude_face_roles(&scope, &mut ordered_faces);
    assert_eq!(
        ordered_faces.map(|group| group.extrude_face_role()),
        [
            Some(DesignExtrudeFaceRole::Start),
            Some(DesignExtrudeFaceRole::Termination)
        ]
    );
    set_extrude_start(&mut scope, DesignExtrudeStart::ProfilePlane);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());

    let profile_offset = parameter("ProfileOffset", "mm", 0.1);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &profile_offset)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    set_extrude_start(&mut scope, DesignExtrudeStart::OffsetProfilePlane);
    let offset_start = project_extrude(
        &scope,
        &[(0, &along), (1, &profile_offset)],
        std::slice::from_ref(&body_group),
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed offset-profile-plane Extrude");
    assert!(matches!(
        offset_start,
        FeatureDefinition::Extrude {
            start: ExtrudeStart::OffsetProfilePlane {
                offset: Length(1.0)
            },
            ..
        }
    ));
    set_extrude_start(&mut scope, DesignExtrudeStart::ProfilePlane);

    set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
    let against = parameter("AgainstDistance", "mm", -0.05);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &against)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedDistance);
    let two_sided = project_extrude(
        &scope,
        &[(0, &along), (1, &against), (2, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed two-sided Extrude");
    assert!(matches!(
        two_sided,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(5.5)
                    },
                    ..
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(0.5)
                    },
                    draft: Some(Angle(-0.3)),
                    ..
                },
            },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, true);
    assert!(project_extrude(
        &scope,
        &[(0, &along), (1, &against), (2, &side_two_taper)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());
    set_extrude_direction_reversed(&mut scope, false);

    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
    let reversed_along = parameter("AlongDistance", "mm", -0.6);
    let reversed = project_extrude(
        &scope,
        &[(0, &reversed_along)],
        &[],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed reversed Extrude");
    assert!(matches!(
        reversed,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(6.0)
                    },
                    ..
                },
            },
            ..
        }
    ));

    set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);
    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedToFace);
    set_extrude_direction_reversed(&mut scope, true);
    face_group.extrude_role = Some(DesignExtrudeOperandRole::Faces(Some(
        DesignExtrudeFaceRole::Termination,
    )));
    let side_offset = parameter("Side1Offset", "mm", 0.025);
    let to_face = project_extrude(
        &scope,
        &[(0, &side_offset), (1, &taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed reversed to-face Extrude");
    assert!(matches!(
        to_face,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Native(ref id),
                        offset: Some(Length(0.25)),
                    },
                    ..
                },
            },
            ..
        } if id == &face_group.id
    ));

    let mut omitted_zero_offset_scope = scope.clone();
    omitted_zero_offset_scope.class_tag = "330".into();
    omitted_zero_offset_scope.paired_class_tag = "258".into();
    omitted_zero_offset_scope.frame_length = 476;
    let omitted_zero_offset = project_extrude(
        &omitted_zero_offset_scope,
        &[(0, &taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("class-330 face-target Extrude admits omitted zero offset");
    assert!(matches!(
        omitted_zero_offset,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Native(ref id),
                        offset: None,
                    },
                    ..
                },
            },
            ..
        } if id == &face_group.id
    ));
    omitted_zero_offset_scope.class_tag = "331".into();
    assert!(project_extrude(
        &omitted_zero_offset_scope,
        &[(0, &taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .is_none());

    set_extrude_direction_reversed(&mut scope, false);
    set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedToFaces);
    let mut second_face_group = face_group.clone();
    second_face_group.id = "f3d:Design/BulkStream.dat:operand-group#104".into();
    second_face_group.scope_reference_ordinal = 3;
    let second_side_offset = parameter("Side2Offset", "mm", 0.05);
    let two_sided_to_faces = project_extrude(
        &scope,
        &[
            (0, &side_offset),
            (1, &taper),
            (2, &second_side_offset),
            (3, &side_two_taper),
        ],
        &[
            body_group.clone(),
            face_group.clone(),
            second_face_group.clone(),
        ],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed two-sided face-target Extrude");
    assert!(matches!(
        two_sided_to_faces,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Native(ref first_id),
                        offset: Some(Length(0.25)),
                    },
                    draft: Some(Angle(0.2)),
                    ..
                },
                second: ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Native(ref second_id),
                        offset: Some(Length(0.5)),
                    },
                    draft: Some(Angle(-0.3)),
                    ..
                },
            },
            ..
        } if first_id == &face_group.id && second_id == &second_face_group.id
    ));

    set_extrude_direction_reversed(&mut scope, true);
    let reversed_two_sided_to_faces = project_extrude(
        &scope,
        &[
            (0, &side_offset),
            (1, &taper),
            (2, &second_side_offset),
            (3, &side_two_taper),
        ],
        &[
            body_group.clone(),
            face_group.clone(),
            second_face_group.clone(),
        ],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed reversed two-sided face-target Extrude");
    assert!(matches!(
        reversed_two_sided_to_faces,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal,
            extent: ExtrudeExtent::TwoSided { .. },
            ..
        }
    ));
    set_extrude_direction_reversed(&mut scope, false);

    set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedDistanceToFace);
    let mixed_two_sided = project_extrude(
        &scope,
        &[(0, &along), (1, &second_side_offset), (2, &side_two_taper)],
        &[body_group.clone(), face_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed mixed two-sided Extrude");
    assert!(matches!(
        mixed_two_sided,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::ProfileNormal,
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(5.5)
                    },
                    ..
                },
                second: ExtrudeSide {
                    termination: LinearTermination::ToFace {
                        face: FaceSelection::Native(ref id),
                        offset: Some(Length(0.5)),
                    },
                    draft: Some(Angle(-0.3)),
                    ..
                },
            },
            ..
        } if id == &face_group.id
    ));

    set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedToFace);
    set_extrude_start(&mut scope, DesignExtrudeStart::FromFace);
    let mut start_group = face_group.clone();
    start_group.id = "f3d:Design/BulkStream.dat:operand-group#103".into();
    start_group.extrude_role = Some(DesignExtrudeOperandRole::Faces(Some(
        DesignExtrudeFaceRole::Start,
    )));
    let from_face = project_extrude(
        &scope,
        &[
            (0, &parameter("ProfileOffset", "mm", 0.0)),
            (1, &side_offset),
            (2, &taper),
        ],
        &[body_group, start_group.clone(), face_group],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed selected-face start Extrude");
    assert!(matches!(
        from_face,
        FeatureDefinition::Extrude {
            start: ExtrudeStart::FromFace {
                face: FaceSelection::Native(ref id),
                offset: None,
            },
            ..
        } if id == &start_group.id
    ));

    set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
    set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedDistance);
    set_extrude_direction_reversed(&mut scope, false);
    let from_face_two_sided = project_extrude(
        &scope,
        &[
            (0, &parameter("ProfileOffset", "mm", 0.0)),
            (1, &along),
            (2, &against),
        ],
        &[start_group.clone()],
        &[],
        std::slice::from_ref(&placement),
        &[],
    )
    .expect("typed selected-face-start two-sided Extrude");
    assert!(matches!(
        from_face_two_sided,
        FeatureDefinition::Extrude {
            start: ExtrudeStart::FromFace {
                face: FaceSelection::Native(ref id),
                offset: None,
            },
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind { length: Length(5.5) },
                    ..
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind { length: Length(0.5) },
                    ..
                },
            },
            ..
        } if id == &start_group.id
    ));
}

#[test]
fn sketch_inputs_bind_owner_dependencies_after_sketch_conversion() {
    use cadmpeg_ir::features::{BooleanOp, LoftSection, PathRef, SheetMetalThicknessSide};
    use cadmpeg_ir::sketches::SpatialSketchId;

    let feature = |id: &str, ordinal, definition| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    let planar_sketch = SketchId("f3d:sketch:planar".into());
    let spatial_sketch = SpatialSketchId("f3d:sketch:spatial".into());
    let planar_feature = feature(
        "f3d:feature:planar-sketch",
        0,
        FeatureDefinition::Sketch {
            sketch: Some(planar_sketch.clone()),
        },
    );
    let spatial_feature = feature(
        "f3d:feature:spatial-sketch",
        1,
        FeatureDefinition::SpatialSketch {
            sketch: Some(spatial_sketch.clone()),
        },
    );
    let base_flange = feature(
        "f3d:feature:base-flange",
        2,
        FeatureDefinition::SheetMetalBaseFlange {
            profile: ProfileRef::Sketch(planar_sketch.clone()),
            thickness: Length(1.0),
            side: SheetMetalThicknessSide::Forward,
        },
    );
    let loft = feature(
        "f3d:feature:loft",
        3,
        FeatureDefinition::Loft {
            sections: vec![
                LoftSection::Profile(ProfileRef::SpatialSketchProfiles {
                    sketch: spatial_sketch.clone(),
                    profiles: vec![2],
                }),
                LoftSection::Profile(ProfileRef::SpatialSketchProfiles {
                    sketch: spatial_sketch.clone(),
                    profiles: vec![5],
                }),
            ],
            guides: vec![PathRef::SpatialSketchSelection {
                sketch: spatial_sketch,
                selections: vec!["f3d:native:guide".into()],
            }],
            centerline: Some(PathRef::Sketch(planar_sketch)),
            op: BooleanOp::Join,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        },
    );
    let expected_dependencies = [spatial_feature.id.clone(), planar_feature.id.clone()];
    let mut features = vec![planar_feature, spatial_feature, base_flange, loft];

    crate::design::feature_project::bind_sketch_feature_geometry(&mut features, &[], &[], &[], &[]);

    assert_eq!(features[2].dependencies, [features[0].id.clone()]);
    assert_eq!(features[3].dependencies, expected_dependencies);
}
