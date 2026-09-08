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
        "synthetic:test:id#complete-helix",
        0,
        FeatureDefinition::Helix {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            radius: Length(1.0),
            shape: cadmpeg_ir::features::HelixShape::Cylindrical {
                pitch: cadmpeg_ir::features::HelixPitch::new(Length(2.0)).unwrap(),
            },
            revolutions: 3.0,
            start_angle: Angle(0.0),
            clockwise: false,
            segment_turns: None,
            construction_style: None,
        },
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#incomplete-dome",
        1,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native("face".into()),
            height: None,
            elliptical: None,
            reverse: None,
        },
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#unresolved-plane",
        2,
        FeatureDefinition::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        },
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#unaudited-stored-geometry",
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
    let source = FeatureId::mint("synthetic:test:id#base").expect("identity grammar");
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
        "synthetic:test:id#base",
        0,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![body.clone()]),
        },
    );
    push(
        "synthetic:test:id#stored",
        1,
        Vec::new(),
        vec![body.clone()],
        FeatureDefinition::StoredGeometry,
    );
    push(
        "synthetic:test:id#derived",
        2,
        vec![source.clone()],
        Vec::new(),
        FeatureDefinition::DerivedGeometry { source },
    );
    push(
        "synthetic:test:id#mirror",
        3,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::MirrorShape {
            source: BodySelection::Bodies(vec![body.clone()]),
            plane_origin: Point3::new(0.0, 0.0, 0.0),
            plane_normal: Vector3::new(0.0, 0.0, 1.0),
            plane_reference: Some(FaceSelection::Native("plane".into())),
        },
    );
    push(
        "synthetic:test:id#sew",
        4,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::SewBodies {
            bodies: BodySelection::Bodies(vec![body.clone()]),
            gap_tolerance: None,
        },
    );
    push(
        "synthetic:test:id#trim",
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
        "synthetic:test:id#import",
        6,
        Vec::new(),
        Vec::new(),
        FeatureDefinition::ImportedGeometry {
            path: "  ".into(),
            format: cadmpeg_ir::features::GeometryImportFormat::Step,
        },
    );
    push(
        "synthetic:test:id#section",
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
    let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
        "test:model:entity#face",
    )
    .expect("identity grammar")]);
    let definitions = [
        FeatureDefinition::PointGeometry {
            position: Point3::new(0.0, 0.0, 0.0),
        },
        FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::Box {
                length: Length(1.0),
                width: Length(2.0),
                height: Length(3.0),
            },
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::SheetMetalBaseFlange {
            profile: cadmpeg_ir::features::ProfileRef::Sketch(sketch),
            thickness: Length(1.0),
            side: cadmpeg_ir::features::SheetMetalThicknessSide::Symmetric,
        },
        FeatureDefinition::Polyline {
            points: vec![Point3::new(0.0, 0.0, 0.0)],
            closed: false,
        },
        FeatureDefinition::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        },
        FeatureDefinition::ProjectOnSurface {
            sources: PathRef::Native("sources".into()),
            support_face: face.clone(),
            direction: Vector3::new(0.0, 0.0, 1.0),
            mode: cadmpeg_ir::features::SurfaceProjectionMode::All,
            height: Length(0.0),
            offset: Length(0.0),
        },
        FeatureDefinition::Coil {
            construction: cadmpeg_ir::features::CoilConstruction {
                placement: cadmpeg_ir::features::CoilPlacement::Native {
                    native_ref: "placement".into(),
                },
                diameter: Length(10.0),
                extent: cadmpeg_ir::features::CoilExtent::RevolutionsHeight {
                    revolutions: 2.0,
                    height: Length(5.0),
                },
                section: cadmpeg_ir::features::CoilSection::Circular {
                    diameter: Length(1.0),
                },
                section_placement: cadmpeg_ir::features::CoilSectionPlacement::Center,
                clockwise: false,
                taper: Angle(0.0),
            },
            result: cadmpeg_ir::features::CoilResult::NewBody,
        },
        FeatureDefinition::Sphere {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: Length(1.0),
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
            id: FeatureId::mint(format!("synthetic:test:id#construction-{ordinal}"))
                .expect("identity grammar"),
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
    let source = FeatureId::mint("synthetic:test:id#source").expect("identity grammar");
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
        "synthetic:test:id#source",
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
        "synthetic:test:id#complete",
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
        "synthetic:test:id#native",
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
        "synthetic:test:id#multiple-shape-sources",
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
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            radius: Length(1.0),
            shape: cadmpeg_ir::features::HelixShape::Cylindrical {
                pitch: cadmpeg_ir::features::HelixPitch::new(Length(2.0)).unwrap(),
            },
            revolutions: 3.0,
            start_angle: Angle(0.0),
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
            id: FeatureId::mint(format!("synthetic:test:id#post-process-{ordinal}"))
                .expect("identity grammar"),
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
        FeatureId::mint("synthetic:test:id#seed").expect("identity grammar"),
    );
    for (ordinal, pattern) in [
        (
            0,
            PatternKind::LinearOffsets {
                direction: None,
                offsets: vec![Length(0.0), Length(10.0)],
            },
        ),
        (
            1,
            PatternKind::CurveDriven {
                path: Some(PathRef::Native("path".into())),
                spacing: Length(10.0),
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
                        spacing: Length(10.0),
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
                angle: Angle(std::f64::consts::TAU),
                count: 4,
            },
        ),
    ] {
        ir.model.features.push(Feature {
            id: FeatureId::mint(format!("synthetic:test:id#pattern-{ordinal}"))
                .expect("identity grammar"),
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
    let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
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
            id: FeatureId::mint(format!("synthetic:test:id#path-feature-{ordinal}"))
                .expect("identity grammar"),
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
    let sketch = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap();
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
                length: Length(10.0),
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
            distance: Some(Length(10.0)),
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
            id: FeatureId::mint(format!("synthetic:test:id#operation-{ordinal}"))
                .expect("identity grammar"),
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
        id: FeatureId::mint(format!("synthetic:test:id#feature-{ordinal}"))
            .expect("identity grammar"),
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
                        radius: Length(1.0),
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
                thickness: Some(Length(1.0)),
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
                    direction: Vector3::new(0.0, 0.0, 1.0),
                    distance: Length(1.0),
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
            position: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        }]),
        construction: cadmpeg_ir::features::HoleConstruction::form(
            cadmpeg_ir::features::HoleKind::Simple,
        ),
        exit_kind,
        diameter: Some(Length(5.0)),
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
            id: FeatureId::mint(format!("synthetic:test:id#hole-{ordinal}"))
                .expect("identity grammar"),
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
    let owner = FeatureId::mint("synthetic:test:id#owner").expect("identity grammar");
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
        id: ParameterId::mint("synthetic:test:id#base-parameter").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 0,
        name: "D0".into(),
        expression: "1mm".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(Length(1.0))),
        dependencies: Vec::new(),
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
        dependencies: Vec::new(),
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
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
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
        value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
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
        id: ParameterId::mint("synthetic:test:id#omitted-dependency").expect("identity grammar"),
        owner: Some(owner.clone()),
        ordinal: 6,
        name: "D6".into(),
        expression: "D0 + 1mm".into(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(Length(2.0))),
        dependencies: Vec::new(),
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
            id: ParameterId::mint(format!("synthetic:test:id#identity:{id}"))
                .expect("identity grammar"),
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
    let first = FeatureId::mint("synthetic:test:id#first").expect("identity grammar");
    let second = FeatureId::mint("synthetic:test:id#second").expect("identity grammar");
    let missing = FeatureId::mint("synthetic:test:id#missing").expect("identity grammar");
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
        FeatureId::mint("synthetic:test:id#third").expect("identity grammar"),
        1,
        vec![missing],
    ));
    ir.model.features[0].source_content = vec![
        FeatureSourceContent::Feature(
            FeatureId::mint("synthetic:test:id#second").expect("identity grammar"),
        ),
        FeatureSourceContent::Feature(
            FeatureId::mint("synthetic:test:id#second").expect("identity grammar"),
        ),
    ];
    ir.model.features[1].source_content = vec![FeatureSourceContent::Feature(
        FeatureId::mint("synthetic:test:id#third").expect("identity grammar"),
    )];
    ir.model.features[2].source_content = vec![FeatureSourceContent::Parameter(
        ParameterId::mint("synthetic:test:id#missing-parameter").expect("identity grammar"),
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
