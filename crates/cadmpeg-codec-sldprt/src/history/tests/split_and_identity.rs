// SPDX-License-Identifier: Apache-2.0
//! Split-face, body-modifier, and operation-identity projection tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use super::*;

#[test]
fn split_face_path_uses_the_prebound_source_sketch() {
    let dimensions = BTreeMap::from([("D1".into(), "<MOD-DIAM>85".into())]);
    let mut split = feature("split", Some("711"), 0);
    split.kind = "Split Line".into();
    split.input_class = Some("moPLine_c".into());
    split.parameters.clone_from(&dimensions);
    split.properties.insert(
        crate::resolved_features::operations::SPLIT_LINE_MODE_PROPERTY.into(),
        crate::resolved_features::operations::SPLIT_LINE_PROJECTION_MODE.into(),
    );
    split.properties.insert(
        crate::resolved_features::operations::SPLIT_LINE_TOOL_PROPERTY.into(),
        "sketch".into(),
    );
    let mut sketch = feature("sketch", Some("705"), 1);
    sketch.xml_tag = "Sketch".into();
    sketch.kind = "Sketch".into();
    sketch.input_class = Some("moProfileFeature_c".into());
    sketch.parameters = BTreeMap::from([("D1".into(), "<MOD-DIAM>2159mm".into())]);
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![split.clone(), sketch.clone()],
    };

    let projected = project_features(std::slice::from_ref(&history));
    let split_feature = projected
        .iter()
        .find(|candidate| candidate.native_ref.as_deref() == Some(split.id.as_str()))
        .expect("split feature");
    assert_eq!(
        split_feature.definition,
        FeatureDefinition::SplitFace {
            targets: FaceSelection::Unresolved,
            tool: SplitFaceTool::Path(PathRef::Native(sketch.id.clone())),
        }
    );
    assert_eq!(
        split_feature.dependencies,
        vec![neutral_feature_id(&sketch.id)]
    );
}

#[test]
fn split_face_path_binds_to_projected_sketch_geometry() {
    let mut definition = FeatureDefinition::SplitFace {
        targets: FaceSelection::Unresolved,
        tool: SplitFaceTool::Path(PathRef::Native("sketch-native".into())),
    };
    let feature_id = FeatureId("sketch-feature".into());
    let sketch_id = cadmpeg_ir::sketches::SketchId("sketch-geometry".into());

    assert!(bind_definition_sketch(
        &mut definition,
        "sketch-native",
        &feature_id,
        &sketch_id,
        true,
    ));
    assert!(matches!(
        definition,
        FeatureDefinition::SplitFace {
            tool: SplitFaceTool::Path(PathRef::Sketch(ref bound)),
            ..
        } if bound == &sketch_id
    ));
}

#[test]
fn standalone_history_note_projects_as_text_annotation_not_feature() {
    let mut note = feature("sldprt:history:feature#7:3", Some("42"), 3);
    note.xml_tag = "Note".into();
    note.name = "Manufacturing note".into();
    note.kind = "Note".into();
    note.text = Some("REMOVE ALL BURRS".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![note.clone()],
    };

    let annotations = project_semantic_notes(std::slice::from_ref(&history));
    assert!(project_features(&[history]).is_empty());
    assert!(matches!(
        annotations.as_slice(),
        [cadmpeg_ir::semantic_annotations::SemanticAnnotation {
            kind: cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text,
            text,
            native_ref,
            ..
        }] if text == &["REMOVE ALL BURRS"] && native_ref == &note.id
    ));
}

#[test]
fn source_less_offset_plane_resolves_a_native_feature_reference() {
    let mut principal = feature("principal-native", None, 0);
    principal.name = "Right".into();
    principal.input_class = Some("moRefPlane_c".into());
    let mut offset = feature("offset-native", None, 1);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "6".into());
    offset
        .properties
        .insert("Reference".into(), principal.id.clone());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![principal, offset],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        &projected[1].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            distance: Length(6.0),
        } if reference == &projected[0].id
    ));
    assert_eq!(projected[1].dependencies, [projected[0].id.clone()]);
}

#[test]
fn body_modifier_uses_one_based_modeling_history_ordinal() {
    let first = feature("first-native", Some("700"), 0);
    let second = feature("second-native", Some("900"), 1);
    let histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second],
    }];
    let mut projected = project_features(&histories);
    let body_modifiers = vec![("sldprt:brep:body#333".into(), 2)];

    derive_feature_outputs(
        &mut projected,
        &histories,
        &[],
        &body_modifiers,
        &[],
        &[],
        &[],
    );

    assert!(projected[0].outputs.is_empty());
    assert_eq!(
        projected[1].outputs,
        [cadmpeg_ir::ids::BodyId("sldprt:brep:body#333".into())]
    );
}

#[test]
fn body_modifier_ordinal_is_unresolved_when_history_is_ambiguous() {
    let histories = vec![
        FeatureHistory {
            id: "history-a".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature("a", None, 0), feature("a-next", None, 1)],
        },
        FeatureHistory {
            id: "history-b".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![feature("b", None, 0), feature("b-next", None, 1)],
        },
    ];
    let mut projected = project_features(&histories);
    let body_modifiers = vec![("sldprt:brep:body#333".into(), 2)];

    derive_feature_outputs(
        &mut projected,
        &histories,
        &[],
        &body_modifiers,
        &[],
        &[],
        &[],
    );

    assert!(projected.iter().all(|feature| feature.outputs.is_empty()));
}

