// SPDX-License-Identifier: Apache-2.0
//! Typed-feature design-completeness audits.
#![allow(clippy::unwrap_used)]

use super::super::*;
use cadmpeg_ir::ids::{BodyId, EdgeId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use cadmpeg_ir::{
    features::{
        BodyRetentionMode, BodySelection, BooleanOp, DesignParameter, EdgeSelection, FaceSelection,
        Feature, FeatureDefinition, FeatureId, FeatureOperation, FeatureSourceContent,
        FeatureTreeNodeRole, ParameterId, PathRef, PatternKind, PatternTransform, RadiusSpec,
        RuledSurfaceMode, SurfaceContinuity, UnresolvedFamily,
    },
    scalar::{Angle, Length},
};
use std::collections::BTreeMap;

#[test]
fn design_completeness_rejects_unresolved_and_unaudited_typed_families() {
    let mut ir = CadIr::empty();
    let feature = |id: &str, ordinal, definition| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    ir.model.features.push(feature(
        "synthetic:test:id#complete-helix",
        0,
        FeatureDefinition::Operation(FeatureOperation::Helix {
            axis_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .unwrap(),
            axis_direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .unwrap(),
            radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
            shape: cadmpeg_ir::features::HelixShape::Cylindrical {
                pitch: cadmpeg_ir::scalar::NonZeroLength::new(2.0).unwrap(),
            },
            revolutions: cadmpeg_ir::scalar::PositiveReal::new(3.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            clockwise: false,
            segment_turns: None,
            construction_style: None,
        }),
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#incomplete-dome",
        1,
        FeatureDefinition::Operation(FeatureOperation::Dome {
            faces: FaceSelection::Native("face".into()),
            height: None,
            elliptical: None,
            reverse: None,
        }),
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#unresolved-plane",
        2,
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }),
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#unaudited-stored-geometry",
        3,
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
    ));
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "3 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_audits_direct_body_and_shape_families() {
    let mut ir = CadIr::empty();
    let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
    let other_body = BodyId::mint("test:model:entity#other-body").expect("identity grammar");
    let source = FeatureId::mint("synthetic:test:id#base").expect("identity grammar");
    let mut push = |id: &str, ordinal, dependencies: Vec<FeatureId>, outputs, definition| {
        ir.model.features.push(Feature {
            id: FeatureId::mint(id).expect("identity grammar"),
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies: (dependencies).try_into().unwrap(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(definition, outputs),
            native_ref: None,
        });
    };
    push(
        "synthetic:test:id#base",
        0,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Bodies(vec![body.clone()]),
        }),
    );
    push(
        "synthetic:test:id#stored",
        1,
        Vec::new(),
        vec![body.clone()],
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
    );
    push(
        "synthetic:test:id#derived",
        2,
        vec![source.clone()],
        Vec::new(),
        FeatureDefinition::Operation(FeatureOperation::DerivedGeometry { source }),
    );
    push(
        "synthetic:test:id#mirror",
        3,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::Operation(FeatureOperation::MirrorShape {
            source: BodySelection::Bodies(vec![body.clone()]),
            plane_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .unwrap(),
            plane_normal: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                .unwrap(),
            plane_reference: Some(FaceSelection::Native("plane".into())),
        }),
    );
    push(
        "synthetic:test:id#sew",
        4,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: (BodySelection::Bodies(vec![body.clone(), other_body.clone()]))
                .try_into()
                .unwrap(),
            gap_tolerance: None,
        }),
    );
    push(
        "synthetic:test:id#trim",
        5,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Bodies(vec![body.clone()]),
                BodySelection::Bodies(vec![other_body.clone()]),
            )
            .unwrap(),

            keep: cadmpeg_ir::features::BodyTrimSide::Unresolved,
        }),
    );
    push(
        "synthetic:test:id#import",
        6,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::Operation(FeatureOperation::ImportedGeometry {
            path: "  ".to_owned().try_into().unwrap(),
            format: cadmpeg_ir::features::GeometryImportFormat::Step,
        }),
    );
    push(
        "synthetic:test:id#section",
        7,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::Operation(FeatureOperation::SectionShape {
            operands: cadmpeg_ir::features::SectionOperands::new(
                BodySelection::Bodies(vec![body]),
                BodySelection::Bodies(vec![other_body]),
            )
            .unwrap(),

            approximate: None,
        }),
    );
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "5 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_audits_typed_construction_families() {
    let mut ir = CadIr::empty();
    let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
    let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
        "test:model:entity#face",
    )
    .expect("identity grammar")]);
    let definitions = [
        FeatureDefinition::Operation(FeatureOperation::PointGeometry {
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
        }),
        FeatureDefinition::Operation(FeatureOperation::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::new(
                cadmpeg_ir::features::PrimitiveSolidKind::Box {
                    length: Length::new(1.0).unwrap(),
                    width: Length::new(2.0).unwrap(),
                    height: Length::new(3.0).unwrap(),
                },
            )
            .unwrap(),
            op: BooleanOp::NewBody,
        }),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            profile: cadmpeg_ir::features::PlanarProfileRef::Sketch(sketch),
            thickness: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
            side: cadmpeg_ir::features::SheetMetalThicknessSide::Symmetric,
        }),
        FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        }),
        FeatureDefinition::Operation(FeatureOperation::ProjectOnSurface {
            sources: PathRef::Native("sources".into()),
            support_face: face.clone(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                .unwrap(),
            mode: cadmpeg_ir::features::SurfaceProjectionMode::All,
            height: cadmpeg_ir::scalar::NonNegativeLength::new(0.0).unwrap(),
            offset: Length::new(0.0).unwrap(),
        }),
        FeatureDefinition::Operation(FeatureOperation::Coil {
            construction: cadmpeg_ir::features::CoilConstruction {
                placement: cadmpeg_ir::features::CoilPlacement::Native {
                    native_ref: cadmpeg_ir::features::SelectionReference::try_from(String::from(
                        "placement",
                    ))
                    .unwrap(),
                },
                diameter: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
                extent: cadmpeg_ir::features::CoilExtent::RevolutionsHeight {
                    revolutions: cadmpeg_ir::scalar::PositiveReal::new(2.0).unwrap(),
                    height: Length::new(5.0).unwrap(),
                },
                section: cadmpeg_ir::features::CoilSection::Circular {
                    diameter: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
                },
                section_placement: cadmpeg_ir::features::CoilSectionPlacement::Center,
                clockwise: false,
                taper: Angle::new(0.0).unwrap(),
            },
            result: cadmpeg_ir::features::CoilResult::NewBody {},
        }),
        FeatureDefinition::Operation(FeatureOperation::Sphere {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
            op: BooleanOp::Unresolved,
        }),
        FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                face.clone(),
                FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
                    "test:model:entity#other-face",
                )
                .expect("identity grammar")]),
            )
            .unwrap(),

            radius: RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Variable),
            },
        }),
        FeatureDefinition::Operation(FeatureOperation::BoundaryFill {
            tools: BodySelection::Bodies(vec![body]),
            cells: cadmpeg_ir::features::NonEmptyMembers::one(BodySelection::Unresolved),
        }),
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:id#construction-{ordinal}"))
                .expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "6 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn binder_completeness_requires_resolved_targets_and_shape_arity() {
    let mut ir = CadIr::empty();
    let source = FeatureId::mint("synthetic:test:id#source").expect("identity grammar");
    let feature = |id: &str, ordinal, dependencies: Vec<FeatureId>, definition| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: (dependencies).try_into().unwrap(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    ir.model.features.push(feature(
        "synthetic:test:id#source",
        0,
        Vec::new(),
        FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: cadmpeg_ir::features::TreeChildren::default(),
        }),
    ));
    let shape = |sources| {
        FeatureDefinition::Operation(FeatureOperation::Binder {
            sources,
            construction: cadmpeg_ir::features::BinderConstruction::Shape {
                trace_support: false,
            },
        })
    };
    ir.model.features.push(feature(
        "synthetic:test:id#complete",
        1,
        vec![source.clone()],
        shape(vec![cadmpeg_ir::features::BinderSource {
            target: cadmpeg_ir::features::BinderTarget::Feature {
                feature: source.clone(),
            },
            subelements: vec![cadmpeg_ir::NonEmptyString::new("Face1").unwrap()],
        }]),
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#native",
        2,
        Vec::new(),
        shape(vec![cadmpeg_ir::features::BinderSource {
            target: cadmpeg_ir::features::BinderTarget::Native {
                reference: cadmpeg_ir::NonEmptyString::new("source").unwrap(),
            },
            subelements: Vec::new(),
        }]),
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#multiple-shape-sources",
        3,
        Vec::new(),
        shape(vec![
            cadmpeg_ir::features::BinderSource {
                target: cadmpeg_ir::features::BinderTarget::External {
                    document: cadmpeg_ir::NonEmptyString::new("a.FCStd").unwrap(),
                    object: cadmpeg_ir::NonEmptyString::new("Body").unwrap(),
                },
                subelements: Vec::new(),
            },
            cadmpeg_ir::features::BinderSource {
                target: cadmpeg_ir::features::BinderTarget::External {
                    document: cadmpeg_ir::NonEmptyString::new("b.FCStd").unwrap(),
                    object: cadmpeg_ir::NonEmptyString::new("Body").unwrap(),
                },
                subelements: Vec::new(),
            },
        ]),
    ));
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "2 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn post_process_completeness_delegates_to_the_wrapped_operation() {
    let mut ir = CadIr::empty();
    let post_process = |operation: FeatureOperation| FeatureDefinition::PostProcess {
        operation,
        refine: true,
        fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::KernelDefault,
    };
    for (ordinal, definition) in [
        post_process(FeatureOperation::Helix {
            axis_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .unwrap(),
            axis_direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .unwrap(),
            radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
            shape: cadmpeg_ir::features::HelixShape::Cylindrical {
                pitch: cadmpeg_ir::scalar::NonZeroLength::new(2.0).unwrap(),
            },
            revolutions: cadmpeg_ir::scalar::PositiveReal::new(3.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            clockwise: false,
            segment_turns: None,
            construction_style: None,
        }),
        post_process(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:id#post-process-{ordinal}"))
                .expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_recurses_through_pattern_operands() {
    let mut ir = CadIr::empty();
    let seed = cadmpeg_ir::features::PatternSeed::Feature(
        FeatureId::mint("synthetic:test:id#seed").expect("identity grammar"),
    );
    for (ordinal, pattern) in [
        (
            0,
            PatternKind::new(PatternTransform::LinearOffsets {
                direction: None,
                offsets: vec![Length::ZERO, Length::new(10.0).unwrap()],
            })
            .unwrap(),
        ),
        (
            1,
            PatternKind::new(PatternTransform::CurveDriven {
                path: Some(PathRef::Native("path".into())),
                spacing: Length::new(10.0).unwrap(),
                count: 2,
            })
            .unwrap(),
        ),
        (
            2,
            PatternKind::new(PatternTransform::Scale {
                center: cadmpeg_ir::features::PatternScaleCenter::Native("center".into()),
                final_factor: 2.0,
                count: 2,
            })
            .unwrap(),
        ),
        (
            3,
            PatternKind::new(PatternTransform::Composite {
                stages: cadmpeg_ir::features::CompositePattern::new(vec![
                    cadmpeg_ir::features::PatternStage {
                        pattern: Box::new(
                            PatternKind::new(PatternTransform::CurveDriven {
                                path: None,
                                spacing: Length::new(10.0).unwrap(),
                                count: 2,
                            })
                            .unwrap(),
                        ),
                    },
                ])
                .unwrap(),
            })
            .unwrap(),
        ),
        (
            4,
            PatternKind::new(PatternTransform::Circular {
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_dir: Vector3::new(0.0, 0.0, 1.0),
                angle: Angle::new(std::f64::consts::TAU).unwrap(),
                count: 4,
            })
            .unwrap(),
        ),
    ] {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:id#pattern-{ordinal}"))
                .expect("identity grammar"),
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Pattern {
                    seeds: vec![seed.clone()],
                    pattern,
                }),
            ),
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "4 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_checks_secondary_sweep_and_loft_paths() {
    let mut ir = CadIr::empty();
    let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
    let planar_profile = cadmpeg_ir::features::PlanarProfileRef::Sketch(sketch.clone());
    let profile = cadmpeg_ir::features::ProfileRef::Planar(planar_profile.clone());
    let path = PathRef::Sketch(sketch);
    let sweep = |sections, orientation| {
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::sheet_sections(
                cadmpeg_ir::features::SweepMode::Surface {},
                cadmpeg_ir::features::SweepSection::Profile(planar_profile.clone()),
                sections,
            ),

            path: Some(path.clone()),

            orientation,
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
        })
    };
    let definitions = [
        sweep(
            vec![cadmpeg_ir::features::SweepSection::Profile(
                cadmpeg_ir::features::PlanarProfileRef::Native("section".into()),
            )],
            None,
        ),
        sweep(
            Vec::new(),
            Some(cadmpeg_ir::features::SweepOrientation::Auxiliary {
                path: PathRef::Native("auxiliary".into()),
                tangent: false,
                curvilinear: false,
            }),
        ),
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections: vec![
                cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
                cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
            ],
            guidance: cadmpeg_ir::features::LoftGuidance::Centerline(PathRef::Native(
                "centerline".into(),
            )),
            op: BooleanOp::NewBody,
            closed: false,
            solid: false,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }),
        sweep(Vec::new(), None),
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:id#path-feature-{ordinal}"))
                .expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "3 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn design_completeness_rejects_explicitly_unresolved_operation_fields() {
    let mut ir = CadIr::empty();
    let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
    let planar_profile = cadmpeg_ir::features::PlanarProfileRef::Sketch(sketch.clone());
    let profile = cadmpeg_ir::features::ProfileRef::Planar(planar_profile.clone());
    let path = PathRef::Sketch(sketch);
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
        "test:model:entity#face",
    )
    .expect("identity grammar")]);
    let extrude = |direction, termination| {
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: profile.clone(),
            direction,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination,
                    draft: None,
                },
            },
            op: BooleanOp::NewBody,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        })
    };
    let definitions = [
        FeatureDefinition::Operation(FeatureOperation::ProjectedCurve {
            source: path.clone(),
            target_faces: face.clone(),
            direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::Unresolved,
            ),
            bidirectional: Some(false),
        }),
        extrude(
            cadmpeg_ir::features::ExtrudeDirection::Unresolved {},
            cadmpeg_ir::features::LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(10.0).unwrap(),
            },
        ),
        extrude(
            cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
            cadmpeg_ir::features::LinearTermination::ToVertex {
                vertex: cadmpeg_ir::features::VertexSelection::native("vertex".into()).unwrap(),
            },
        ),
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces: face.clone(),
            distance: None,
        }),
        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
            faces: face.clone(),
            merge_entities: None,
            create_solid: None,
            gap_tolerance: None,
        }),
        FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
            faces: face.clone(),
            distance: Some(cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap()),
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        }),
        FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(path.clone()),
            support_faces: face.clone(),
            continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::unresolved(),
            merge_result: Some(false),
        }),
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: face.clone(),
            tool: path.clone(),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        }),
        FeatureDefinition::Operation(FeatureOperation::Draft {
            faces: face.clone(),
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: face.clone(),
                pull: None,
            },
            angle: None,
            outward: None,
        }),
        FeatureDefinition::Operation(FeatureOperation::ProjectedCurve {
            source: path,
            target_faces: face,
            direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal,
            ),
            bidirectional: Some(false),
        }),
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:id#operation-{ordinal}"))
                .expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "9 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn empty_required_operands_are_incomplete_design_semantics() {
    let mut ir = CadIr::empty();
    let feature = |ordinal, definition| Feature {
        id: FeatureId::mint(format!("synthetic:test:id#feature-{ordinal}"))
            .expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    ir.model.features.extend([
        feature(
            0,
            FeatureDefinition::Operation(FeatureOperation::Fillet {
                groups: cadmpeg_ir::features::NonEmptyMembers::one(
                    cadmpeg_ir::features::FilletGroup {
                        edges: EdgeSelection::Edges(Vec::new()),
                        radius: RadiusSpec::Constant {
                            radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
                        },
                        tangency_weight: None,
                    },
                ),
            }),
        ),
        feature(
            1,
            FeatureDefinition::Operation(FeatureOperation::DeleteFace {
                faces: FaceSelection::Faces(Vec::new()),
                heal: false,
            }),
        ),
        feature(
            2,
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Bodies(Vec::new()),
                mode: BodyRetentionMode::DeleteSelected,
            }),
        ),
        feature(
            3,
            FeatureDefinition::Operation(FeatureOperation::CompositeCurve {
                segments: cadmpeg_ir::features::NonEmptyMembers::one(PathRef::Edges(Vec::new())),
                closed: false,
            }),
        ),
        feature(
            4,
            FeatureDefinition::Operation(FeatureOperation::Shell {
                bodies: None,
                removed_faces: FaceSelection::Faces(Vec::new()),
                thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap()),
                outward: Some(false),
                mode: None,
                join: None,
                resolve_intersections: None,
                allow_self_intersections: None,
            }),
        ),
        feature(
            5,
            FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Edges(vec![
                    EdgeId::mint("test:model:entity#boundary").expect("identity grammar"),
                ])),
                support_faces: FaceSelection::Faces(Vec::new()),
                continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
                    SurfaceContinuity::Contact,
                ),
                merge_result: Some(false),
            }),
        ),
        feature(
            6,
            FeatureDefinition::Operation(FeatureOperation::RuledSurface {
                edges: EdgeSelection::Edges(vec![
                    EdgeId::mint("test:model:entity#boundary").expect("identity grammar")
                ]),
                support_faces: FaceSelection::Faces(Vec::new()),
                mode: RuledSurfaceMode::Direction {
                    direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .unwrap(),
                    distance: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
                },
                angle: None,
                alternate_face: None,
                corner: None,
            }),
        ),
        feature(
            7,
            FeatureDefinition::Operation(FeatureOperation::Fillet {
                groups: cadmpeg_ir::features::NonEmptyMembers::one(
                    cadmpeg_ir::features::FilletGroup {
                        edges: EdgeSelection::Edges(vec![
                            EdgeId::mint("test:model:entity#edge").expect("identity grammar")
                        ]),
                        radius: RadiusSpec::Unresolved {
                            form: Some(cadmpeg_ir::features::RadiusForm::Variable),
                        },
                        tangency_weight: None,
                    },
                ),
            }),
        ),
    ]);
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "6 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn hole_completeness_checks_optional_operands_when_present() {
    let mut ir = CadIr::empty();
    let hole = |profile, exit_kind| {
        FeatureDefinition::Operation(FeatureOperation::Hole {
            profile,
            profile_filter: None,
            face: None,
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
                cadmpeg_ir::features::HoleConstruction::form(
                    cadmpeg_ir::features::HoleKind::Simple,
                ),
                exit_kind,
                Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap()),
            )
            .unwrap(),

            extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        })
    };
    for (ordinal, definition) in [
        hole(
            Some(cadmpeg_ir::features::PlanarProfileRef::Native(
                "profile".into(),
            )),
            None,
        ),
        hole(None, Some(cadmpeg_ir::features::HoleKind::Unresolved(None))),
        hole(None, None),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:id#hole-{ordinal}"))
                .expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "2 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn incomplete_parameter_semantics_are_reported_as_design_losses() {
    let mut ir = CadIr::empty();
    let owner = FeatureId::mint("synthetic:test:id#owner").expect("identity grammar");
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: Some("Boss-Extrude1".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }),
        ),
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#base-parameter").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 0,
        name: "D0".into(),
        expression: "1mm".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(
            Length::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 1,
        name: "D1".into(),
        expression: "\"D0@Boss-Extrude1\" + Missing@Sketch1".into(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#bare-reference").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 2,
        name: "D2".into(),
        expression: "D99 + 1".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#malformed-reference").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 3,
        name: "D3".into(),
        expression: "\"D0@Boss-Extrude1".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    let future = ParameterId::mint("synthetic:test:id#future").expect("identity grammar");
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#forward-reference").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 4,
        name: "D4".into(),
        expression: "D5".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(2.0).unwrap(),
        )),
        dependencies: (vec![future.clone()]).try_into().unwrap(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: future,
        owner: Some(owner.clone()),
        ordinal: 5,
        name: "D5".into(),
        expression: "1".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#omitted-dependency").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 6,
        name: "D6".into(),
        expression: "D0 + 1mm".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(
            Length::new(2.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#cached-unsupported-expression")
            .expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 7,
        name: "D7".into(),
        expression: "unsupported(1)".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    for (id, ordinal, name) in [
        ("empty", 8, ""),
        ("shared-a", 9, "Shared"),
        ("shared-b", 10, "Shared"),
        ("ordinal", 10, "Unique"),
    ] {
        ir.model.parameters.push(DesignParameter {
            id: ParameterId::mint(format!("synthetic:test:id#identity:{id}"))
                .expect("identity grammar"),
            owner: Some(owner.clone()),
            ordinal,
            name: name.into(),
            expression: "1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
            )),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 parameter(s) lack an evaluated scalar; 3 parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; 4 parameter expression(s) cannot regenerate a finite typed value; 1 parameter record(s) contain missing or non-preceding dependency edges; 2 parameter record(s) have dependency edges inconsistent with their expressions; 1 dependency-driven parameter(s) disagree with their evaluated expressions."
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "1 parameter record(s) have empty names; 2 parameter record(s) share owner-local names; 2 parameter record(s) share owner-local ordinals."
    }));
}

