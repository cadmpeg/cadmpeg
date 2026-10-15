// SPDX-License-Identifier: Apache-2.0
//! Completeness predicates for projected design feature definitions.

use super::super::feature_definition_is_incomplete;

#[test]
fn untyped_material_distances_charge_one_loss_without_fabricating_geometry() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    };

    super::super::report_untyped_material_distances(&ctx, &mut report, 0).unwrap();
    assert!(report.losses.is_empty());
    super::super::report_untyped_material_distances(&ctx, &mut report, 2).unwrap();

    assert_eq!(report.losses.len(), 1);
    assert_eq!(
        report.losses[0].code.local_code(),
        "material.distance-unit-untyped"
    );
}

#[test]
fn untyped_material_distance_loss_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = super::super::report_untyped_material_distances(&ctx, &mut report, 1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D decode losses")
    );
}

#[test]
fn act_component_link_loss_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error =
        super::super::report_unretained_act_component_links(&ctx, &mut report, 1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D decode losses")
    );
}

#[test]
fn configuration_member_loss_refuses_collection_limit() {
    let configuration = crate::test_support::with_decode_context(|ctx| {
        crate::records::configuration::DesignConfiguration::try_new_charged(
            ctx,
            "table.dsgcfg".into(),
            crate::records::configuration::DesignConfigurationKind::Table,
            Vec::new(),
            serde_json::Map::from_iter([("unknown".into(), serde_json::Value::Bool(true))]),
        )
    })
    .unwrap();
    let native = crate::native::F3dNative {
        design_configurations: vec![configuration],
        ..Default::default()
    };
    let ir = cadmpeg_ir::document::CadIr::empty();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error =
        super::super::report_unresolved_configuration_rules(&ctx, &mut report, &native, &ir)
            .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D decode losses")
    );
}

#[test]
fn configuration_member_variant_scan_refuses_work_limit() {
    let configuration = crate::test_support::with_decode_context(|ctx| {
        crate::records::configuration::DesignConfiguration::try_new_charged(
            ctx,
            "table.dsgcfg".into(),
            crate::records::configuration::DesignConfigurationKind::Table,
            vec!["variant".into()],
            serde_json::from_value(serde_json::json!({
                "configurations": {"variant": {"unknown": true}}
            }))
            .unwrap(),
        )
    })
    .unwrap();
    let native = crate::native::F3dNative {
        design_configurations: vec![configuration],
        ..Default::default()
    };
    let ir = cadmpeg_ir::document::CadIr::empty();
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D configuration variants for members",
        0,
        |ctx| {
            let mut report = cadmpeg_ir::codec::DecodeBody::new(
                cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
            );
            super::super::report_unresolved_configuration_rules(ctx, &mut report, &native, &ir)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "scan F3D configuration variants for members"
    ));
}

#[test]
fn direct_datum_planes_are_complete_but_unresolved_frames_are_not() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};
    use cadmpeg_ir::math::{Point3, Vector3};

    let direct = FeatureDefinition::Operation(FeatureOperation::DatumPlane {
        frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    });
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode, &direct
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane
            })
        ))
        .expect("completeness admission")
    );
}

#[test]
fn trim_surface_completeness_accepts_an_explicit_cell_selection() {
    let complete = cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::TrimSurface {
            faces: cadmpeg_ir::features::FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
                "f3d:test:face#target",
            )
            .expect("identity grammar")]),
            tool: cadmpeg_ir::features::PathRef::Curves(vec![cadmpeg_ir::ids::CurveId::mint(
                "f3d:test:curve#tool",
            )
            .expect("identity grammar")]),
            keep: cadmpeg_ir::features::TrimRegion::Cells(
                cadmpeg_ir::features::TrimCellSelection::new(vec![1, 4], 5).unwrap(),
            ),
        },
    );
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode, &complete
        ))
        .expect("completeness admission")
    );
}

#[test]
fn datum_axes_require_a_finite_nonzero_direction() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::DatumAxis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                    0.0, 0.0, 1.0
                ))
                .unwrap(),
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::DatumAxis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                    f64::EPSILON / 2.0,
                    0.0,
                    0.0
                ))
                .unwrap(),
            })
        ))
        .expect("completeness admission")
    );
}

#[test]
fn coordinate_systems_require_a_finite_right_handed_frame() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem {
                frame: cadmpeg_ir::features::FeatureCoordinateFrame::new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0)
                )
                .unwrap()
            })
        ))
        .expect("completeness admission")
    );
}

#[test]
fn zero_body_base_features_are_complete_but_empty_insertions_are_not() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: Default::default(),
                    native: "native:base-feature".into(),
                },
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("native:base-feature".into()),
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::InsertBodies {
                bodies: cadmpeg_ir::features::InsertedBodies::Native("native:insert-bodies".into()),
            })
        ))
        .expect("completeness admission")
    );
}

