// SPDX-License-Identifier: Apache-2.0
//! Completeness predicates for projected design feature definitions.

use super::super::feature_definition_is_incomplete;

#[test]
fn untyped_material_distances_charge_one_loss_without_fabricating_geometry() {
    let mut report = cadmpeg_ir::report::DecodeReport::unclassified(
        "f3d",
        false,
        true,
        std::collections::BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        cadmpeg_ir::report::TransferLedger::default(),
    );

    super::super::report_untyped_material_distances(&mut report, 0);
    assert!(report.losses.is_empty());
    super::super::report_untyped_material_distances(&mut report, 2);

    assert_eq!(report.losses.len(), 1);
    assert_eq!(report.losses[0].code.code, "material.distance-unit-untyped");
}

#[test]
fn direct_datum_planes_are_complete_but_unresolved_frames_are_not() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let direct = FeatureDefinition::DatumPlane {
        origin: Point3::new(1.0, 2.0, 3.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    assert!(!feature_definition_is_incomplete(&direct));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::DatumPlane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::DatumPlaneUnresolved
    ));
}

#[test]
fn trim_surface_completeness_accepts_an_explicit_cell_selection() {
    let complete = cadmpeg_ir::features::FeatureDefinition::TrimSurface {
        faces: cadmpeg_ir::features::FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId(
            "face:target".into(),
        )]),
        tool: cadmpeg_ir::features::PathRef::Curves(vec![cadmpeg_ir::ids::CurveId(
            "curve:tool".into(),
        )]),
        keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        cell_selection: Some(cadmpeg_ir::features::TrimCellSelection {
            removed: vec![1, 4],
            total: 5,
        }),
    };
    assert!(!feature_definition_is_incomplete(&complete));

    let conflicting = cadmpeg_ir::features::FeatureDefinition::TrimSurface {
        faces: cadmpeg_ir::features::FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId(
            "face:target".into(),
        )]),
        tool: cadmpeg_ir::features::PathRef::Curves(vec![cadmpeg_ir::ids::CurveId(
            "curve:tool".into(),
        )]),
        keep: cadmpeg_ir::features::TrimRegion::Inside,
        cell_selection: Some(cadmpeg_ir::features::TrimCellSelection {
            removed: vec![1, 4],
            total: 5,
        }),
    };
    assert!(feature_definition_is_incomplete(&conflicting));

    let invalid = cadmpeg_ir::features::FeatureDefinition::TrimSurface {
        faces: cadmpeg_ir::features::FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId(
            "face:target".into(),
        )]),
        tool: cadmpeg_ir::features::PathRef::Curves(vec![cadmpeg_ir::ids::CurveId(
            "curve:tool".into(),
        )]),
        keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        cell_selection: Some(cadmpeg_ir::features::TrimCellSelection {
            removed: vec![6],
            total: 5,
        }),
    };
    assert!(feature_definition_is_incomplete(&invalid));
}

#[test]
fn datum_axes_require_a_finite_nonzero_direction() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::DatumAxis {
            origin: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::DatumAxis {
            origin: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(0.0, 0.0, 0.0),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::DatumAxis {
            origin: Point3::new(f64::NAN, 2.0, 3.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        }
    ));
}

#[test]
fn coordinate_systems_require_a_finite_right_handed_frame() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::DatumCoordinateSystem {
            origin: Point3::new(1.0, 2.0, 3.0),
            x_axis: Vector3::new(1.0, 0.0, 0.0),
            y_axis: Vector3::new(0.0, 1.0, 0.0),
            z_axis: Vector3::new(0.0, 0.0, 1.0),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::DatumCoordinateSystem {
            origin: Point3::new(1.0, 2.0, 3.0),
            x_axis: Vector3::new(1.0, 0.0, 0.0),
            y_axis: Vector3::new(0.0, 1.0, 0.0),
            z_axis: Vector3::new(0.0, 0.0, -1.0),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::DatumCoordinateSystem {
            origin: Point3::new(1.0, 2.0, 3.0),
            x_axis: Vector3::new(2.0, 0.0, 0.0),
            y_axis: Vector3::new(0.0, 1.0, 0.0),
            z_axis: Vector3::new(0.0, 0.0, 1.0),
        }
    ));
}

#[test]
fn zero_body_base_features_are_complete_but_empty_insertions_are_not() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::BaseFeature {
            bodies: BodySelection::Resolved {
                bodies: Vec::new(),
                native: "native:base-feature".into(),
            },
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::BaseFeature {
            bodies: BodySelection::Native("native:base-feature".into()),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::InsertBodies {
            bodies: BodySelection::Resolved {
                bodies: Vec::new(),
                native: "native:insert-bodies".into(),
            },
        }
    ));
}

#[test]
fn replace_face_requires_resolved_target_and_replacement_faces() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};
    use cadmpeg_ir::ids::FaceId;

    let resolved = |name: &str| FaceSelection::Faces(vec![FaceId(name.to_owned())]);
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::ReplaceFace {
            targets: resolved("target"),
            replacements: resolved("replacement"),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::ReplaceFace {
            targets: FaceSelection::Native("native:target".into()),
            replacements: resolved("replacement"),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::ReplaceFace {
            targets: resolved("target"),
            replacements: FaceSelection::Native("native:replacement".into()),
        }
    ));
}

