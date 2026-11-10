// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::project_extrude;
use crate::design::test_support::parameter_record;
use crate::ids::neutral_sketch_id;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

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

#[test]
fn sketch_text_extrude_projects_a_reversed_cut_of_the_text_entities() {
    use crate::design::feature_project::{project_extrude_projection, ExtrudeProjection};
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
        PlanarProfileRef, ProfileRef,
    };

    let parameter = |source_kind: &str, value| {
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            source_kind,
            Some("mm"),
            "d1",
            value,
        ))
        .expect("generated feature parameter is canonical")
    };
    let mut scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            payload: DesignScopePayload::Extrude(Some(
                crate::records::feature::scope::DesignExtrudeScope {
                    extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                        reference: None,
                        operation: DesignExtrudeOperation::NewBody,
                        operation_offset: 128,
                        direction_face_extend_values: [1, 2],
                        side_extent_discriminators: [1, 0],
                        side_extent_discriminator_offsets: [177, 190],
                        first_side_target_ordinal: None,
                        distance_tail: None,
                        extent: DesignExtrudeExtent::OneSidedDistance,
                        direction_face_extend_offsets: [132, 136],
                        direction_reversed: false,
                        direction_reversed_offset: 140,
                        solid_operation: true,
                        solid_operation_offset: 141,
                        start: DesignExtrudeStart::ProfilePlane,
                        start_offset: 142,
                    }),
                    extrude_profile: Some(
                        DesignSketchProfileOperand::try_new(
                            crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
                                scope_reference_ordinal: 0,
                                record_index: 100,
                                byte_offset: 300,
                                class_tag: crate::records::references::DesignClassTag::try_from(
                                    "308".to_owned(),
                                )
                                .unwrap(),
                                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                                    "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".to_owned(),
                                )
                                .unwrap(),
                                asset_id_offset: 330,
                                entity_id: crate::records::identity::DesignEntityId::try_from(
                                    "0_172".to_owned(),
                                )
                                .expect("valid entity identity"),
                                entity_reference_offset: 420,
                                compact_tail_flags: None,
                                region_selection: None,
                                paired_class_tag:
                                    crate::records::references::DesignClassTag::try_from(
                                        "259".to_owned(),
                                    )
                                    .unwrap(),
                                paired_byte_offset: 520,
                            },
                        )
                        .unwrap(),
                    ),
                    ..crate::records::feature::scope::DesignExtrudeScope::default()
                },
            )),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            label: None,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100],
                vec![185],
                "reference_members",
            )
            .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            deleted_members: None,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            600,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit(
                crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, -1.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .unwrap(),
            ),
        )
        .unwrap(),
        id: "f3d:Design/BulkStream.dat:placement#200".into(),
        scope_record_index: Some(11),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned())
            .expect("valid entity ID"),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 200,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned())
            .unwrap(),
    };
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
        scope.payload_mut()
    {
        let extrude = slot.as_mut().expect("extrude payload");
        extrude.extrude_profile = None;
        extrude.extrude_native_profiles = Some(Vec::new());
    }
    let along = parameter("AlongDistance", -0.02);
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project_extrude(
            decode_ctx,
            &scope,
            &[(0, &along)],
            &[],
            &[],
            std::slice::from_ref(&placement),
            &[],
        ))
        .expect("project the extrude feature")
        .is_none()
    );
    set_extrude_operation(&mut scope, DesignExtrudeOperation::Cut);
    let body_group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#101".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 1,
            record_index: 101,
            byte_offset: 1000,
            class_tag: crate::records::references::DesignClassTag::try_from("332".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 200,
                offset: 1026,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 300,
                        offset: 1044,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 180,
                    opaque_index_offset: 1072,
                    opaque_scalar: 0.125,
                    opaque_scalar_offset: 1076,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesB,
            role_offset: 1054,
            paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
                .unwrap(),
            paired_byte_offset: 1125,
        },
    )
    .unwrap();
    let sketch = neutral_sketch_id(&placement);
    let letters = [
        crate::ids::neutral_sketch_text_id(&sketch, 9996),
        crate::ids::neutral_sketch_text_id(&sketch, 10015),
    ];
    let text_profile = PlanarProfileRef::sketch_entities(sketch.clone(), letters.to_vec()).unwrap();
    let cut = crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude_projection(ExtrudeProjection {
            ctx: decode_ctx,
            scope: &scope,
            parameters: &[(0, &along)],
            construction_groups: std::slice::from_ref(&body_group),
            face_operands: &[],
            placements: std::slice::from_ref(&placement),
            body_recipe_operands: &[],
            text_profile: Some(text_profile),
        })
    })
    .expect("project the text extrude")
    .expect("typed text cut");
    assert!(matches!(
        &cut,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::SketchEntities { sketch: profile_sketch, entities }),
            direction: ExtrudeDirection::ReversedProfileNormal {},
            start: ExtrudeStart::ProfilePlane {},
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length },
                    draft: None,
                },
            },
            op: BooleanOp::Cut,
            solid: Some(true),
            ..
        }) if profile_sketch == &sketch
            && entities.as_slice() == letters.as_slice()
            && length.get() == 0.2
    ));
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project_extrude_projection(
            ExtrudeProjection {
                ctx: decode_ctx,
                scope: &scope,
                parameters: &[(0, &along)],
                construction_groups: &[],
                face_operands: &[],
                placements: std::slice::from_ref(&placement),
                body_recipe_operands: &[],
                text_profile: Some(
                    PlanarProfileRef::sketch_entities(sketch, letters.to_vec()).unwrap()
                ),
            }
        ))
        .expect("project the text extrude")
        .is_none(),
        "a cut with no body operand stays unprojected"
    );
    let confirmed = [
        crate::design::decode::operands::ConfirmedSketchTextExtrude {
            scope_id: scope.id.clone(),
            text_records: vec![9996],
        },
    ];
    let timelines =
        crate::design::test_support::synthetic_feature_timelines(std::slice::from_ref(&scope));
    let projected = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: std::slice::from_ref(&scope),
                timelines: &timelines,
                sketch_text_extrudes: &confirmed,
                ..Default::default()
            },
        )
    })
    .expect("project the confirmed text extrude");
    let skip = projected
        .projection_losses
        .iter()
        .find(|loss| {
            loss.code
                == crate::loss::F3dLossCode::FeatureExtrudeSketchTextSkipped
                    .note("1 sketch-text extrude profile(s) were skipped.")
                    .code
        })
        .expect("an unprojected sketch-text extrude emits the skip loss");
    assert_eq!(
        skip.message,
        "1 sketch-text extrude profile(s) were skipped."
    );
    assert!(matches!(
        projected.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
            if kind.as_str() == "SketchTextExtrude"
    ));
}