#[test]
fn replace_face_requires_resolved_target_and_replacement_faces() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::FaceId;

    let resolved = |name: &str| {
        FaceSelection::Faces(vec![
            FaceId::mint(format!("test:model:face#{name}")).expect("identity grammar")
        ])
    };
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
                operands: cadmpeg_ir::features::ReplaceFaceOperands::new(
                    resolved("target"),
                    resolved("replacement"),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("operand admission")
                .unwrap(),
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
                operands: cadmpeg_ir::features::ReplaceFaceOperands::new(
                    FaceSelection::Native("native:target".into()),
                    resolved("replacement"),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("operand admission")
                .unwrap(),
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
                operands: cadmpeg_ir::features::ReplaceFaceOperands::new(
                    resolved("target"),
                    FaceSelection::Native("native:replacement".into()),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("operand admission")
                .unwrap(),
            })
        ))
        .expect("completeness admission")
    );
}

#[test]
fn remove_body_requires_resolved_bodies_and_a_retention_mode() {
    use cadmpeg_ir::features::{
        BodyRetentionMode, BodySelection, FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;

    let complete = FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Bodies(
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![BodyId::mint("test:model:body#1").expect("identity grammar")],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("distinct bodies"),
        ),
        mode: BodyRetentionMode::DeleteSelected,
    });
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode, &complete
        ))
        .expect("completeness admission")
    );

    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Native("native:remove-body".into()),
                mode: BodyRetentionMode::DeleteSelected,
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Bodies(
                    cadmpeg_ir::features::DistinctMembers::try_from(
                        vec![BodyId::mint("test:model:body#1").expect("identity grammar")],
                        &cadmpeg_test_support::service_decode_context()
                    )
                    .expect("distinct bodies")
                ),
                mode: BodyRetentionMode::Unresolved,
            })
        ))
        .expect("completeness admission")
    );
}

#[test]
fn product_feature_definitions_require_neutral_reference_ids() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::JointId;

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                occurrence: OccurrenceId::mint("model:test:occurrence#component")
                    .expect("identity grammar"),
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::AssemblyJoint {
                joint: JointId::mint("model:test:joint#assembly").expect("identity grammar"),
            })
        ))
        .expect("completeness admission")
    );
    assert!(OccurrenceId::mint(String::new()).is_err());
    assert!(JointId::mint(String::new()).is_err());
}

#[test]
fn direct_and_analytic_features_require_resolved_geometry_and_operands() {
    use cadmpeg_ir::features::{
        AxisAngle, BodySelection, BooleanOp, FaceMotion, FaceSelection, FeatureDefinition,
        FeatureOperation, ScaleCenter, ScaleFactors, ThickenSide,
    };
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::Length;

    let faces = FaceSelection::Faces(vec!["test:model:face#1"
        .try_into()
        .expect("valid identity")]);
    let bodies = BodySelection::Bodies(
        cadmpeg_ir::features::DistinctMembers::try_from(
            vec![BodyId::mint("test:model:body#1").expect("identity grammar")],
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("distinct bodies"),
    );

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::Sphere {
                center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .unwrap(),
                radius: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
                op: BooleanOp::NewBody,
            })
        ))
        .expect("completeness admission")
    );

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::Torus {
                center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .unwrap(),
                axis: cadmpeg_ir::units::UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
                major_radius: cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
                minor_radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
                op: BooleanOp::Join,
            })
        ))
        .expect("completeness admission")
    );

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::MoveFace {
                faces: faces.clone(),
                motion: FaceMotion::Offset {
                    distance: Length::new(-2.0).unwrap(),
                },
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::MoveFace {
                faces: FaceSelection::Native("native:faces".into()),
                motion: FaceMotion::Offset {
                    distance: Length::new(2.0).unwrap(),
                },
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces: faces.clone(),
                thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap()),
                side: Some(ThickenSide::Forward),
            })
        ))
        .expect("completeness admission")
    );

    let shell = |bodies, removed_faces| {
        FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies,
            removed_faces,
            thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap()),
            outward: Some(true),
            mode: None,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        })
    };
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &shell(Some(bodies.clone()), FaceSelection::Faces(Vec::new()),)
        ))
        .expect("completeness admission")
    );
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &shell(
                None,
                FaceSelection::Faces(vec!["test:model:face#opening"
                    .try_into()
                    .expect("valid identity")]),
            )
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &shell(None, FaceSelection::Faces(Vec::new()),)
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &shell(
                Some(BodySelection::Native("native:bodies".into())),
                FaceSelection::Faces(Vec::new()),
            )
        ))
        .expect("completeness admission")
    );

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::MoveBody {
                bodies: bodies.clone(),
                translation: cadmpeg_ir::features::FiniteVector3::new(Vector3::new(1.0, 2.0, 3.0))
                    .unwrap(),
                rotation: Some(AxisAngle {
                    origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .unwrap(),
                    direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0
                    ))
                    .unwrap(),
                    angle: cadmpeg_ir::scalar::Angle::new(0.5).unwrap(),
                }),
                copies: 0,
            })
        ))
        .expect("completeness admission")
    );

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::Scale {
                bodies: BodySelection::Bodies(
                    cadmpeg_ir::features::DistinctMembers::try_from(
                        vec![BodyId::mint("test:model:body#scale").expect("identity grammar")],
                        &cadmpeg_test_support::service_decode_context()
                    )
                    .expect("distinct bodies")
                ),
                center: Some(ScaleCenter::ModelOrigin),
                factors: ScaleFactors::Uniform {
                    factor: cadmpeg_ir::scalar::NonZeroReal::new(1.5).unwrap()
                },
            })
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &FeatureDefinition::Operation(FeatureOperation::Scale {
                bodies: BodySelection::Bodies(
                    cadmpeg_ir::features::DistinctMembers::try_from(
                        vec![BodyId::mint("test:model:body#scale").expect("identity grammar")],
                        &cadmpeg_test_support::service_decode_context()
                    )
                    .expect("distinct bodies")
                ),
                center: Some(ScaleCenter::Native("native:center".into())),
                factors: ScaleFactors::Uniform {
                    factor: cadmpeg_ir::scalar::NonZeroReal::new(1.5).unwrap()
                },
            })
        ))
        .expect("completeness admission")
    );
}