#[test]
fn remove_body_requires_resolved_bodies_and_a_retention_mode() {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;

    let complete = FeatureDefinition::DeleteBody {
        bodies: BodySelection::Bodies(vec![BodyId("body:1".into())]),
        mode: BodyRetentionMode::DeleteSelected,
    };
    assert!(!feature_definition_is_incomplete(&complete));

    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::DeleteBody {
            bodies: BodySelection::Native("native:remove-body".into()),
            mode: BodyRetentionMode::DeleteSelected,
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::DeleteBody {
            bodies: BodySelection::Bodies(vec![BodyId("body:1".into())]),
            mode: BodyRetentionMode::Unresolved,
        }
    ));
}

#[test]
fn product_feature_definitions_require_neutral_reference_ids() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::JointId;

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::InsertComponent {
            occurrence: OccurrenceId("model:occurrence#component".into()),
        }
    ));
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::AssemblyJoint {
            joint: JointId("model:joint#assembly".into()),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::InsertComponent {
            occurrence: OccurrenceId(String::new()),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::AssemblyJoint {
            joint: JointId(String::new()),
        }
    ));
}

#[test]
fn direct_and_analytic_features_require_resolved_geometry_and_operands() {
    use cadmpeg_ir::features::{
        AxisAngle, BodySelection, BooleanOp, FaceMotion, FaceSelection, FeatureDefinition, Length,
        ScaleCenter, ScaleFactors, ThickenSide,
    };
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let faces = FaceSelection::Faces(vec!["face:1".into()]);
    let bodies = BodySelection::Bodies(vec![BodyId("body:1".into())]);

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Sphere {
            center: Point3::new(1.0, 2.0, 3.0),
            radius: Length(4.0),
            op: BooleanOp::NewBody,
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Sphere {
            center: Point3::new(1.0, 2.0, 3.0),
            radius: Length(0.0),
            op: BooleanOp::NewBody,
        }
    ));
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Torus {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_radius: Length(8.0),
            minor_radius: Length(2.0),
            op: BooleanOp::Join,
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Torus {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 0.0),
            major_radius: Length(8.0),
            minor_radius: Length(2.0),
            op: BooleanOp::Join,
        }
    ));

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::MoveFace {
            faces: faces.clone(),
            motion: FaceMotion::Offset {
                distance: Length(-2.0),
            },
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::MoveFace {
            faces: FaceSelection::Native("native:faces".into()),
            motion: FaceMotion::Offset {
                distance: Length(2.0),
            },
        }
    ));
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Thicken {
            faces: faces.clone(),
            thickness: Some(Length(2.0)),
            side: Some(ThickenSide::Forward),
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Thicken {
            faces,
            thickness: Some(Length(0.0)),
            side: Some(ThickenSide::Forward),
        }
    ));

    let shell = |bodies, removed_faces| FeatureDefinition::Shell {
        bodies,
        removed_faces,
        thickness: Some(Length(1.0)),
        outward: Some(true),
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    };
    assert!(!feature_definition_is_incomplete(&shell(
        Some(bodies.clone()),
        FaceSelection::Faces(Vec::new()),
    )));
    assert!(!feature_definition_is_incomplete(&shell(
        None,
        FaceSelection::Faces(vec!["face:opening".into()]),
    )));
    assert!(feature_definition_is_incomplete(&shell(
        None,
        FaceSelection::Faces(Vec::new()),
    )));
    assert!(feature_definition_is_incomplete(&shell(
        Some(BodySelection::Native("native:bodies".into())),
        FaceSelection::Faces(Vec::new()),
    )));

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::MoveBody {
            bodies: bodies.clone(),
            translation: Vector3::new(1.0, 2.0, 3.0),
            rotation: Some(AxisAngle {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
                angle: cadmpeg_ir::features::Angle(0.5),
            }),
            copies: 0,
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::MoveBody {
            bodies,
            translation: Vector3::new(f64::NAN, 0.0, 0.0),
            rotation: None,
            copies: 0,
        }
    ));

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Scale {
            bodies: BodySelection::Bodies(vec![BodyId("body:scale".into())]),
            center: Some(ScaleCenter::ModelOrigin),
            factors: ScaleFactors {
                uniform: Some(1.5),
                x: None,
                y: None,
                z: None,
            },
        }
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Scale {
            bodies: BodySelection::Bodies(vec![BodyId("body:scale".into())]),
            center: Some(ScaleCenter::Native("native:center".into())),
            factors: ScaleFactors {
                uniform: Some(1.5),
                x: None,
                y: None,
                z: None,
            },
        }
    ));
}

#[test]
fn knit_surfaces_require_resolved_faces_and_operation_settings() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let complete =
        |faces, merge_entities, create_solid, gap_tolerance| FeatureDefinition::KnitSurface {
            faces,
            merge_entities,
            create_solid,
            gap_tolerance,
        };
    let faces = FaceSelection::Faces(vec!["face:1".into()]);

    assert!(!feature_definition_is_incomplete(&complete(
        faces.clone(),
        Some(true),
        Some(true),
        Some(Length(0.1)),
    )));
    assert!(!feature_definition_is_incomplete(&complete(
        faces.clone(),
        Some(false),
        Some(false),
        Some(Length(0.1)),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        FaceSelection::Native("native:surface-stitch".into()),
        Some(true),
        Some(true),
        Some(Length(0.1)),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        faces.clone(),
        None,
        Some(true),
        Some(Length(0.1)),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        faces.clone(),
        Some(true),
        None,
        Some(Length(0.1)),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        faces.clone(),
        Some(true),
        Some(true),
        Some(Length(0.0)),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        faces,
        Some(true),
        Some(true),
        None,
    )));
}