#[test]
fn native_operation_identity_selects_surface_and_solid_projectors() {
    let mut dome = feature("dome", Some("1"), 0);
    dome.kind = "Dome".into();
    dome.input_class = Some("moDome_c".into());
    dome.parameters.insert("D1".into(), "2mm".into());

    let mut rib = feature("rib", Some("2"), 1);
    rib.kind = "Rib".into();
    rib.input_class = Some("moRib_c".into());
    rib.parameters.insert("D1".into(), "1mm".into());

    let mut surface_loft = feature("surface-loft", Some("3"), 2);
    surface_loft.kind = "Surface-Loft".into();
    surface_loft.input_class = Some("moBlendRefSurface_c".into());

    let mut cut_loft = feature("cut-loft", Some("4"), 3);
    cut_loft.kind = "Cut-Loft".into();
    cut_loft.input_class = Some("moBlendCut_c".into());

    let mut surface_extrude = feature("surface-extrude", Some("5"), 4);
    surface_extrude.kind = "Surface-Extrude".into();
    surface_extrude.input_class = Some("moExtruRefSurface_c".into());
    surface_extrude.parameters.insert("D1".into(), "3mm".into());

    let mut offset_surface = feature("offset-surface", Some("6"), 5);
    offset_surface.kind = "Surface-Offset".into();
    offset_surface.input_class = Some("moOffsetRefSurface_c".into());

    let mut knit_surface = feature("knit-surface", Some("7"), 6);
    knit_surface.kind = "Surface-Knit".into();
    knit_surface.input_class = Some("moSewRefSurface_c".into());

    let mut filled_surface = feature("filled-surface", Some("8"), 7);
    filled_surface.kind = "Surface-Fill".into();
    filled_surface.input_class = Some("moFillRefSurface_c".into());

    let mut trim_surface = feature("trim-surface", Some("9"), 8);
    trim_surface.kind = "Surface-Trim".into();
    trim_surface.input_class = Some("moTrimRefSurface_c".into());

    let mut extend_surface = feature("extend-surface", Some("10"), 9);
    extend_surface.kind = "Surface-Extend".into();
    extend_surface.input_class = Some("moExtendRefSurface_c".into());

    let mut draft = feature("draft", Some("11"), 10);
    draft.kind = "Draft".into();
    draft.input_class = Some("moDraft_c".into());
    draft.parameters.insert("D1".into(), "3deg".into());

    let projected = project_features(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            dome,
            rib,
            surface_loft,
            cut_loft,
            surface_extrude,
            offset_surface,
            knit_surface,
            filled_surface,
            trim_surface,
            extend_surface,
            draft,
        ],
    }]);

    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Dome {
            height: Some(Length(2.0)),
            ..
        }
    ));
    assert!(matches!(
        projected[1].definition,
        FeatureDefinition::Rib {
            construction: RibConstruction {
                thickness: Some(Length(1.0)),
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        projected[2].definition,
        FeatureDefinition::Loft {
            solid: false,
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        projected[3].definition,
        FeatureDefinition::Loft {
            solid: true,
            op: BooleanOp::Cut,
            ..
        }
    ));
    assert!(matches!(
        projected[4].definition,
        FeatureDefinition::Extrude {
            solid: Some(false),
            ..
        }
    ));
    assert!(matches!(
        projected[5].definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
        }
    ));
    assert!(matches!(
        projected[6].definition,
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Unresolved,
            merge_entities: None,
            create_solid: None,
            gap_tolerance: None,
        }
    ));
    assert!(matches!(
        projected[7].definition,
        FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Unresolved),
            support_faces: FaceSelection::Unresolved,
            continuity: None,
            merge_result: None,
            ..
        }
    ));
    assert!(matches!(
        projected[8].definition,
        FeatureDefinition::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: PathRef::Unresolved(_),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        projected[9].definition,
        FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        }
    ));
    assert!(matches!(
        projected[10].definition,
        FeatureDefinition::Draft {
            faces: FaceSelection::Unresolved,
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: FaceSelection::Unresolved,
                pull: None,
            },
            angle: Some(Angle(value)),
            outward: None,
            ..
        } if (value - std::f64::consts::PI / 60.0).abs() < 1.0e-12
    ));
}

#[test]
fn variable_fillet_does_not_use_d1_as_a_constant_radius() {
    let mut feature = feature("variable-fillet", Some("61"), 0);
    feature.kind = "VarFillet".into();
    feature.input_class = Some("VarFillet_c".into());
    feature.parameters.insert("D1".into(), "R1".into());
    assert!(matches!(
        project_fillet(&feature),
        FeatureDefinition::Fillet { groups }
            if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Unresolved { .. },
                ..
            }])
    ));
}

#[test]
fn variable_fillet_d_dimensions_require_native_vertex_associations() {
    let mut feature = feature("variable-fillet", Some("61"), 0);
    feature.kind = "VarFillet".into();
    feature.input_class = Some("VarFillet_c".into());
    feature.parameters = BTreeMap::from([
        ("D0".into(), "R2mm".into()),
        ("D01".into(), "R3mm".into()),
        ("D02".into(), "R2mm".into()),
        ("D03".into(), "R3mm".into()),
    ]);

    assert!(matches!(
        project_fillet(&feature),
        FeatureDefinition::Fillet { groups }
            if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Unresolved { .. },
                ..
            }])
    ));
}