#[test]
fn incoherent_feature_graph_is_reported_as_design_loss() {
    let mut ir = CadIr::empty();
    let first = FeatureId::mint("synthetic:test:id#first").expect("identity grammar");
    let second = FeatureId::mint("synthetic:test:id#second").expect("identity grammar");
    let missing = FeatureId::mint("synthetic:test:id#missing").expect("identity grammar");
    let feature = |id, ordinal, dependencies: Vec<FeatureId>| Feature {
        id,
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: (dependencies).try_into().unwrap(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }),
        ),
        native_ref: None,
    };
    ir.model
        .features
        .push(feature(first.clone(), 0, vec![second.clone()]));
    ir.model.features.push(feature(second, 1, vec![first]));
    ir.model.features.push(feature(
        FeatureId::mint("synthetic:test:id#third").expect("identity grammar"),
        1,
        vec![missing],
    ));
    ir.model.features[0].source_content = (vec![FeatureSourceContent::Feature(
        FeatureId::mint("synthetic:test:id#second").expect("identity grammar"),
    )])
    .try_into()
    .unwrap();
    ir.model.features[1].source_content = (vec![FeatureSourceContent::Feature(
        FeatureId::mint("synthetic:test:id#third").expect("identity grammar"),
    )])
    .try_into()
    .unwrap();
    ir.model.features[2].source_content = (vec![FeatureSourceContent::Parameter(
        ParameterId::mint("synthetic:test:id#missing-parameter").expect("identity grammar"),
    )])
    .try_into()
    .unwrap();
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "2 feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; 2 feature record(s) share regeneration ordinals."
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "3 feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
    }));
}

#[test]
fn incoherent_feature_outputs_are_reported_as_design_loss() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.features.clear();
    ir.model.parameters.clear();
    let body = ir.model.bodies[0].id.clone();
    let feature = |id: &str, ordinal: u64, outputs: Vec<BodyId>| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }),
            outputs,
        ),
        native_ref: None,
    };
    ir.model.features.push(feature(
        "synthetic:test:id#duplicate",
        0,
        vec![body.clone(), body],
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#missing",
        1,
        vec![BodyId::mint("test:model:entity#missing-body").expect("identity grammar")],
    ));
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message == "2 feature record(s) contain missing or repeated output body references."
    }));
}
