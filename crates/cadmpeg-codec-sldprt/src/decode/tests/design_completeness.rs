// SPDX-License-Identifier: Apache-2.0
//! Typed-feature design-completeness audits.
#![allow(clippy::unwrap_used)]

use super::super::*;
use cadmpeg_ir::features::{
    Angle, BodyRetentionMode, BodySelection, BooleanOp, DesignParameter, EdgeSelection,
    FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureSourceContent,
    FeatureTreeNodeRole, Length, ParameterId, PathRef, PatternKind, RadiusSpec, RuledSurfaceMode,
    SurfaceContinuity, UnresolvedFamily,
};
use cadmpeg_ir::ids::{BodyId, EdgeId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn design_completeness_rejects_unresolved_and_unaudited_typed_families() {
    let mut ir = CadIr::empty();
    let feature = |id: &str, ordinal, definition| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    ir.model.features.push(feature(
        "complete-helix",
        0,
        FeatureDefinition::Helix {
            axis_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .unwrap(),
            axis_direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .unwrap(),
            radius: cadmpeg_ir::features::PositiveLength::new(1.0).unwrap(),
            shape: cadmpeg_ir::features::HelixShape::Cylindrical {
                pitch: cadmpeg_ir::features::NonZeroLength::new(2.0).unwrap(),
            },
            revolutions: cadmpeg_ir::features::PositiveReal::new(3.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            clockwise: false,
            segment_turns: None,
            construction_style: None,
        },
    ));
    ir.model.features.push(feature(
        "incomplete-dome",
        1,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native("face".into()),
            height: None,
            elliptical: None,
            reverse: None,
        },
    ));
    ir.model.features.push(feature(
        "unresolved-plane",
        2,
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        },
    ));
    ir.model.features.push(feature(
        "unaudited-stored-geometry",
        3,
        FeatureDefinition::StoredGeometry,
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
    let source = FeatureId::mint("base").expect("identity grammar");
    let mut push = |id: &str, ordinal, dependencies, outputs, definition| {
        ir.model.features.push(Feature {
            id: FeatureId::mint(id).expect("identity grammar"),
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref: None,
        });
    };
    push(
        "base",
        0,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![body.clone()]),
        },
    );
    push(
        "stored",
        1,
        Vec::new(),
        vec![body.clone()],
        FeatureDefinition::StoredGeometry,
    );
    push(
        "derived",
        2,
        vec![source.clone()],
        Vec::new(),
        FeatureDefinition::DerivedGeometry { source },
    );
    push(
        "mirror",
        3,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::MirrorShape {
            source: BodySelection::Bodies(vec![body.clone()]),
            plane_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .unwrap(),
            plane_normal: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                .unwrap(),
            plane_reference: Some(FaceSelection::Native("plane".into())),
        },
    );
    push(
        "sew",
        4,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::SewBodies {
            bodies: BodySelection::Bodies(vec![body.clone()]),
            gap_tolerance: None,
        },
    );
    push(
        "trim",
        5,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::TrimBodies {
            targets: BodySelection::Bodies(vec![body.clone()]),
            tools: BodySelection::Bodies(vec![body.clone()]),
            keep: cadmpeg_ir::features::BodyTrimSide::Unresolved,
        },
    );
    push(
        "import",
        6,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::ImportedGeometry {
            path: "  ".into(),
            format: cadmpeg_ir::features::GeometryImportFormat::Step,
        },
    );
    push(
        "section",
        7,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::SectionShape {
            first: BodySelection::Bodies(vec![body.clone()]),
            second: BodySelection::Bodies(vec![body]),
            approximate: None,
        },
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
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
        "test:model:entity#face",
    )
    .expect("identity grammar")]);
    let definitions = [
        FeatureDefinition::PointGeometry {
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
        },
        FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::new(
                cadmpeg_ir::features::PrimitiveSolidKind::Box {
                    length: Length::new(1.0).unwrap(),
                    width: Length::new(2.0).unwrap(),
                    height: Length::new(3.0).unwrap(),
                },
            )
            .unwrap(),
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::SheetMetalBaseFlange {
            profile: cadmpeg_ir::features::ProfileRef::Sketch(sketch),
            thickness: cadmpeg_ir::features::PositiveLength::new(1.0).unwrap(),
            side: cadmpeg_ir::features::SheetMetalThicknessSide::Symmetric,
        },
        FeatureDefinition::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        },
        FeatureDefinition::ProjectOnSurface {
            sources: PathRef::Native("sources".into()),
            support_face: face.clone(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                .unwrap(),
            mode: cadmpeg_ir::features::SurfaceProjectionMode::All,
            height: cadmpeg_ir::features::NonNegativeLength::new(0.0).unwrap(),
            offset: Length::new(0.0).unwrap(),
        },
        FeatureDefinition::Coil {
            construction: cadmpeg_ir::features::CoilConstruction {
                placement: cadmpeg_ir::features::CoilPlacement::Native {
                    native_ref: cadmpeg_ir::features::SelectionReference::try_from(String::from(
                        "placement",
                    ))
                    .unwrap(),
                },
                diameter: cadmpeg_ir::features::PositiveLength::new(10.0).unwrap(),
                extent: cadmpeg_ir::features::CoilExtent::RevolutionsHeight {
                    revolutions: cadmpeg_ir::features::PositiveReal::new(2.0).unwrap(),
                    height: Length::new(5.0).unwrap(),
                },
                section: cadmpeg_ir::features::CoilSection::Circular {
                    diameter: cadmpeg_ir::features::PositiveLength::new(1.0).unwrap(),
                },
                section_placement: cadmpeg_ir::features::CoilSectionPlacement::Center,
                clockwise: false,
                taper: Angle::new(0.0).unwrap(),
            },
            result: cadmpeg_ir::features::CoilResult::NewBody,
        },
        FeatureDefinition::Sphere {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            radius: cadmpeg_ir::features::PositiveLength::new(1.0).unwrap(),
            op: BooleanOp::Unresolved,
        },
        FeatureDefinition::FaceBlend {
            first_faces: face.clone(),
            second_faces: face.clone(),
            radius: RadiusSpec::Variable { points: Vec::new() },
        },
        FeatureDefinition::BoundaryFill {
            tools: BodySelection::Bodies(vec![body]),
            cells: Vec::new(),
        },
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("construction-{ordinal}")).expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "7 typed feature(s) retain native or unresolved required operation operands."
    }));
}

