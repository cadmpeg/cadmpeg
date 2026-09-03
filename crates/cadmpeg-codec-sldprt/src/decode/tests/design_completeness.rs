// SPDX-License-Identifier: Apache-2.0
//! Typed-feature design-completeness audits.
#![allow(clippy::unwrap_used)]

use super::super::*;
use cadmpeg_ir::features::{
    Angle, BodyRetentionMode, BodySelection, BooleanOp, DesignParameter, EdgeSelection,
    FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureSourceContent,
    FeatureTreeNodeRole, Length, ParameterId, PathRef, PatternKind, RadiusSpec, RuledSurfaceMode,
    SurfaceContinuity,
};
use cadmpeg_ir::ids::{BodyId, EdgeId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn design_completeness_rejects_unresolved_and_unaudited_typed_families() {
    let mut ir = CadIr::empty(Units::default());
    let feature = |id: &str, ordinal, definition| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
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
        FeatureDefinition::DatumPlaneUnresolved,
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
    let mut ir = CadIr::empty(Units::default());
    let body = BodyId("body".into());
    let source = FeatureId("base".into());
    let mut push = |id: &str, ordinal, dependencies, outputs, definition| {
        ir.model.features.push(Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
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
            plane_origin: Point3::new(0.0, 0.0, 0.0),
            plane_normal: Vector3::new(0.0, 0.0, 1.0),
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
    let mut ir = CadIr::empty(Units::default());
    let body = BodyId("body".into());
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("face".into())]);
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
            id: FeatureId(format!("construction-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
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
    let mut ir = CadIr::empty(Units::default());
    let source = FeatureId("source".into());
    let feature = |id: &str, ordinal, dependencies, definition| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
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
    let mut ir = CadIr::empty(Units::default());
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
        post_process(post_process(FeatureDefinition::DatumPlaneUnresolved)),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId(format!("post-process-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
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
    let mut ir = CadIr::empty(Units::default());
    let seed = cadmpeg_ir::features::PatternSeed::Feature(FeatureId("seed".into()));
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
            id: FeatureId(format!("pattern-{ordinal}")),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
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
    let mut ir = CadIr::empty(Units::default());
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
            guides: Vec::new(),
            centerline: Some(PathRef::Native("centerline".into())),
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
            id: FeatureId(format!("path-feature-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
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
    let mut ir = CadIr::empty(Units::default());
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
    let profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone());
    let path = PathRef::Sketch(sketch);
    let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("face".into())]);
    let extrude = |direction, termination| FeatureDefinition::Extrude {
        profile: profile.clone(),
        direction,
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
        extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination,
                draft: None,
                offset: None,
            },
        },
        op: BooleanOp::NewBody,
        direction_source: None,
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
            cadmpeg_ir::features::Termination::Blind {
                length: Length(10.0),
            },
        ),
        extrude(
            cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            cadmpeg_ir::features::Termination::ToVertex {
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
            continuity: None,
            boundary_continuities: Vec::new(),
            merge_result: Some(false),
        },
        FeatureDefinition::TrimSurface {
            faces: face.clone(),
            tool: path.clone(),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            cell_selection: None,
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
            id: FeatureId(format!("operation-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
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
    let mut ir = CadIr::empty(Units::default());
    let feature = |ordinal, definition| Feature {
        id: FeatureId(format!("feature-{ordinal}")),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
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
                    EdgeId("boundary".into()),
                ])),
                support_faces: FaceSelection::Faces(Vec::new()),
                continuity: Some(SurfaceContinuity::Contact),
                boundary_continuities: Vec::new(),
                merge_result: Some(false),
            },
        ),
        feature(
            6,
            FeatureDefinition::RuledSurface {
                edges: EdgeSelection::Edges(vec![EdgeId("boundary".into())]),
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
                    edges: EdgeSelection::Edges(vec![EdgeId("edge".into())]),
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
    let mut ir = CadIr::empty(Units::default());
    let hole = |profile, exit_kind| FeatureDefinition::Hole {
        profile,
        profile_filter: None,
        face: None,
        position: None,
        direction: None,
        placements: vec![cadmpeg_ir::features::HolePlacement::Directed {
            position: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        }],
        kind: cadmpeg_ir::features::HoleKind::Simple,
        exit_kind,
        diameter: Some(Length(5.0)),
        extent: Some(cadmpeg_ir::features::Termination::ThroughAll),
        bottom: None,
        taper_angle: None,
        specification: None,
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
            id: FeatureId(format!("hole-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
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
    let mut ir = CadIr::empty(Units::default());
    let owner = FeatureId("owner".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: Some("Boss-Extrude1".into()),
        suppressed: Some(false),
        parent: None,
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
        id: ParameterId("base-parameter".into()),
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
        id: ParameterId("parameter".into()),
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
        id: ParameterId("bare-reference".into()),
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
        id: ParameterId("malformed-reference".into()),
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
    let future = ParameterId("future".into());
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("forward-reference".into()),
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
        id: ParameterId("omitted-dependency".into()),
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
        id: ParameterId("cached-unsupported-expression".into()),
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
            id: ParameterId(format!("identity:{id}")),
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
    let mut ir = CadIr::empty(Units::default());
    let first = FeatureId("first".into());
    let second = FeatureId("second".into());
    let missing = FeatureId("missing".into());
    let feature = |id, ordinal, parent, dependencies| Feature {
        id,
        ordinal,
        name: None,
        suppressed: Some(false),
        parent,
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
        .push(feature(first.clone(), 0, None, vec![second.clone()]));
    ir.model
        .features
        .push(feature(second, 1, Some(first.clone()), vec![first]));
    ir.model.features.push(feature(
        FeatureId("third".into()),
        1,
        Some(missing),
        Vec::new(),
    ));
    ir.model.features[0].source_content = vec![
        FeatureSourceContent::Feature(FeatureId("second".into())),
        FeatureSourceContent::Feature(FeatureId("second".into())),
    ];
    ir.model.features[1].source_content =
        vec![FeatureSourceContent::Feature(FeatureId("third".into()))];
    ir.model.features[2].source_content = vec![FeatureSourceContent::Parameter(ParameterId(
        "missing-parameter".into(),
    ))];
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
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
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
    ir.model
        .features
        .push(feature("missing", 1, vec![BodyId("missing-body".into())]));
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message == "2 feature record(s) contain missing or repeated output body references."
    }));
}