#[test]
fn knit_surfaces_require_resolved_faces_and_operation_settings() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::scalar::NonNegativeLength;

    let complete = |faces, merge_entities, create_solid, gap_tolerance| {
        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
            faces,
            merge_entities,
            create_solid,
            gap_tolerance,
        })
    };
    let faces = FaceSelection::Faces(vec!["test:model:face#1"
        .try_into()
        .expect("valid identity")]);

    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &complete(
                faces.clone(),
                Some(true),
                Some(true),
                Some(NonNegativeLength::new(0.1).unwrap()),
            )
        ))
        .expect("completeness admission")
    );
    assert!(
        !crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &complete(
                faces.clone(),
                Some(false),
                Some(false),
                Some(NonNegativeLength::new(0.1).unwrap()),
            )
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &complete(
                FaceSelection::Native("native:surface-stitch".into()),
                Some(true),
                Some(true),
                Some(NonNegativeLength::new(0.1).unwrap()),
            )
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &complete(
                faces.clone(),
                None,
                Some(true),
                Some(NonNegativeLength::new(0.1).unwrap()),
            )
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &complete(
                faces.clone(),
                Some(true),
                None,
                Some(NonNegativeLength::new(0.1).unwrap()),
            )
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &complete(
                faces.clone(),
                Some(true),
                Some(true),
                Some(NonNegativeLength::new(0.0).unwrap()),
            )
        ))
        .expect("completeness admission")
    );
    assert!(
        crate::test_support::with_decode_context(|decode| feature_definition_is_incomplete(
            decode,
            &complete(faces, Some(true), Some(true), None,)
        ))
        .expect("completeness admission")
    );
}

#[test]
fn datum_point_completeness_needs_no_work_for_fixed_plane_array() {
    use cadmpeg_ir::features::{
        DatumPlaneReference, DatumPointConstruction, FeatureDefinition, FeatureId,
        FeatureOperation, FinitePoint3,
    };
    let definition = FeatureDefinition::Operation(FeatureOperation::DatumPoint {
        position: FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap(),
        construction: Some(Box::new(DatumPointConstruction::ThreePlaneIntersection {
            planes: Box::new(std::array::from_fn(|index| DatumPlaneReference::Feature {
                feature: FeatureId::mint(format!("test:model:feature#plane:{index}")).unwrap(),
            })),
        })),
    });
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (decode, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!feature_definition_is_incomplete(&decode, &definition).unwrap());
    decode.finish_session().unwrap();
}

#[test]
fn loft_completeness_searches_ignore_unvisited_sections_and_guides() {
    let definition: cadmpeg_ir::features::FeatureDefinition =
        serde_json::from_value(serde_json::json!({
            "definition": "loft",
            "sections": [
                {"kind": "native", "value": "native:profile:first"},
                {"kind": "native", "value": "native:profile:second"},
                {"kind": "native", "value": "native:profile:third"}
            ],
            "guidance": {"kind": "guides", "path": [
                {"kind": "native", "value": "native:guide:first"},
                {"kind": "native", "value": "native:guide:second"},
                {"kind": "native", "value": "native:guide:third"}
            ]},
            "op": "join"
        }))
        .unwrap();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // Each search stops at its first incomplete item.
    policy.limits.max_work_units = 2;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (decode, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(feature_definition_is_incomplete(&decode, &definition).unwrap());
    decode.finish_session().unwrap();
}

#[test]
fn draft_neutral_plane_comparison_preserves_work_refusal() {
    use cadmpeg_ir::features::{FaceSelection, FeatureId};
    let id = FeatureId::mint("test:model:feature#plane").unwrap();
    let selection = FaceSelection::Native("test:model:feature#plane".into());
    let direction = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    assert!(crate::test_support::with_decode_context(|decode| {
        super::super::draft_neutral_plane_is_resolved(
            decode,
            &selection,
            Some(&id),
            Some(&direction),
        )
    })
    .unwrap());
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "compare F3D Draft neutral plane",
        0,
        |decode| {
            super::super::draft_neutral_plane_is_resolved(
                decode,
                &selection,
                Some(&id),
                Some(&direction),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "compare F3D Draft neutral plane")
    );
}