#[test]
fn binder_completeness_requires_resolved_targets_and_shape_arity() {
    let mut ir = CadIr::empty();
    let source = FeatureId::mint("source").expect("identity grammar");
    let feature = |id: &str, ordinal, dependencies, definition| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies,
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    ir.model.features.push(feature(
        "source",
        0,
        Vec::new(),
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
    ));
    let shape = |sources| FeatureDefinition::Binder {
        sources,
        construction: cadmpeg_ir::features::BinderConstruction::Shape {
            trace_support: false,
        },
    };
    ir.model.features.push(feature(
        "complete",
        1,
        vec![source.clone()],
        shape(vec![cadmpeg_ir::features::BinderSource {
            target: cadmpeg_ir::features::BinderTarget::Feature {
                feature: source.clone(),
            },
            subelements: vec!["Face1".into()],
        }]),
    ));
    ir.model.features.push(feature(
        "native",
        2,
        Vec::new(),
        shape(vec![cadmpeg_ir::features::BinderSource {
            target: cadmpeg_ir::features::BinderTarget::Native {
                reference: "source".into(),
            },
            subelements: Vec::new(),
        }]),
    ));
    ir.model.features.push(feature(
        "multiple-shape-sources",
        3,
        Vec::new(),
        shape(vec![
            cadmpeg_ir::features::BinderSource {
                target: cadmpeg_ir::features::BinderTarget::External {
                    document: "a.FCStd".into(),
                    object: "Body".into(),
                },
                subelements: Vec::new(),
            },
            cadmpeg_ir::features::BinderSource {
                target: cadmpeg_ir::features::BinderTarget::External {
                    document: "b.FCStd".into(),
                    object: "Body".into(),
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
    let post_process = |operation| FeatureDefinition::PostProcess {
        operation: Box::new(operation),
        refine: true,
        fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::KernelDefault,
    };
    for (ordinal, definition) in [
        post_process(FeatureDefinition::Helix {
            axis_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .unwrap(),
            axis_direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .unwrap(),
            radius: cadmpeg_ir::features::PositiveLength::new(1.0).unwrap(),
            shape: cadmpeg_ir::features::HelixShape::Cylindrical {
                pitch: cadmpeg_ir::features::NonZeroLength::new(2.0).unwrap(),
            },
            revolutions: cadmpeg_ir::features::PositiveReal::new(3.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            clockwise: false,
            segment_turns: None,
            construction_style: None,
        }),
        post_process(post_process(FeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        })),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("post-process-{ordinal}")).expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
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
        FeatureId::mint("seed").expect("identity grammar"),
    );
    for (ordinal, pattern) in [
        (
            0,
            PatternKind::LinearOffsets {
                direction: None,
                offsets: vec![Length::ZERO, Length::new(10.0).unwrap()],
            },
        ),
        (
            1,
            PatternKind::CurveDriven {
                path: Some(PathRef::Native("path".into())),
                spacing: Length::new(10.0).unwrap(),
                count: 2,
            },
        ),
        (
            2,
            PatternKind::Scale {
                center: cadmpeg_ir::features::PatternScaleCenter::Native("center".into()),
                final_factor: 2.0,
                count: 2,
            },
        ),
        (
            3,
            PatternKind::Composite {
                stages: vec![cadmpeg_ir::features::PatternStage {
                    pattern: Box::new(PatternKind::CurveDriven {
                        path: None,
                        spacing: Length::new(10.0).unwrap(),
                        count: 2,
                    }),
                    combination: cadmpeg_ir::features::PatternStageCombination::Initialize,
                }],
            },
        ),
        (
            4,
            PatternKind::Circular {
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_dir: Vector3::new(0.0, 0.0, 1.0),
                angle: Angle::new(std::f64::consts::TAU).unwrap(),
                count: 4,
            },
        ),
    ] {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("pattern-{ordinal}")).expect("identity grammar"),
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Pattern {
                seeds: vec![seed.clone()],
                pattern,
            },
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
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
    let profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone());
    let path = PathRef::Sketch(sketch);
    let sweep = |sections, orientation| FeatureDefinition::Sweep {
        section: cadmpeg_ir::features::SweepSection::Profile(profile.clone()),
        sections,
        path: Some(path.clone()),
        mode: cadmpeg_ir::features::SweepMode::Surface,
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
    };
    let definitions = [
        sweep(
            vec![cadmpeg_ir::features::SweepSection::Profile(
                cadmpeg_ir::features::ProfileRef::Native("section".into()),
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
        FeatureDefinition::Loft {
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
        },
        sweep(Vec::new(), None),
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("path-feature-{ordinal}")).expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
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
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
    let profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone());
    let path = PathRef::Sketch(sketch);
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
        "test:model:entity#face",
    )
    .expect("identity grammar")]);
    let extrude = |direction, termination| FeatureDefinition::Extrude {
        profile: profile.clone(),
        direction,
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
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
    };
    let definitions = [
        FeatureDefinition::ProjectedCurve {
            source: path.clone(),
            target_faces: face.clone(),
            direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::Unresolved,
            ),
            bidirectional: Some(false),
        },
        extrude(
            cadmpeg_ir::features::ExtrudeDirection::Unresolved,
            cadmpeg_ir::features::LinearTermination::Blind {
                length: cadmpeg_ir::features::NonZeroLength::new(10.0).unwrap(),
            },
        ),
        extrude(
            cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            cadmpeg_ir::features::LinearTermination::ToVertex {
                vertex: cadmpeg_ir::features::VertexSelection::Native("vertex".into()),
            },
        ),
        FeatureDefinition::OffsetSurface {
            faces: face.clone(),
            distance: None,
        },
        FeatureDefinition::KnitSurface {
            faces: face.clone(),
            merge_entities: None,
            create_solid: None,
            gap_tolerance: None,
        },
        FeatureDefinition::ExtendSurface {
            faces: face.clone(),
            distance: Some(cadmpeg_ir::features::PositiveLength::new(10.0).unwrap()),
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        },
        FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Path(path.clone()),
            support_faces: face.clone(),
            continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::unresolved(),
            merge_result: Some(false),
        },
        FeatureDefinition::TrimSurface {
            faces: face.clone(),
            tool: path.clone(),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        },
        FeatureDefinition::Draft {
            faces: face.clone(),
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: face.clone(),
                pull: None,
            },
            angle: None,
            outward: None,
        },
        FeatureDefinition::ProjectedCurve {
            source: path,
            target_faces: face,
            direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal,
            ),
            bidirectional: Some(false),
        },
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("operation-{ordinal}")).expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
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
        id: FeatureId::mint(format!("feature-{ordinal}")).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: None,
    };
    ir.model.features.extend([
        feature(
            0,
            FeatureDefinition::Fillet {
                groups: vec![cadmpeg_ir::features::FilletGroup {
                    edges: EdgeSelection::Edges(Vec::new()),
                    radius: RadiusSpec::Constant {
                        radius: Length::new(1.0).unwrap(),
                    },
                    tangency_weight: None,
                }],
            },
        ),
        feature(
            1,
            FeatureDefinition::DeleteFace {
                faces: FaceSelection::Faces(Vec::new()),
                heal: false,
            },
        ),
        feature(
            2,
            FeatureDefinition::DeleteBody {
                bodies: BodySelection::Bodies(Vec::new()),
                mode: BodyRetentionMode::DeleteSelected,
            },
        ),
        feature(
            3,
            FeatureDefinition::CompositeCurve {
                segments: vec![PathRef::Edges(Vec::new())],
                closed: false,
            },
        ),
        feature(
            4,
            FeatureDefinition::Shell {
                bodies: None,
                removed_faces: FaceSelection::Faces(Vec::new()),
                thickness: Some(cadmpeg_ir::features::PositiveLength::new(1.0).unwrap()),
                outward: Some(false),
                mode: None,
                join: None,
                resolve_intersections: None,
                allow_self_intersections: None,
            },
        ),
        feature(
            5,
            FeatureDefinition::FilledSurface {
                boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Edges(vec![
                    EdgeId::mint("test:model:entity#boundary").expect("identity grammar"),
                ])),
                support_faces: FaceSelection::Faces(Vec::new()),
                continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
                    SurfaceContinuity::Contact,
                ),
                merge_result: Some(false),
            },
        ),
        feature(
            6,
            FeatureDefinition::RuledSurface {
                edges: EdgeSelection::Edges(vec![
                    EdgeId::mint("test:model:entity#boundary").expect("identity grammar")
                ]),
                support_faces: FaceSelection::Faces(Vec::new()),
                mode: RuledSurfaceMode::Direction {
                    direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .unwrap(),
                    distance: cadmpeg_ir::features::PositiveLength::new(1.0).unwrap(),
                },
                angle: None,
                alternate_face: None,
                corner: None,
            },
        ),
        feature(
            7,
            FeatureDefinition::Fillet {
                groups: vec![cadmpeg_ir::features::FilletGroup {
                    edges: EdgeSelection::Edges(vec![
                        EdgeId::mint("test:model:entity#edge").expect("identity grammar")
                    ]),
                    radius: RadiusSpec::Variable { points: Vec::new() },
                    tangency_weight: None,
                }],
            },
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
    let hole = |profile, exit_kind| FeatureDefinition::Hole {
        profile,
        profile_filter: None,
        face: None,
        direction: None,
        placements: Some(vec![cadmpeg_ir::features::HolePlacement::Directed {
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                .unwrap(),
        }]),
        construction: cadmpeg_ir::features::HoleConstruction::form(
            cadmpeg_ir::features::HoleKind::Simple,
        ),
        exit_kind,
        diameter: Some(cadmpeg_ir::features::PositiveLength::new(5.0).unwrap()),
        extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll),
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    };
    for (ordinal, definition) in [
        hole(
            Some(cadmpeg_ir::features::ProfileRef::Native("profile".into())),
            None,
        ),
        hole(None, Some(cadmpeg_ir::features::HoleKind::Unresolved(None))),
        hole(None, None),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("hole-{ordinal}")).expect("identity grammar"),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
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
    let owner = FeatureId::mint("owner").expect("identity grammar");
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: Some("Boss-Extrude1".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("base-parameter").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 0,
        name: "D0".into(),
        expression: "1mm".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(
            Length::new(1.0).unwrap(),
        )),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("parameter").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 1,
        name: "D1".into(),
        expression: "\"D0@Boss-Extrude1\" + Missing@Sketch1".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("bare-reference").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 2,
        name: "D2".into(),
        expression: "D99 + 1".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("malformed-reference").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 3,
        name: "D3".into(),
        expression: "\"D0@Boss-Extrude1".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    let future = ParameterId::mint("future").expect("identity grammar");
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("forward-reference").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 4,
        name: "D4".into(),
        expression: "D5".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(2.0)),
        dependencies: vec![future.clone()],
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
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("omitted-dependency").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 6,
        name: "D6".into(),
        expression: "D0 + 1mm".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(
            Length::new(2.0).unwrap(),
        )),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("cached-unsupported-expression").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 7,
        name: "D7".into(),
        expression: "unsupported(1)".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
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
            id: ParameterId::mint(format!("identity:{id}")).expect("identity grammar"),
            owner: Some(owner.clone()),
            ordinal,
            name: name.into(),
            expression: "1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
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
    let first = FeatureId::mint("first").expect("identity grammar");
    let second = FeatureId::mint("second").expect("identity grammar");
    let missing = FeatureId::mint("missing").expect("identity grammar");
    let feature = |id, ordinal, dependencies| Feature {
        id,
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies,
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    };
    ir.model
        .features
        .push(feature(first.clone(), 0, vec![second.clone()]));
    ir.model.features.push(feature(second, 1, vec![first]));
    ir.model.features.push(feature(
        FeatureId::mint("third").expect("identity grammar"),
        1,
        vec![missing],
    ));
    ir.model.features[0].source_content = vec![
        FeatureSourceContent::Feature(FeatureId::mint("second").expect("identity grammar")),
        FeatureSourceContent::Feature(FeatureId::mint("second").expect("identity grammar")),
    ];
    ir.model.features[1].source_content = vec![FeatureSourceContent::Feature(
        FeatureId::mint("third").expect("identity grammar"),
    )];
    ir.model.features[2].source_content = vec![FeatureSourceContent::Parameter(
        ParameterId::mint("missing-parameter").expect("identity grammar"),
    )];
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
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs,
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    };
    ir.model
        .features
        .push(feature("duplicate", 0, vec![body.clone(), body]));
    ir.model.features.push(feature(
        "missing",
        1,
        vec![BodyId::mint("test:model:entity#missing-body").expect("identity grammar")],
    ));
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message == "2 feature record(s) contain missing or repeated output body references."
    }));
}
