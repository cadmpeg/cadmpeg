// SPDX-License-Identifier: Apache-2.0
//! Feature-history reference, projection, and write-prepare tests.
#![allow(clippy::unwrap_used)]

use super::*;

fn feature(id: &str, source_id: Option<&str>, ordinal: u32) -> Feature {
    Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: source_id.map(str::to_string),
        parent_source_id: None,
        ordinal,
        name: id.into(),
        kind: "Custom".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn feature_input_lane(id: &str, configuration: Option<&str>) -> crate::records::FeatureInputLane {
    crate::records::FeatureInputLane {
        id: id.into(),
        configuration: configuration.map(str::to_string),
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

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
            neutral_plane: FaceSelection::Unresolved,
            pull_direction: None,
            angle: Some(Angle(value)),
            outward: None,
            ..
        } if (value - std::f64::consts::PI / 60.0).abs() < 1e-12
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

#[test]
fn offset_plane_frame_resolves_one_preceding_parallel_plane() {
    let mut reference = feature("sldprt:history:feature#0:0", None, 0);
    reference.input_class = Some("moRefPlane_c".into());
    reference
        .properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    reference.properties.insert("Normal".into(), "1,0,0".into());
    reference.properties.insert("UAxis".into(), "0,0,-1".into());
    let mut offset = feature("sldprt:history:feature#0:1", None, 1);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "6mm".into());
    offset
        .properties
        .insert("Origin".into(), "6mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "1,0,0".into());
    offset.properties.insert("UAxis".into(), "0,0,1".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![reference, offset],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        &projected[1].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(bound)),
            distance: Length(6.0),
        } if bound == &projected[0].id
    ));
    assert_eq!(projected[1].dependencies, [projected[0].id.clone()]);
}

#[test]
fn coincident_plane_frame_does_not_infer_an_offset_reference() {
    let mut reference = feature("sldprt:history:feature#0:0", None, 0);
    reference.input_class = Some("moRefPlane_c".into());
    reference
        .properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    reference.properties.insert("Normal".into(), "1,0,0".into());
    reference.properties.insert("UAxis".into(), "0,0,-1".into());
    let mut offset = feature("sldprt:history:feature#0:1", None, 1);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "0mm".into());
    offset
        .properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "1,0,0".into());
    offset.properties.insert("UAxis".into(), "0,0,-1".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![reference, offset],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        &projected[1].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(0.0),
        }
    ));
}

#[test]
fn planar_face_reference_requires_one_coincident_face() {
    let surface = Surface {
        id: cadmpeg_ir::ids::SurfaceId("surface".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 12.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let face = Face {
        id: cadmpeg_ir::ids::FaceId("face".into()),
        shell: cadmpeg_ir::ids::ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };
    let surfaces = HashMap::from([(&surface.id, &surface)]);
    let mut selection = FaceSelection::Unresolved;
    resolve_planar_face_selection(
        &mut selection,
        Point3::new(5.0, -3.0, 12.0),
        Vector3::new(0.0, 0.0, -1.0),
        std::slice::from_ref(&face),
        &surfaces,
    );
    assert_eq!(selection, FaceSelection::Faces(vec![face.id.clone()]));

    let mut native = FaceSelection::Native("component-path".into());
    resolve_planar_face_selection(
        &mut native,
        Point3::new(5.0, -3.0, 12.0),
        Vector3::new(0.0, 0.0, -1.0),
        std::slice::from_ref(&face),
        &surfaces,
    );
    assert_eq!(
        native,
        FaceSelection::Resolved {
            faces: vec![face.id.clone()],
            native: "component-path".into(),
        }
    );

    let mut duplicate = face.clone();
    duplicate.id = cadmpeg_ir::ids::FaceId("duplicate".into());
    let mut ambiguous = FaceSelection::Unresolved;
    resolve_planar_face_selection(
        &mut ambiguous,
        Point3::new(0.0, 0.0, 12.0),
        Vector3::new(0.0, 0.0, 1.0),
        &[face.clone(), duplicate.clone()],
        &surfaces,
    );
    assert_eq!(ambiguous, FaceSelection::Unresolved);

    let mut split = FaceSelection::Native("historical-face".into());
    resolve_planar_face_selection(
        &mut split,
        Point3::new(0.0, 0.0, 12.0),
        Vector3::new(0.0, 0.0, 1.0),
        &[face.clone(), duplicate.clone()],
        &surfaces,
    );
    assert_eq!(
        split,
        FaceSelection::Resolved {
            faces: vec![face.id, duplicate.id],
            native: "historical-face".into(),
        }
    );
}

#[test]
fn offset_plane_face_reference_does_not_mirror_the_serialized_origin() {
    let surface = Surface {
        id: cadmpeg_ir::ids::SurfaceId("surface".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, -5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let face = Face {
        id: cadmpeg_ir::ids::FaceId("face".into()),
        shell: cadmpeg_ir::ids::ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };
    let surfaces = HashMap::from([(&surface.id, &surface)]);
    let mut selection = FaceSelection::Native("component-path".into());
    let origin = Point3::new(0.0, 0.0, 5.0);

    resolve_offset_plane_face_selection(
        &mut selection,
        origin,
        Vector3::new(0.0, 0.0, 1.0),
        std::slice::from_ref(&face),
        &surfaces,
    );

    assert_eq!(origin, Point3::new(0.0, 0.0, 5.0));
    assert_eq!(selection, FaceSelection::Native("component-path".into()));
}

#[test]
fn offset_plane_frame_does_not_bind_a_later_builtin_principal_plane() {
    let mut offset = feature("sldprt:history:feature#0:0", None, 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "6mm".into());
    offset
        .properties
        .insert("Origin".into(), "6mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "1,0,0".into());
    offset.properties.insert("UAxis".into(), "0,0,1".into());
    let mut principal = feature("sldprt:history:feature#0:1", Some("4"), 1);
    principal.name = "Right".into();
    principal.input_class = Some("moRefPlane_c".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![offset, principal],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        &projected[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(6.0),
        }
    ));
    assert!(projected[0].dependencies.is_empty());
}

#[test]
fn explicit_offset_plane_reference_cannot_bind_itself() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "0mm".into());
    offset.properties.insert("Reference".into(), "35".into());
    offset
        .properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "0,0,1".into());
    offset.properties.insert("UAxis".into(), "1,0,0".into());

    let projected = project_features(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![offset],
    }]);

    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(0.0),
        }
    ));
    assert!(projected[0].dependencies.is_empty());
}

#[test]
fn explicit_offset_plane_reference_orders_a_later_serialized_principal_first() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "6mm".into());
    offset.properties.insert("Reference".into(), "4".into());
    offset
        .properties
        .insert("Origin".into(), "6mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "1,0,0".into());
    offset.properties.insert("UAxis".into(), "0,0,-1".into());
    let mut principal = feature("sldprt:history:feature#0:1", Some("4"), 1);
    principal.name = "Right".into();
    principal.input_class = Some("moRefPlane_c".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![offset, principal],
    };

    let mut projected = project_features(&[history]);
    assert!(matches!(
        &projected[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            distance: Length(6.0),
        } if reference == &projected[1].id
    ));
    assert_eq!(projected[0].dependencies, [projected[1].id.clone()]);
    assert!(order_features_for_regeneration(&mut projected));
    assert_eq!(projected[1].ordinal, 0);
    assert_eq!(projected[0].ordinal, 1);
}

#[test]
fn explicit_principal_reference_survives_a_coincident_result_frame() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "6mm".into());
    offset.properties.insert("Reference".into(), "2".into());
    offset
        .properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "0,0,1".into());
    offset.properties.insert("UAxis".into(), "1,0,0".into());
    let mut principal = feature("sldprt:history:feature#0:1", Some("2"), 1);
    principal.name = "Front".into();
    principal.input_class = Some("moRefPlane_c".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![offset, principal],
    };

    let mut projected = project_features(&[history]);
    assert!(matches!(
        &projected[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            distance: Length(6.0),
        } if reference == &projected[1].id
    ));
    assert!(order_features_for_regeneration(&mut projected));
    assert_eq!(projected[1].ordinal, 0);
    assert_eq!(projected[0].ordinal, 1);
}

#[test]
fn incompatible_later_principal_falls_back_to_the_serialized_face_frame() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "0mm".into());
    offset.properties.insert("Reference".into(), "4".into());
    offset
        .properties
        .insert("Origin".into(), "0mm,5mm,0mm".into());
    offset.properties.insert("Normal".into(), "0,1,0".into());
    offset.properties.insert("UAxis".into(), "1,0,0".into());
    offset
        .properties
        .insert("ReferenceFaceOrigin".into(), "0mm,5mm,0mm".into());
    offset
        .properties
        .insert("ReferenceFaceNormal".into(), "0,1,0".into());
    offset
        .properties
        .insert("ReferenceFaceUAxis".into(), "1,0,0".into());
    let mut principal = feature("sldprt:history:feature#0:1", Some("4"), 1);
    principal.name = "Right".into();
    principal.input_class = Some("moRefPlane_c".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![offset, principal],
    };

    let projected = project_features(&[history]);

    assert!(matches!(
        &projected[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face {
                face: FaceSelection::Unresolved,
                ..
            }),
            distance: Length(0.0),
        }
    ));
    assert!(projected[0].dependencies.is_empty());
}

#[test]
fn explicit_offset_plane_reference_orders_a_later_derived_plane_first() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "6mm".into());
    offset.properties.insert("Reference".into(), "40".into());
    offset
        .properties
        .insert("Origin".into(), "6mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "1,0,0".into());
    offset.properties.insert("UAxis".into(), "0,1,0".into());
    let mut reference = feature("sldprt:history:feature#0:1", Some("40"), 1);
    reference.input_class = Some("moRefPlane_c".into());
    reference
        .properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    reference.properties.insert("Normal".into(), "1,0,0".into());
    reference.properties.insert("UAxis".into(), "0,1,0".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![offset, reference],
    };

    let mut projected = project_features(&[history]);

    assert!(matches!(
        &projected[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            distance: Length(6.0),
        } if reference == &projected[1].id
    ));
    assert!(order_features_for_regeneration(&mut projected));
    assert_eq!(projected[1].ordinal, 0);
    assert_eq!(projected[0].ordinal, 1);
}

#[test]
fn configuration_dependencies_participate_in_the_shared_regeneration_order() {
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("sldprt:history:feature#0:0", None, 0),
            feature("sldprt:history:feature#0:1", None, 1),
        ],
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features = project_features(&[history]);
    let predecessor = ir.model.features[1].id.clone();
    let consumer = ir.model.features[0].id.clone();
    ir.model
        .configurations
        .push(cadmpeg_ir::features::DesignConfiguration {
            id: cadmpeg_ir::features::ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true.into(),
            source_index: None,
            name: "configuration".into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            suppressed_features: Vec::new(),
            bodies: cadmpeg_ir::features::ConfigurationBodies::Unresolved,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                consumer.clone(),
                cadmpeg_ir::features::ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: vec![predecessor.clone()],
                    outputs: Vec::new(),
                    definition: ir.model.features[0].definition.clone(),
                },
            )]),
            native_ref: None,
        });

    assert!(order_model_features_for_regeneration(&mut ir));
    let ordinals = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.ordinal))
        .collect::<HashMap<_, _>>();
    assert!(ordinals[&predecessor] < ordinals[&consumer]);
    assert!(ir.model.features[0].dependencies.is_empty());
}

#[test]
fn blind_extrusion_uses_its_sole_dimension_as_depth() {
    let mut feature = feature("sldprt:history:feature#1:2", Some("12"), 2);
    feature.xml_tag = "Extrusion".into();
    feature.input_class = Some("moExtrusion_c".into());
    feature.parameters.insert("s".into(), "2.1".into());
    feature
        .properties
        .insert("EndCondition".into(), "Blind".into());

    assert!(native_parameter_is_length(&feature, "s", Some("2.1")));
    assert!(matches!(
        project_extrude(&feature, &HashMap::new(), &HashMap::new()),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(2.1)
                    },
                    ..
                }
            },
            ..
        })
    ));
}

#[test]
fn legacy_history_extrusion_uses_preceding_profile_and_sole_source_depth() {
    let mut profile = feature("sldprt:history:feature#1:0", Some("9"), 0);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    let mut extrusion = feature("sldprt:history:feature#1:1", Some("20"), 1);
    extrusion.xml_tag = "Extrusion".into();
    extrusion.kind = "localized-boss-kind".into();
    extrusion.input_class = None;
    extrusion.parameters.insert("m".into(), "6.8".into());
    extrusion.parameters.insert("aux-1".into(), "1.2".into());
    extrusion.parameters.insert("aux-2".into(), "3.4".into());
    extrusion.content = vec![
        FeatureContent::Dimension("m".into()),
        FeatureContent::Dimension("m".into()),
    ];
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![extrusion, profile],
    };

    let projected = project_features(&[history]);
    let extrusion = projected
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("sldprt:history:feature#1:1"))
        .expect("legacy extrusion feature");
    let profile = projected
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("sldprt:history:feature#1:0"))
        .expect("legacy extrusion profile");
    assert!(profile.ordinal < extrusion.ordinal);
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile_ref),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind { length: Length(6.8) },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        } if profile_ref == &profile.id
    ));
}

#[test]
fn repeated_dimension_content_projects_one_owned_parameter() {
    let mut feature = feature("sldprt:history:feature#1:2", None, 2);
    feature.parameters.insert("D1".into(), "2".into());
    feature.content = vec![
        FeatureContent::Dimension("D1".into()),
        FeatureContent::Dimension("D1".into()),
    ];

    assert_eq!(parameter_names(&feature), vec!["D1", "D1"]);
    assert_eq!(projected_parameter_names(&feature), vec!["D1"]);
    assert_eq!(
        project_feature_content(&feature, &HashMap::new()),
        vec![FeatureSourceContent::Parameter(ParameterId(
            "sldprt:model:parameter#1:2:0".into()
        ))]
    );
}

#[test]
fn spatial_profile_class_projects_a_spatial_sketch() {
    let mut spatial = feature("spatial", Some("7"), 0);
    spatial.xml_tag = "Sketch".into();
    spatial.kind = "Sketch".into();
    spatial.input_class = Some("mo3DProfileFeature_c".into());

    assert_eq!(
        project_definition(
            &spatial,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&spatial),
        ),
        FeatureDefinition::SpatialSketch { sketch: None }
    );
}

#[test]
fn base_body_class_projects_stored_geometry_independently_of_display_name() {
    let mut base_body = feature("base-body", Some("18"), 0);
    base_body.kind = "Localized imported body".into();
    base_body.input_class = Some("moBaseBody_c".into());

    assert_eq!(
        project_definition(
            &base_body,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&base_body),
        ),
        FeatureDefinition::StoredGeometry
    );
}

#[test]
fn hole_profile_dimension_order_distinguishes_counterbore_and_thread() {
    let profile = |roles: &[(&str, &str)]| {
        let mut profile = feature("profile", Some("7"), 0);
        profile.kind = "Sketch".into();
        profile.input_class = Some("moProfileFeature_c".into());
        for (name, expression) in roles {
            profile
                .parameters
                .insert((*name).into(), (*expression).into());
            profile
                .content
                .push(FeatureContent::Dimension((*name).into()));
        }
        profile
    };

    let counterbore = profile(&[
        ("a", "118°"),
        ("b", "5.7"),
        ("c", "<MOD-DIAM>9"),
        ("d", "12"),
        ("e", "<MOD-DIAM>5.5"),
    ]);
    let construction = hole_sketch_construction(&counterbore).expect("required invariant");
    assert_eq!(construction.diameter, Length(5.5));
    assert_eq!(construction.depth, Some(Length(12.0)));
    assert!(matches!(
        construction.kind,
        HoleKind::CounterboreDrilled {
            diameter: Length(9.0),
            depth: Length(5.7),
            ..
        }
    ));

    let threaded = profile(&[
        ("a", "<MOD-DIAM>4.2"),
        ("b", "12.4"),
        ("c", "<MOD-DIAM>5"),
        ("d", "10"),
        ("e", "118°"),
    ]);
    let construction = hole_sketch_construction(&threaded).expect("required invariant");
    assert_eq!(construction.diameter, Length(4.2));
    assert_eq!(construction.depth, Some(Length(12.4)));
    assert!(matches!(
        construction.kind,
        HoleKind::Threaded {
            major_diameter: Length(5.0),
            thread_depth: Length(10.0),
            pitch: None,
            ..
        }
    ));

    let tapered_thread = profile(&[
        ("a", "3.43°"),
        ("b", "6.92"),
        ("c", "118°"),
        ("d", "<MOD-DIAM>8.43"),
        ("e", "11.62"),
        ("f", "<MOD-DIAM>10.29"),
    ]);
    let construction = hole_sketch_construction(&tapered_thread).expect("tapered thread profile");
    assert_eq!(construction.diameter, Length(8.43));
    assert_eq!(construction.depth, Some(Length(11.62)));
    assert!(matches!(
        construction.kind,
        HoleKind::Threaded {
            major_diameter: Length(10.29),
            thread_depth: Length(6.92),
            pitch: None,
            drill_point_angle: Angle(angle),
        } if (angle - 118_f64.to_radians()).abs() < 1.0e-12
    ));
    assert_eq!(
        construction.bottom,
        Some(HoleBottom::Angled {
            included_angle: Angle(118_f64.to_radians()),
            depth_to_tip: false,
        })
    );
    assert_eq!(construction.taper_angle, Some(Angle(3.43_f64.to_radians())));

    let counterbore_with_exit_countersink = profile(&[
        ("a", "4.6"),
        ("b", "<MOD-DIAM>8"),
        ("c", "90°"),
        ("d", "10"),
        ("e", "<MOD-DIAM>4.5"),
        ("f", "<MOD-DIAM>4.55"),
    ]);
    let construction =
        hole_sketch_construction(&counterbore_with_exit_countersink).expect("dual-ended profile");
    assert_eq!(construction.diameter, Length(4.5));
    assert_eq!(construction.depth, Some(Length(10.0)));
    assert_eq!(
        construction.kind,
        HoleKind::Counterbore {
            diameter: Length(8.0),
            depth: Length(4.6),
        }
    );
    assert_eq!(
        construction.exit_kind,
        Some(HoleKind::Countersink {
            diameter: Length(4.55),
            angle: Angle(std::f64::consts::FRAC_PI_2),
        })
    );

    let counterdrill = profile(&[
        ("a", "12.4"),
        ("b", "<MOD-DIAM>5.5"),
        ("c", "118°"),
        ("d", "<MOD-DIAM>10.05"),
        ("e", "90°"),
        ("f", "5.4"),
        ("g", "<MOD-DIAM>9.95"),
    ]);
    let construction = hole_sketch_construction(&counterdrill).expect("counterdrill profile");
    assert_eq!(construction.diameter, Length(5.5));
    assert_eq!(construction.depth, Some(Length(12.4)));
    assert_eq!(
        construction.kind,
        HoleKind::Counterdrill {
            diameter: Length(9.95),
            entry_diameter: Some(Length(10.05)),
            depth: Length(5.4),
            angle: Angle(std::f64::consts::FRAC_PI_2),
        }
    );
    assert_eq!(
        construction.bottom,
        Some(HoleBottom::Angled {
            included_angle: Angle(118_f64.to_radians()),
            depth_to_tip: false,
        })
    );

    let placement_dimensions = profile(&[
        ("a", "<MOD-DIAM>9"),
        ("b", "6"),
        ("c", "4"),
        ("d", "4"),
        ("e", "6"),
    ]);
    assert!(hole_sketch_construction(&placement_dimensions).is_none());

    let unsupported_countersink = profile(&[
        ("diameter", "<MOD-DIAM>5"),
        ("entry", "<MOD-DIAM>9"),
        ("depth", "6"),
        ("angle", "82°"),
    ]);
    assert!(hole_sketch_construction(&unsupported_countersink).is_none());

    let unsupported_counterbore = profile(&[
        ("diameter", "<MOD-DIAM>5"),
        ("entry", "<MOD-DIAM>9"),
        ("entry depth", "3"),
        ("depth", "6"),
    ]);
    assert!(hole_sketch_construction(&unsupported_counterbore).is_none());

    let mut native_profile = profile(&[("diameter", "<MOD-DIAM>6.6"), ("depth", "9.4")]);
    native_profile.id = "native-profile".into();
    native_profile.source_id = None;
    let mut native_owned = feature("native-owned-hole", None, 0);
    native_owned
        .properties
        .insert("DissectableChildren".into(), native_profile.id.clone());
    let projected = project_hole(
        &native_owned,
        &HashMap::new(),
        &[native_owned.clone(), native_profile],
    );
    assert!(matches!(
        projected,
        FeatureDefinition::Hole {
            diameter: Some(Length(6.6)),
            extent: Some(Termination::Blind {
                length: Length(9.4)
            }),
            ..
        }
    ));

    let mut canonical = feature("hole", Some("8"), 0);
    canonical.parameters = [
        ("Diameter".into(), "4.2mm".into()),
        ("Depth".into(), "12.4mm".into()),
        ("ThreadMajorDiameter".into(), "5mm".into()),
        ("ThreadDepth".into(), "10mm".into()),
        ("DrillPointAngle".into(), "118°".into()),
    ]
    .into();
    let projected = project_hole(
        &canonical,
        &HashMap::new(),
        std::slice::from_ref(&canonical),
    );
    let FeatureDefinition::Hole {
        kind:
            HoleKind::Threaded {
                major_diameter,
                thread_depth,
                ..
            },
        diameter: Some(diameter),
        extent: Some(Termination::Blind { length }),
        ..
    } = projected
    else {
        panic!("expected canonical threaded hole: {projected:?}");
    };
    assert!((diameter.0 - 4.2).abs() < 1.0e-12);
    assert!((major_diameter.0 - 5.0).abs() < 1.0e-12);
    assert!((thread_depth.0 - 10.0).abs() < 1.0e-12);
    assert!((length.0 - 12.4).abs() < 1.0e-12);
}

#[test]
fn scene_class_binds_only_its_explicit_source_identifier() {
    let mut first = feature("first", Some("153"), 0);
    first.kind = "localized light".into();
    let mut second = feature("second", Some("155"), 1);
    second.kind = first.kind.clone();
    let mut singleton = feature("singleton", Some("200"), 2);
    singleton.kind = "unrelated".into();
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second, singleton],
    }];
    let scene = crate::tessellation::SceneFeatureClasses {
        by_source: HashMap::from([("153".into(), "moDirectionLight_c".into())]),
    };

    enrich_scene_classes(&mut histories, &scene);

    assert_eq!(
        histories[0].features[0].input_class.as_deref(),
        Some("moDirectionLight_c")
    );
    assert_eq!(histories[0].features[1].input_class, None);
    assert_eq!(histories[0].features[2].input_class, None);
}

#[test]
fn structurally_stable_feature_manager_nodes_use_source_identity() {
    let roster = |node: &Feature| {
        let mut roster = vec![node.clone()];
        for (source, class) in [
            ("7", "moDocsFolder_c"),
            ("8", "moCommentsFolder_c"),
            ("9", "moSolidBodyFolder_c"),
            ("10", "moSurfaceBodyFolder_c"),
        ] {
            let mut sentinel = feature("sentinel", Some(source), roster.len() as u32);
            sentinel.input_class = Some(class.into());
            roster.push(sentinel);
        }
        roster
    };
    let cases = [
        ("1", FeatureTreeNodeRole::Annotations),
        ("5", FeatureTreeNodeRole::ModelOrigin),
        ("6", FeatureTreeNodeRole::LightsAndCameras),
        ("12", FeatureTreeNodeRole::AmbientLight),
        ("13", FeatureTreeNodeRole::DirectionalLight),
        ("14", FeatureTreeNodeRole::DirectionalLight),
        ("15", FeatureTreeNodeRole::DirectionalLight),
    ];

    for (source_id, expected) in cases {
        let mut node = feature("node", Some(source_id), 0);
        node.kind = "任意本地化標籤".into();
        if source_id == "5" {
            node.xml_tag = "Sketch".into();
        }
        assert_eq!(
            feature_tree_node_role(&node, &roster(&node)),
            Some(expected)
        );
    }

    let mut fourth_light = feature("fourth", Some("70"), 0);
    fourth_light.kind = "本地化方向光".into();
    let mut directional_roster = roster(&fourth_light);
    let mut first_light = feature("light", Some("13"), 13);
    first_light.kind = fourth_light.kind.clone();
    directional_roster.push(first_light);
    assert_eq!(
        feature_tree_node_role(&fourth_light, &directional_roster),
        Some(FeatureTreeNodeRole::DirectionalLight)
    );

    let mut additional_ambient = feature("additional ambient", Some("16"), 0);
    additional_ambient.kind = "本地化环境光".into();
    let mut ambient_roster = roster(&additional_ambient);
    let mut reserved_ambient = feature("ambient", Some("12"), 12);
    reserved_ambient.kind = additional_ambient.kind.clone();
    ambient_roster.push(reserved_ambient);
    assert_eq!(
        feature_tree_node_role(&additional_ambient, &ambient_roster),
        Some(FeatureTreeNodeRole::AmbientLight)
    );

    let legacy_roster = |node: &Feature| {
        let mut roster = vec![node.clone()];
        for (source, class) in [
            ("6", "moOriginProfileFeature_c"),
            ("9", "moSurfaceBodyFolder_c"),
            ("10", "moSolidBodyFolder_c"),
            ("12", "moDocsFolder_c"),
            ("13", "moCommentsFolder_c"),
        ] {
            let mut sentinel = feature("sentinel", Some(source), roster.len() as u32);
            sentinel.input_class = Some(class.into());
            roster.push(sentinel);
        }
        roster
    };
    for (source, expected) in [
        ("2", FeatureTreeNodeRole::LightsAndCameras),
        ("7", FeatureTreeNodeRole::AmbientLight),
        ("8", FeatureTreeNodeRole::DirectionalLight),
    ] {
        let node = feature("legacy", Some(source), 0);
        assert_eq!(
            feature_tree_node_role(&node, &legacy_roster(&node)),
            Some(expected)
        );
    }
    let legacy_lights = feature("legacy lights", Some("2"), 0);
    let mut complete_legacy_roster = legacy_roster(&legacy_lights);
    for (source, class) in [
        ("1", "moDetailCabinet_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moRefPlane_c"),
    ] {
        let mut sentinel = feature(
            "legacy frame",
            Some(source),
            complete_legacy_roster.len() as u32,
        );
        sentinel.input_class = Some(class.into());
        complete_legacy_roster.push(sentinel);
    }
    for source in ["7", "8"] {
        complete_legacy_roster.push(feature(
            "legacy light",
            Some(source),
            complete_legacy_roster.len() as u32,
        ));
    }
    assert_eq!(
        feature_tree_node_role(&legacy_lights, &complete_legacy_roster),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let roster_from = |node: &Feature, classes: &[(&str, &str)], classless_sources: &[&str]| {
        let mut features = vec![node.clone()];
        for (source, class) in classes {
            let mut sentinel = feature("sentinel", Some(source), features.len() as u32);
            sentinel.input_class = Some((*class).into());
            features.push(sentinel);
        }
        for source in classless_sources {
            features.push(feature("reserved", Some(source), features.len() as u32));
        }
        features
    };
    let default_frame = [
        ("1", "moDetailCabinet_c"),
        ("2", "moRefPlane_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moOriginProfileFeature_c"),
    ];
    let lights = feature("lights", Some("6"), 0);
    assert_eq!(
        feature_tree_node_role(&lights, &roster_from(&lights, &default_frame, &["7", "8"])),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let ambient = feature("ambient", Some("10"), 0);
    let mut folders_at_seven = default_frame.to_vec();
    folders_at_seven.extend([("7", "moSolidBodyFolder_c"), ("8", "moSurfaceBodyFolder_c")]);
    assert_eq!(
        feature_tree_node_role(
            &ambient,
            &roster_from(&ambient, &folders_at_seven, &["6", "11", "12"]),
        ),
        Some(FeatureTreeNodeRole::AmbientLight)
    );

    let early_lights = feature("lights", Some("2"), 0);
    let origin_at_six = [
        ("1", "moDetailCabinet_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moRefPlane_c"),
        ("6", "moOriginProfileFeature_c"),
    ];
    assert_eq!(
        feature_tree_node_role(
            &early_lights,
            &roster_from(&early_lights, &origin_at_six, &["7", "8"]),
        ),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let ambiguous = feature("node", Some("99"), 0);
    assert_eq!(feature_tree_node_role(&ambiguous, &[]), None);

    let mut exploded_views = ambiguous.clone();
    exploded_views.name.clear();
    assert_eq!(
        feature_tree_node_role(&exploded_views, &roster(&exploded_views)),
        Some(FeatureTreeNodeRole::ExplodedViews)
    );

    let mut reference_plane = feature("node", Some("5"), 0);
    reference_plane.input_class = Some("moRefPlane_c".into());
    assert_eq!(feature_tree_node_role(&reference_plane, &[]), None);

    let mut sheet_metal = feature("node", Some("-1"), 0);
    sheet_metal.name.clear();
    assert_eq!(
        feature_tree_node_role(&sheet_metal, &roster(&sheet_metal)),
        Some(FeatureTreeNodeRole::SheetMetal)
    );
    sheet_metal.name = "任意本地化鈑金根節點".into();
    assert_eq!(
        feature_tree_node_role(&sheet_metal, &roster(&sheet_metal)),
        Some(FeatureTreeNodeRole::SheetMetal)
    );
    assert_eq!(feature_tree_node_role(&sheet_metal, &[]), None);
}

#[test]
fn sketch_block_instances_bind_to_adjacent_typed_definition_objects() {
    let mut instance = feature("instance", Some("25"), 1);
    instance.input_class = Some("moSketchBlockInst_c".into());
    let mut compact_instance = feature("compact instance", Some("34"), 2);
    compact_instance.input_class = Some("moSketchBlockInst_c".into());
    let mut definition = feature("definition", Some("23"), 0);
    definition.input_class = Some("moSketchBlockDef_c".into());
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![definition, instance, compact_instance],
    }];
    let mut lane = feature_input_lane("lane", None);
    lane.native_payload.resize(500, 0);
    let write_local_id = |payload: &mut [u8], offset: usize, token: [u8; 4], local_id: u16| {
        payload[offset..offset + 4].copy_from_slice(&[0xff; 4]);
        payload[offset + 4..offset + 8].copy_from_slice(&token);
        payload[offset + 12..offset + 18].copy_from_slice(&[0x02, 0, 0, 0, 0, 0]);
        payload[offset + 18..offset + 20].copy_from_slice(&local_id.to_le_bytes());
        payload[offset + 40..offset + 44].copy_from_slice(&[0, 0, 1, 0]);
    };
    write_local_id(&mut lane.native_payload, 180, [0x11, 0x22, 0x33, 0x01], 0);
    write_local_id(
        &mut lane.native_payload,
        250,
        [0x11, 0x22, 0x33, 0x01],
        0x0115,
    );
    lane.native_payload[294..296].copy_from_slice(&[0x26, 0x81]);
    for (index, value) in [0.00575_f64, -0.169, 0.0].into_iter().enumerate() {
        let start = 296 + index * 8;
        lane.native_payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    lane.native_payload[388..390].copy_from_slice(&0x0115_u16.to_le_bytes());
    write_local_id(
        &mut lane.native_payload,
        420,
        [0x44, 0x55, 0x66, 0x01],
        0x0115,
    );
    lane.native_payload[464..466].copy_from_slice(&[0x73, 0x81]);
    for (index, value) in [0.01075_f64, -0.132, 0.0].into_iter().enumerate() {
        let start = 466 + index * 8;
        lane.native_payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    lane.names = vec![
        crate::records::FeatureInputName {
            id: "instance-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            object_id: Some(25),
            value: "instance".into(),
        },
        crate::records::FeatureInputName {
            id: "definition-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 140,
            object_id: Some(23),
            value: "definition".into(),
        },
        crate::records::FeatureInputName {
            id: "compact-instance-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 340,
            object_id: Some(34),
            value: "compact".into(),
        },
    ];

    crate::resolved_features::reference_geometry::enrich_history_sketch_block_references(
        &mut histories,
        &[lane],
    );

    assert_eq!(
        histories[0].features[1]
            .properties
            .get("BlockDefinition")
            .map(String::as_str),
        Some("23")
    );
    assert_eq!(
        histories[0].features[1]
            .properties
            .get("BlockOrigin")
            .map(String::as_str),
        Some("5.75mm,-169mm,0mm")
    );
    assert_eq!(
        histories[0].features[2]
            .properties
            .get("BlockOrigin")
            .map(String::as_str),
        Some("10.75mm,-132mm,0mm")
    );
    assert_eq!(
        histories[0].features[2]
            .properties
            .get("BlockDefinition")
            .map(String::as_str),
        Some("23")
    );
}

#[test]
fn principal_plane_requires_the_reference_plane_native_class() {
    let mut plane = feature("plane", Some("2"), 0);
    assert_eq!(crate::classification::principal_plane(&plane), None);
    plane.input_class = Some("moRefPlane_c".into());
    assert_eq!(
        crate::classification::principal_plane(&plane),
        Some(cadmpeg_ir::features::PrincipalPlane::Front)
    );
}

#[test]
fn shifted_reserved_triplet_does_not_classify_principal_planes() {
    let mut scene = feature("scene", Some("2"), 0);
    let mut front = feature("front", Some("3"), 1);
    let mut top = feature("top", Some("4"), 2);
    let mut right = feature("right", Some("5"), 3);
    for plane in [&mut front, &mut top, &mut right] {
        plane.input_class = Some("moRefPlane_c".into());
    }
    scene.input_class = Some("moSceneFolder_c".into());
    let features = [scene, front.clone(), top.clone(), right.clone()];
    let by_source = features
        .iter()
        .filter_map(|feature| Some((feature.source_id.as_deref()?, feature)))
        .collect::<HashMap<_, _>>();

    assert_eq!(
        principal_plane_in_history(&front, &by_source, &features),
        None
    );
    assert_eq!(
        principal_plane_in_history(&top, &by_source, &features),
        None
    );
    assert_eq!(
        principal_plane_in_history(&right, &by_source, &features),
        None
    );
}

#[test]
fn angular_plane_parameter_does_not_claim_offset_semantics() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());
    plane.parameters.insert("D1".into(), "0rad".into());
    plane
        .properties
        .insert("Origin".into(), "0mm,70mm,0mm".into());
    plane.properties.insert("Normal".into(), "0,1,0".into());
    plane.properties.insert("UAxis".into(), "-1,0,0".into());

    assert!(!is_offset_plane(&plane));
    assert_eq!(
        project_definition(
            &plane,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&plane),
        ),
        FeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 70.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(-1.0, 0.0, 0.0),
        }
    );
}

#[test]
fn length_plane_parameter_claims_offset_semantics() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());
    plane.parameters.insert("D1".into(), "70mm".into());

    assert!(is_offset_plane(&plane));
    assert_eq!(
        project_definition(
            &plane,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&plane),
        ),
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(70.0),
        }
    );
}

#[test]
fn frameless_reference_plane_remains_typed_unresolved() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());

    assert_eq!(
        project_definition(
            &plane,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&plane),
        ),
        FeatureDefinition::DatumPlaneUnresolved
    );
}

#[test]
fn legacy_principal_plane_requires_a_complete_matching_triplet() {
    let front = feature("front", Some("2"), 0);
    let top = feature("top", Some("3"), 1);
    let right = feature("right", Some("4"), 2);
    let features = [&front, &top, &right]
        .into_iter()
        .map(|feature| {
            (
                feature.source_id.as_deref().expect("required invariant"),
                feature,
            )
        })
        .collect::<HashMap<_, _>>();
    assert_eq!(
        principal_plane_in_history(&front, &features, &[]),
        Some(cadmpeg_ir::features::PrincipalPlane::Front)
    );

    let mut mismatched = right.clone();
    mismatched.kind = "Different".into();
    let features = [&front, &top, &mismatched]
        .into_iter()
        .map(|feature| {
            (
                feature.source_id.as_deref().expect("required invariant"),
                feature,
            )
        })
        .collect::<HashMap<_, _>>();
    assert_eq!(principal_plane_in_history(&front, &features, &[]), None);
}

#[test]
fn idless_legacy_principal_planes_require_an_exact_bounded_triplet() {
    let front = feature("front", None, 10);
    let top = feature("top", None, 11);
    let right = feature("right", None, 12);
    let mut successor = feature("origin", None, 13);
    successor.kind = "Other".into();
    let records = [front.clone(), top.clone(), right.clone(), successor.clone()];

    assert_eq!(
        principal_plane_in_history(&front, &HashMap::new(), &records),
        Some(cadmpeg_ir::features::PrincipalPlane::Front)
    );

    let mut unbounded = records.clone();
    unbounded[3].kind = unbounded[0].kind.clone();
    assert_eq!(
        principal_plane_in_history(&front, &HashMap::new(), &unbounded),
        None
    );

    let second_front = feature("front-2", None, 20);
    let second_top = feature("top-2", None, 21);
    let second_right = feature("right-2", None, 22);
    let mut second_successor = feature("origin-2", None, 23);
    second_successor.kind = "Other".into();
    let ambiguous = [
        front,
        top,
        right,
        successor,
        second_front,
        second_top,
        second_right,
        second_successor,
    ];
    assert_eq!(
        principal_plane_in_history(&ambiguous[0], &HashMap::new(), &ambiguous),
        None
    );
}

#[test]
fn custom_properties_are_document_attributes_not_model_features() {
    let mut property = feature("property", None, 0);
    property.xml_tag = "CustomProperty".into();
    property.name = "PartNumber".into();
    property.text = Some("A-123".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![property],
    };

    assert!(project_features(std::slice::from_ref(&history)).is_empty());
    let attributes = custom_property_attributes(std::slice::from_ref(&history));
    assert_eq!(attributes.len(), 1);
    assert_eq!(attributes[0].name, "PartNumber");
    assert_eq!(
        attributes[0].values,
        vec![AttributeValue::String("A-123".into())]
    );

    let mut native = Some(crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: vec![history],
        feature_input_lanes: Vec::new(),
        pmi_dimensions: Vec::new(),
    });
    sync_neutral_features(&[], &[], &[], &mut native).expect("required invariant");
    assert_eq!(
        native.expect("required invariant").feature_histories[0]
            .features
            .len(),
        1
    );
}

#[test]
fn native_attribute_records_are_metadata_not_model_features() {
    let mut definition = feature("definition", Some("-1"), 0);
    definition.name = "VendorSettings.1".into();
    definition
        .parameters
        .insert("VendorSettings.1".into(), "0".into());
    let mut attribute = feature("attribute", Some("27"), 1);
    attribute.name = "VendorSettings.14236".into();
    attribute.input_class = Some("moAttribute_c".into());
    let mut comments = feature("comments", Some("28"), 2);
    comments.input_class = Some("moConfigCommentsFolder_c".into());
    let mut alignment = feature("alignment", Some("29"), 3);
    alignment.input_class = Some("moAlignGroup_c".into());
    let mut model = feature("model", Some("30"), 4);
    model.xml_tag = "Sketch".into();
    model.kind = "Sketch".into();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![definition, attribute, comments, alignment, model],
    };

    let projected = project_features(std::slice::from_ref(&history));
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].native_ref.as_deref(), Some("model"));
    assert!(project_parameters(&[history]).is_empty());
}

#[test]
fn native_attribute_definition_type_is_metadata_without_an_instance_name_match() {
    let mut definition = feature("definition", Some("-1"), 0);
    definition.kind = "Attribute-Definition".into();
    definition.name = "NativeAttributeFamily".into();
    definition
        .parameters
        .insert("NativeAttributeFamily".into(), "0".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![definition],
    };

    assert!(project_features(std::slice::from_ref(&history)).is_empty());
    assert!(project_parameters(&[history]).is_empty());
}

#[test]
fn configuration_snapshots_preserve_base_tree_node_roles() {
    let light = feature("light", Some("30"), 0);
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![light],
    };
    let mut configured = project_features(std::slice::from_ref(&history));
    assert!(matches!(
        configured[0].definition,
        FeatureDefinition::Native { .. }
    ));
    let mut base = configured.clone();
    base[0].definition = FeatureDefinition::TreeNode {
        role: FeatureTreeNodeRole::DirectionalLight,
        children: Vec::new(),
        active_child: None,
    };

    restore_configuration_tree_node_definitions(&mut configured, &base);
    assert!(matches!(
        configured[0].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::DirectionalLight,
            ..
        }
    ));
}

#[test]
fn simple_hole_uses_its_profile_dimension_roles() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties
        .insert("DissectableChildren".into(), "213,212".into());
    let mut position = feature("position", Some("213"), 1);
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    let mut profile = feature("profile", Some("212"), 1);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile
        .parameters
        .insert("localized diameter".into(), "<MOD-DIAM>4.5".into());
    profile
        .parameters
        .insert("localized depth".into(), "13.2".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, position, profile],
    };

    let projected = project_features(std::slice::from_ref(&history));
    let FeatureDefinition::Hole {
        diameter, extent, ..
    } = &projected[0].definition
    else {
        panic!("expected a hole definition");
    };
    assert_eq!(*diameter, Some(Length(4.5)));
    assert_eq!(
        *extent,
        Some(Termination::Blind {
            length: Length(13.2)
        })
    );

    let mut ambiguous = history;
    ambiguous.features[2]
        .parameters
        .insert("another length".into(), "2".into());
    let ambiguous = project_features(&[ambiguous]);
    let FeatureDefinition::Hole {
        diameter, extent, ..
    } = &ambiguous[0].definition
    else {
        panic!("expected a hole definition");
    };
    assert_eq!(*diameter, None);
    assert_eq!(*extent, None);
}

#[test]
fn hole_wizard_rejects_unsupported_countersink_child_schema() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties
        .insert("DissectableChildren".into(), "213,212".into());
    let mut position = feature("position", Some("213"), 1);
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.parameters.insert("D1".into(), "11".into());
    let mut profile = feature("profile", Some("212"), 2);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile
        .parameters
        .insert("localized bore".into(), "<MOD-DIAM>3.4".into());
    profile
        .parameters
        .insert("localized depth".into(), "3".into());
    profile
        .parameters
        .insert("localized entry".into(), "<MOD-DIAM>6.6".into());
    profile
        .parameters
        .insert("localized angle".into(), "90°".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, position, profile],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Simple,
            diameter: None,
            extent: None,
            ..
        }
    ));
}

#[test]
fn hole_wizard_drill_point_profile_retains_bore_and_blind_depth() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties
        .insert("DissectableChildren".into(), "212".into());
    let mut profile = feature("profile", Some("212"), 1);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    profile
        .parameters
        .insert("螺纹孔钻头直径".into(), "<MOD-DIAM>4.2".into());
    profile
        .parameters
        .insert("螺纹孔钻头深度".into(), "10".into());
    profile.parameters.insert("导头角度".into(), "118°".into());
    profile.content.extend([
        FeatureContent::Dimension("导头角度".into()),
        FeatureContent::Dimension("螺纹孔钻头深度".into()),
        FeatureContent::Dimension("螺纹孔钻头直径".into()),
    ]);
    profile
        .parameters
        .insert("derived native scalar".into(), "937.25".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, profile],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::SimpleDrilled {
                drill_point_angle: Angle(drill_point_angle),
            },
            diameter: Some(Length(4.2)),
            extent: Some(Termination::Blind {
                length: Length(10.0),
            }),
            ..
        } if (drill_point_angle - 118.0_f64.to_radians()).abs() < 1.0e-12
    ));
}

#[test]
fn native_scalar_refresh_preserves_radial_dimension_semantics() {
    let profile = feature("profile", Some("212"), 1);

    assert_eq!(
        format_native_scalar(&profile, "bore", 0.0042, Some("<MOD-DIAM>4.2")),
        "<MOD-DIAM>4.2"
    );
    assert_eq!(
        format_native_scalar(&profile, "radius", 0.003, Some("&lt;MOD-RHO&gt;3")),
        "&lt;MOD-RHO&gt;3"
    );
}

#[test]
fn legacy_revolve_uses_d1_angle_and_cut_class_operation() {
    let mut revolve = feature("revolve", Some("42"), 0);
    revolve.input_class = Some("moRevCut_c".into());
    revolve.parameters.insert("D1".into(), "360°".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![revolve],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle: Angle(value) }
                }),
                ..
            },
            op: BooleanOp::Cut,
        } if (value - std::f64::consts::TAU).abs() < 1.0e-12
    ));
}

#[test]
fn localized_cut_extrusion_uses_its_native_class_operation() {
    let mut cut = feature("cut", Some("43"), 0);
    cut.kind = "BossExtrude".into();
    cut.input_class = Some("moCut_c".into());
    cut.parameters.insert("D1".into(), "45".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![cut],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Extrude {
            op: BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn revolve_uses_its_ordered_angle_dimension_name() {
    let mut revolve = feature("revolve", Some("42"), 0);
    revolve.input_class = Some("moRevolution_c".into());
    revolve.parameters.insert("FIX_1".into(), "360°".into());
    revolve
        .content
        .push(FeatureContent::Dimension("FIX_1".into()));
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![revolve],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle: Angle(value) }
                }),
                ..
            },
            ..
        } if (value - std::f64::consts::TAU).abs() < 1.0e-12
    ));
}

#[test]
fn chamfer_uses_physical_types_of_ordered_localized_dimensions() {
    let mut chamfer = feature("chamfer", Some("42"), 0);
    chamfer.input_class = Some("Chamfer_c".into());
    chamfer
        .parameters
        .insert("localized length".into(), "1.5".into());
    chamfer
        .parameters
        .insert("localized angle".into(), "45°".into());
    chamfer
        .content
        .push(FeatureContent::Dimension("localized length".into()));
    chamfer
        .content
        .push(FeatureContent::Dimension("localized angle".into()));
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![chamfer],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Chamfer { ref groups, .. }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::ChamferGroup {
                    spec: ChamferSpec::DistanceAngle {
                        distance: Length(1.5),
                        angle: Angle(value),
                    },
                    ..
                }] if (*value - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12
            )
    ));

    let mut distance = feature("distance", Some("43"), 0);
    distance.input_class = Some("Chamfer_c".into());
    distance
        .parameters
        .insert("localized distance".into(), "2mm".into());
    distance
        .content
        .push(FeatureContent::Dimension("localized distance".into()));
    assert!(matches!(
        project_chamfer(&distance),
        FeatureDefinition::Chamfer { ref groups, .. }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::ChamferGroup {
                    spec: ChamferSpec::Distance {
                        distance: Length(2.0),
                    },
                    ..
                }]
            )
    ));

    distance
        .parameters
        .insert("localized second distance".into(), "3mm".into());
    distance.content.push(FeatureContent::Dimension(
        "localized second distance".into(),
    ));
    assert!(matches!(
        project_chamfer(&distance),
        FeatureDefinition::Chamfer { ref groups, .. }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::ChamferGroup {
                    spec: ChamferSpec::TwoDistances {
                        first: Length(2.0),
                        second: Length(3.0),
                    },
                    ..
                }]
            )
    ));
}

#[test]
fn cosmetic_thread_retains_nominal_diameter_and_blind_length() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread.parameters.insert("D1".into(), "16".into());
    thread.parameters.insert("D2".into(), "<MOD-DIAM>8".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected = project_features(&[history]);
    assert_eq!(
        projected[0].definition,
        FeatureDefinition::CosmeticThread {
            face: FaceSelection::Unresolved,
            diameter: Some(Length(8.0)),
            extent: Some(CosmeticThreadExtent::Blind {
                length: Length(16.0),
            }),
        }
    );
}

#[test]
fn cosmetic_thread_without_blind_length_is_through() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread.parameters.insert("D2".into(), "<MOD-DIAM>8".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected = project_features(&[history]);
    assert_eq!(
        projected[0].definition,
        FeatureDefinition::CosmeticThread {
            face: FaceSelection::Unresolved,
            diameter: Some(Length(8.0)),
            extent: Some(CosmeticThreadExtent::Through),
        }
    );
}

#[test]
fn cosmetic_thread_non_length_d1_and_named_diameter_are_through() {
    for d1 in ["0", "6.2831853071796rad"] {
        let mut thread = feature("thread", Some("42"), 0);
        thread.input_class = Some("moCosmeticThread_c".into());
        thread.parameters.insert("D1".into(), d1.into());
        thread
            .parameters
            .insert("thread size".into(), "<MOD-DIAM>4.9".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![thread],
        };

        let projected = project_features(&[history]);
        assert_eq!(
            projected[0].definition,
            FeatureDefinition::CosmeticThread {
                face: FaceSelection::Unresolved,
                diameter: Some(Length(4.9)),
                extent: Some(CosmeticThreadExtent::Through),
            }
        );
    }
}

#[test]
fn cosmetic_thread_requires_one_named_diameter() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread
        .parameters
        .insert("major".into(), "<MOD-DIAM>8".into());
    thread
        .parameters
        .insert("minor".into(), "<MOD-DIAM>6.8".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected = project_features(&[history]);
    let FeatureDefinition::CosmeticThread { diameter, .. } = &projected[0].definition else {
        panic!("expected a cosmetic thread");
    };
    assert_eq!(*diameter, None);
}

#[test]
fn cosmetic_thread_inherits_one_threaded_hole_major_diameter() {
    let mut hole = feature("hole", Some("10"), 0);
    hole.input_class = Some("moHoleWzd_c".into());
    hole.properties
        .insert("DissectableChildren".into(), "11".into());

    let mut profile = feature("profile", Some("11"), 1);
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    profile.parameters = [
        ("bore".into(), "<MOD-DIAM>2.5".into()),
        ("drill depth".into(), "7.5".into()),
        ("major".into(), "<MOD-DIAM>3".into()),
        ("thread depth".into(), "6".into()),
        ("angle".into(), "118°".into()),
    ]
    .into();
    profile.content = ["bore", "drill depth", "major", "thread depth", "angle"]
        .into_iter()
        .map(|name| FeatureContent::Dimension(name.into()))
        .collect();

    let mut thread = feature("thread", Some("12"), 2);
    thread.input_class = Some("moCosmeticThread_c".into());
    let thread_id = thread.id.clone();
    let hole_id = hole.id.clone();
    let mut history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, profile, thread],
    };
    let mut lane = feature_input_lane("lane", None);
    lane.surface_selections
        .push(crate::records::FeatureInputSurfaceSelection {
            id: "selection".into(),
            parent: lane.id.clone(),
            ordinal: 0,
            offset: 0,
            selector: 0,
            object_name_ref: "thread-name".into(),
            feature_ref: thread_id,
            producer_feature_refs: vec![hole_id.clone()],
            terminal_feature_ref: Some(hole_id),
            components: Vec::new(),
        });

    crate::resolved_features::holes::enrich_history_cosmetic_thread_diameters(
        std::slice::from_mut(&mut history),
        &[lane],
    );
    assert_eq!(
        history.features[2].parameters.get("D2"),
        Some(&"<MOD-DIAM>3mm".to_string())
    );
}

#[test]
fn profile_consumers_require_a_regeneration_profile() {
    let mut definition = FeatureDefinition::Extrude {
        profile: ProfileRef::Native("sketch-native".into()),
        direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Unresolved,
                draft: None,
                offset: None,
            },
        },
        op: BooleanOp::Unresolved,
        direction_source: None,
        solid: None,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());

    assert!(!bind_definition_sketch(
        &mut definition,
        "sketch-native",
        &FeatureId("sketch-feature".into()),
        &sketch,
        false,
    ));
    assert!(matches!(
        definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(_),
            ..
        }
    ));
    assert!(bind_definition_sketch(
        &mut definition,
        "sketch-native",
        &FeatureId("sketch-feature".into()),
        &sketch,
        true,
    ));
    assert!(matches!(
        definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(ref bound),
            ..
        } if bound == &sketch
    ));
}

#[test]
fn exact_native_profile_source_projects_a_feature_dependency() {
    let mut sketch = feature("sketch", Some("42"), 0);
    sketch.kind = "Sketch".into();
    sketch.input_class = Some("moProfileFeature_c".into());
    let mut extrusion = feature("extrusion", Some("43"), 1);
    extrusion.kind = "Extrusion".into();
    extrusion.input_class = Some("moExtrusion_c".into());
    extrusion.properties.insert("Profile".into(), "42".into());
    extrusion
        .properties
        .insert("Operation".into(), "Join".into());
    extrusion.parameters.insert("D1".into(), "5".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![sketch, extrusion],
    };

    let projected = project_features(&[history]);
    let sketch_id = neutral_feature_id("sketch");
    assert!(matches!(
        &projected[1].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &sketch_id
    ));
    assert_eq!(projected[1].dependencies, [sketch_id]);
}

fn design_configuration(
    id: &str,
    ordinal: u32,
    source_index: Option<u32>,
    native_ref: Option<&str>,
) -> DesignConfiguration {
    DesignConfiguration {
        id: ConfigurationId(id.into()),
        ordinal,
        active: false.into(),
        source_index,
        name: id.into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: native_ref.map(str::to_string),
    }
}

fn native_configuration(id: &str, ordinal: u32, source_index: Option<u32>) -> Configuration {
    Configuration {
        id: id.into(),
        parent: "history".into(),
        ordinal,
        source_index,
        name: id.into(),
        material: None,
        properties: BTreeMap::new(),
    }
}

fn with_configuration_id(mut configuration: DesignConfiguration, id: u32) -> DesignConfiguration {
    configuration.properties.insert("id".into(), id.to_string());
    configuration
}

fn native_with_configuration_id(mut configuration: Configuration, id: u32) -> Configuration {
    configuration.properties.insert("id".into(), id.to_string());
    configuration
}

fn native_with_configuration_lanes(
    configurations: Vec<Configuration>,
    lanes: Vec<crate::records::FeatureInputLane>,
) -> crate::native::SldprtNative {
    crate::native::SldprtNative {
        feature_histories: vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations,
            features: Vec::new(),
        }],
        feature_input_lanes: lanes,
        ..crate::native::SldprtNative::default()
    }
}
#[test]
fn repeated_aliases_from_one_parameter_remain_unambiguous() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters.insert("Width".into(), "4mm".into());
    owner.dimension_properties.insert(
        "Width".into(),
        BTreeMap::from([("EquationId".into(), "Width".into())]),
    );
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }]);

    let aliases = parameter_aliases(
        &parameters,
        &HashMap::new(),
        &HashSet::new(),
        parameters[0].owner.as_ref(),
    );

    assert_eq!(aliases.get("Width"), Some(&Some(parameters[0].id.clone())));
}

#[test]
fn project_parameters_preserves_composite_txd_text_without_hiding_bad_equations() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters = BTreeMap::from([
        ("TXD1".into(), "4X <MOD-DIAM> 12 <HOLE-DEPTH> 40".into()),
        ("TXD2".into(), "<MOD-DIAM>4".into()),
        ("D1".into(), "1 +".into()),
    ]);
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }]);
    let by_name = parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter))
        .collect::<HashMap<_, _>>();

    assert_eq!(
        by_name["TXD1"].value,
        Some(ParameterValue::String(
            "4X <MOD-DIAM> 12 <HOLE-DEPTH> 40".into()
        ))
    );
    assert_eq!(
        by_name["TXD2"].value,
        Some(ParameterValue::Length(Length(4.0)))
    );
    assert_eq!(by_name["D1"].value, None);
    assert_eq!(
        parameters_with_unevaluable_expressions(&parameters, &HashMap::new(), &HashSet::new(), &[],),
        1
    );
}

#[test]
fn layered_parameter_aliases_match_materialized_precedence() {
    let global_owner = FeatureId("global".into());
    let local_owner = FeatureId("local".into());
    let parameters = [
        DesignParameter {
            id: ParameterId("global-id".into()),
            owner: Some(global_owner.clone()),
            ordinal: 0,
            name: "Width".into(),
            expression: "1".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        },
        DesignParameter {
            id: ParameterId("local-id".into()),
            owner: Some(local_owner.clone()),
            ordinal: 0,
            name: "Width".into(),
            expression: "2".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        },
    ];
    let aliases =
        ParameterAliases::new(&parameters, &HashMap::new(), &HashSet::from([global_owner]));

    for owner in [Some(local_owner), Some(FeatureId("unrelated".into())), None] {
        let materialized = aliases.materialize(owner.as_ref());
        let layered = aliases.for_owner(owner.as_ref());
        for alias in ["Width", "global-id", "local-id", "missing"] {
            assert_eq!(layered.get(alias), materialized.get(alias));
        }
    }
}

#[test]
fn subtraction_separates_unquoted_parameter_references() {
    assert_eq!(
        expression_identifiers("D1@Sketch1-D2@Sketch1").collect::<Vec<_>>(),
        ["D1@Sketch1", "D2@Sketch1"]
    );
}

#[test]
fn numeric_literals_do_not_bind_numeric_parameter_names() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters = BTreeMap::from([
        ("4".into(), "3mm".into()),
        ("Literal".into(), "4".into()),
        ("Reference".into(), "\"4\" * 2".into()),
    ]);
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }]);
    let by_name = parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter))
        .collect::<HashMap<_, _>>();

    assert!(by_name["Literal"].dependencies.is_empty());
    assert_eq!(by_name["Reference"].dependencies, [by_name["4"].id.clone()]);
    assert_eq!(
        by_name["Reference"].value,
        Some(ParameterValue::Length(Length(6.0)))
    );
    assert!(!unquoted_expression_identifier("4"));
    assert_eq!(
        rewrite_parameter_expression("Width * 2", &HashMap::from([("Width".into(), "4".into())]),)
            .as_deref(),
        Some("\"4\" * 2")
    );
}

#[test]
fn subtraction_projects_both_parameter_dependencies() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters = BTreeMap::from([
        ("A".into(), "7".into()),
        ("B".into(), "2".into()),
        ("C".into(), "A-B".into()),
    ]);
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }]);

    assert_eq!(
        parameters[2].dependencies,
        [parameters[0].id.clone(), parameters[1].id.clone()]
    );
    assert_eq!(parameters[2].value, Some(ParameterValue::Integer(5)));
}

#[test]
fn unqualified_aliases_are_local_to_the_expression_owner() {
    let mut first = feature("first", Some("1"), 0);
    first.parameters.insert("Width".into(), "4mm".into());
    let mut second = feature("second", Some("2"), 1);
    second.parameters.insert("Width".into(), "5mm".into());
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second],
    }]);

    let first_aliases = parameter_aliases(
        &parameters,
        &HashMap::new(),
        &HashSet::new(),
        parameters[0].owner.as_ref(),
    );
    let second_aliases = parameter_aliases(
        &parameters,
        &HashMap::new(),
        &HashSet::new(),
        parameters[1].owner.as_ref(),
    );
    let unrelated_aliases = parameter_aliases(
        &parameters,
        &HashMap::new(),
        &HashSet::new(),
        Some(&FeatureId("unrelated".into())),
    );

    assert_eq!(
        first_aliases.get("Width"),
        Some(&Some(parameters[0].id.clone()))
    );
    assert_eq!(
        second_aliases.get("Width"),
        Some(&Some(parameters[1].id.clone()))
    );
    assert_eq!(unrelated_aliases.get("Width"), None);
}

#[test]
fn equation_driven_parameters_are_global() {
    let mut equations = feature("equations", Some("1"), 0);
    equations.kind = "EquationDriven".into();
    equations.parameters.insert("Width".into(), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer
        .parameters
        .insert("Result".into(), "Width * 2".into());

    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![equations, consumer],
    }]);

    assert_eq!(parameters[1].dependencies, [parameters[0].id.clone()]);
    assert_eq!(
        parameters[1].value,
        Some(ParameterValue::Length(Length(8.0)))
    );
}

#[test]
fn ordinary_feature_parameters_do_not_leak_globally() {
    let mut source = feature("source", Some("1"), 0);
    source.parameters.insert("Width".into(), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer
        .parameters
        .insert("Result".into(), "Width * 2".into());

    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![source, consumer],
    }]);

    assert!(parameters[1].dependencies.is_empty());
    assert_eq!(parameters[1].value, None);
}

#[test]
fn local_parameter_precedes_same_named_global() {
    let mut equations = feature("equations", Some("1"), 0);
    equations.kind = "EquationDriven".into();
    equations.parameters.insert("Width".into(), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer.parameters = BTreeMap::from([
        ("Width".into(), "5mm".into()),
        ("Result".into(), "Width * 2".into()),
    ]);

    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![equations, consumer],
    }]);

    assert_eq!(parameters[1].dependencies, [parameters[2].id.clone()]);
    assert_eq!(
        parameters[1].value,
        Some(ParameterValue::Length(Length(10.0)))
    );
}

#[test]
fn ambiguous_and_missing_history_references_do_not_bind_arbitrarily() {
    let first = feature("first", Some("1"), 0);
    let second = feature("second", Some("1"), 1);
    let mut dependent = feature("dependent", Some("2"), 2);
    dependent.properties.insert("Dependency".into(), "1".into());
    let mut malformed = feature("malformed", Some("3"), 3);
    malformed.parent_source_id = Some("missing".into());
    malformed
        .content
        .push(FeatureContent::Feature("missing-child".into()));
    malformed
        .content
        .push(FeatureContent::Dimension("D1".into()));
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second, dependent, malformed],
    };

    let projected = project_features(std::slice::from_ref(&history));

    assert!(projected[2].dependencies.is_empty());
    assert_eq!(incomplete_history_reference_features(&[history]), 4);
}

#[test]
fn assigning_configuration_index_does_not_capture_global_input_lane() {
    let mut native = native_with_configuration_lanes(
        vec![native_configuration("native-configuration", 0, None)],
        vec![feature_input_lane("global-lane", None)],
    )
    .into();
    let mut configuration =
        design_configuration("configuration", 0, Some(0), Some("native-configuration"));
    configuration.active = true.into();
    sync_neutral_configurations(&[configuration], &mut native);

    let native = native.expect("required invariant");
    assert_eq!(
        native.feature_histories[0].configurations[0].source_index,
        Some(0)
    );
    assert_eq!(native.feature_input_lanes[0].configuration, None);
}

#[test]
fn stored_configuration_id_precedes_ordinal_fallback() {
    let configurations = [
        with_configuration_id(design_configuration("explicit", 0, Some(7), None), 1),
        design_configuration("fallback", 1, None, None),
    ];
    let lanes = [feature_input_lane("lane", Some("1"))];

    assert_eq!(
        configuration_lane_assignments(&configurations, &lanes),
        [(0, 0)]
    );
}

#[test]
fn configuration_lane_loss_uses_stored_ids_not_partition_indices() {
    let configurations = [
        with_configuration_id(design_configuration("first", 0, Some(8), None), 1),
        with_configuration_id(design_configuration("second", 1, Some(9), None), 2),
    ];

    assert_eq!(
        unresolved_configuration_lanes(
            &configurations,
            &[
                feature_input_lane("first", Some("1")),
                feature_input_lane("second", Some("2")),
            ],
        ),
        0
    );
    assert_eq!(
        unresolved_configuration_lanes(
            &configurations,
            &[
                feature_input_lane("duplicate-first", Some("1")),
                feature_input_lane("duplicate-second", Some("1")),
                feature_input_lane("unmatched", Some("3")),
            ],
        ),
        3
    );
}

#[test]
fn changing_shadowed_ordinal_does_not_steal_stored_id_lane() {
    let native_configurations = vec![
        native_with_configuration_id(native_configuration("explicit-native", 0, Some(7)), 1),
        native_configuration("fallback-native", 1, None),
    ];
    let mut native = native_with_configuration_lanes(
        native_configurations,
        vec![feature_input_lane("explicit-lane", Some("1"))],
    )
    .into();
    let configurations = [
        with_configuration_id(
            design_configuration("explicit", 0, Some(8), Some("explicit-native")),
            1,
        ),
        design_configuration("fallback", 2, None, Some("fallback-native")),
    ];

    sync_neutral_configurations(&configurations, &mut native);

    assert_eq!(
        native.expect("required invariant").feature_input_lanes[0]
            .configuration
            .as_deref(),
        Some("1")
    );
}

#[test]
fn configuration_lane_index_swaps_are_simultaneous() {
    let mut native = native_with_configuration_lanes(
        vec![
            native_with_configuration_id(native_configuration("first-native", 0, None), 1),
            native_with_configuration_id(native_configuration("second-native", 1, None), 2),
        ],
        vec![
            feature_input_lane("first-lane", Some("1")),
            feature_input_lane("second-lane", Some("2")),
        ],
    )
    .into();
    let configurations = [
        with_configuration_id(
            design_configuration("first", 0, None, Some("first-native")),
            2,
        ),
        with_configuration_id(
            design_configuration("second", 1, None, Some("second-native")),
            1,
        ),
    ];

    sync_neutral_configurations(&configurations, &mut native);

    assert_eq!(
        native
            .expect("required invariant")
            .feature_input_lanes
            .into_iter()
            .map(|lane| lane.configuration)
            .collect::<Vec<_>>(),
        [Some("2".into()), Some("1".into())]
    );
}

#[test]
fn deleting_configuration_removes_its_uniquely_owned_lane() {
    let mut native = native_with_configuration_lanes(
        vec![
            native_with_configuration_id(native_configuration("kept-native", 0, Some(9)), 1),
            native_with_configuration_id(native_configuration("deleted-native", 1, Some(10)), 2),
        ],
        vec![
            feature_input_lane("kept-lane", Some("1")),
            feature_input_lane("deleted-lane", Some("2")),
        ],
    )
    .into();

    sync_neutral_configurations(
        &[with_configuration_id(
            design_configuration("kept", 0, Some(11), Some("kept-native")),
            1,
        )],
        &mut native,
    );

    let native = native.expect("required invariant");
    assert_eq!(native.feature_input_lanes.len(), 1);
    assert_eq!(native.feature_input_lanes[0].id, "kept-lane");

    let mut native = native_with_configuration_lanes(
        vec![native_with_configuration_id(
            native_configuration("deleted-native", 0, Some(1)),
            1,
        )],
        vec![
            feature_input_lane("global-lane", None),
            feature_input_lane("deleted-lane", Some("1")),
        ],
    )
    .into();
    sync_neutral_configurations(&[], &mut native);
    let native = native.expect("required invariant");
    assert!(native.feature_histories[0].configurations.is_empty());
    assert_eq!(native.feature_input_lanes.len(), 1);
    assert_eq!(native.feature_input_lanes[0].id, "global-lane");
}

#[test]
fn configuration_lane_follows_stored_id_or_ordinal_changes() {
    for (previous_ordinal, previous_id, previous_lane, ordinal, id, expected) in [
        (2, Some(7), "7", 3, None, "3"),
        (2, None, "2", 4, None, "4"),
    ] {
        let native_configuration =
            native_configuration("native-configuration", previous_ordinal, Some(19));
        let native_configuration = previous_id.map_or(native_configuration.clone(), |id| {
            native_with_configuration_id(native_configuration, id)
        });
        let mut native = native_with_configuration_lanes(
            vec![native_configuration],
            vec![feature_input_lane("lane", Some(previous_lane))],
        )
        .into();
        let mut configuration = design_configuration(
            "configuration",
            ordinal,
            Some(23),
            Some("native-configuration"),
        );
        if let Some(id) = id {
            configuration = with_configuration_id(configuration, id);
        }
        configuration.active = true.into();
        sync_neutral_configurations(&[configuration], &mut native);

        assert_eq!(
            native.expect("required invariant").feature_input_lanes[0]
                .configuration
                .as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn configuration_sketch_state_reuses_projected_neutral_sketch() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, DesignConfiguration, Feature as NeutralFeature,
        FeatureDefinition,
    };
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
        SpatialSketch, SpatialSketchId,
    };

    let native_feature = feature("sketch-native", Some("7"), 0);
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native_feature],
    };
    let feature_id = cadmpeg_ir::features::FeatureId("sketch".into());
    let unresolved = FeatureDefinition::Sketch {
        space: SketchSpace::Planar,
        sketch: None,
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("sketch-native".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: unresolved.clone(),
        native_ref: Some("sketch-native".into()),
    });
    let spatial_feature_id = cadmpeg_ir::features::FeatureId("sldprt:model:feature#spatial".into());
    let spatial_sketch_id = SpatialSketchId("sldprt:model:spatial-sketch#spatial".into());
    ir.model.features.push(NeutralFeature {
        id: spatial_feature_id.clone(),
        ordinal: 1,
        name: Some("spatial-native".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(spatial_sketch_id.clone()),
        },
        native_ref: Some("spatial-native".into()),
    });
    let sketch_id = SketchId("projected-sketch".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("sketch-native".into()),
        configuration: Some("0".into()),
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    });
    ir.model.sketch_entities.push(SketchEntity {
        id: SketchEntityId("configuration-line".into()),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: Some("line-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        },
    });
    ir.model.spatial_sketches.push(SpatialSketch {
        id: spatial_sketch_id.clone(),
        name: Some("spatial-native".into()),
        configuration: Some("0".into()),
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    });
    ir.model.configurations.push(DesignConfiguration {
        id: cadmpeg_ir::features::ConfigurationId("configuration".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(0),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (
                feature_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: unresolved,
                },
            ),
            (
                spatial_feature_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::SpatialSketch { sketch: None },
                },
            ),
        ]),
        native_ref: None,
    });
    let mut lane = feature_input_lane("lane", Some("0"));
    lane.sketch_entities = vec![
        crate::records::SketchInputEntity {
            id: "line-marker".into(),
            parent: lane.id.clone(),
            feature_ref: Some("sketch-native".into()),
            ordinal: 0,
            offset: 10,
            object_index: Some(1),
            local_id: Some(1),
            kind: crate::records::SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: None,
            links: Vec::new(),
            link_selector: None,
        },
        crate::records::SketchInputEntity {
            id: "relation-marker".into(),
            parent: lane.id.clone(),
            feature_ref: Some("sketch-native".into()),
            ordinal: 1,
            offset: 20,
            object_index: Some(2),
            local_id: Some(2),
            kind: crate::records::SketchInputKind::Relation(
                crate::records::SketchRelationKind::Horizontal,
            ),
            state_value: None,
            coordinates_m: None,
            links: vec![crate::records::SketchInputLink {
                local_id: 1,
                entity_ref: "line-marker".into(),
            }],
            link_selector: None,
        },
    ];

    let mut annotations = cadmpeg_ir::Annotations::default();
    project_configuration_sketch_states(&mut ir, &[history], &[lane], &mut annotations);

    assert_eq!(ir.model.sketches.len(), 1);
    assert!(matches!(
        &ir.model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch),
            ..
        } if sketch == &sketch_id
    ));
    assert!(matches!(
        &ir.model.configurations[0].feature_states[&spatial_feature_id].definition,
        FeatureDefinition::SpatialSketch {
            sketch: Some(sketch),
        } if sketch == &spatial_sketch_id
    ));
    assert!(ir.model.sketch_constraints.iter().any(|constraint| {
        constraint.native_ref.as_deref() == Some("relation-marker")
            && matches!(
                constraint.definition,
                SketchConstraintDefinition::Horizontal { ref entity }
                    if entity.0 == "configuration-line"
            )
    }));
}

#[test]
fn dissected_sketch_alias_inherits_an_omitted_class_without_solved_geometry() {
    use cadmpeg_ir::features::{Feature as NeutralFeature, FeatureDefinition};

    let mut owner = feature("owner-native", Some("63"), 0);
    owner.xml_tag = "Sketch".into();
    owner.name = "Sketch1".into();
    owner.kind = "Sketch".into();
    owner.input_class = Some("moProfileFeature_c".into());
    owner.parameters.insert("D1".into(), "10".into());
    owner.content.push(FeatureContent::Dimension("D1".into()));
    let mut alias = feature("alias-native", Some("85"), 1);
    alias.xml_tag = "Sketch".into();
    alias.name = "Sketch1<3>".into();
    alias.kind = alias.name.clone();
    alias
        .properties
        .insert("Description".into(), alias.name.clone());
    alias.parameters = owner.parameters.clone();
    alias.content = owner.content.clone();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner, alias],
    };
    let neutral = |id: &str, name: &str, native_ref: &str, ordinal| NeutralFeature {
        id: cadmpeg_ir::features::FeatureId(id.into()),
        ordinal,
        name: Some(name.into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Sketch".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: None,
        },
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        neutral("owner", "Sketch1", "owner-native", 0),
        neutral("alias", "Sketch1<3>", "alias-native", 1),
    ];
    bind_unique_sketch_feature(&mut features, &[], std::slice::from_ref(&history));
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert_eq!(features[1].dependencies, [features[0].id.clone()]);

    crate::resolved_features::component_paths::project_dissected_sketches(
        &mut features,
        &[],
        &[history],
    );
    assert!(matches!(
        features[1].definition,
        FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::DissectedProfile,
            ..
        }
    ));
}

#[test]
fn configuration_sketch_states_reuse_shared_geometry_across_lanes() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, DesignConfiguration, Feature as NeutralFeature,
        FeatureDefinition, FeatureId,
    };
    use cadmpeg_ir::sketches::{SpatialSketch, SpatialSketchId};

    let feature_id = FeatureId("sldprt:model:feature#spatial".into());
    let sketch_id = SpatialSketchId("sldprt:model:spatial-sketch#spatial".into());
    let planar_state_id = FeatureId("sldprt:model:feature#planar-state".into());
    let planar_sketch_id = SpatialSketchId("sldprt:model:spatial-sketch#planar-state".into());
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(NeutralFeature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("spatial".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("spatial-native".into()),
    });
    ir.model.features.push(NeutralFeature {
        id: planar_state_id.clone(),
        ordinal: 1,
        name: Some("planar-state".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(planar_sketch_id.clone()),
        },
        native_ref: Some("planar-state-native".into()),
    });
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch_id.clone(),
        name: Some("spatial".into()),
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("first-lane".into()),
    });
    ir.model.spatial_sketches.push(SpatialSketch {
        id: planar_sketch_id.clone(),
        name: Some("planar-state".into()),
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("first-lane".into()),
    });
    for ordinal in 0..2 {
        ir.model.configurations.push(DesignConfiguration {
            id: cadmpeg_ir::features::ConfigurationId(format!("configuration-{ordinal}")),
            ordinal,
            active: (ordinal == 0).into(),
            source_index: Some(ordinal),
            name: format!("Configuration {ordinal}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::from([
                (
                    feature_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: Vec::new(),
                        outputs: Vec::new(),
                        definition: FeatureDefinition::SpatialSketch { sketch: None },
                    },
                ),
                (
                    planar_state_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: Vec::new(),
                        outputs: Vec::new(),
                        definition: FeatureDefinition::Sketch {
                            space: SketchSpace::Planar,
                            sketch: None,
                        },
                    },
                ),
            ]),
            native_ref: None,
        });
    }
    let lanes = [
        feature_input_lane("first-lane", Some("0")),
        feature_input_lane("second-lane", Some("1")),
    ];

    let mut annotations = cadmpeg_ir::Annotations::default();
    project_configuration_sketch_states(&mut ir, &[], &lanes, &mut annotations);

    assert!(ir.model.configurations.iter().all(|configuration| matches!(
        &configuration.feature_states[&feature_id].definition,
        FeatureDefinition::SpatialSketch {
            sketch: Some(projected),
        } if projected == &sketch_id
    )));
    assert!(ir.model.configurations.iter().all(|configuration| matches!(
        &configuration.feature_states[&planar_state_id].definition,
        FeatureDefinition::SpatialSketch {
            sketch: Some(projected),
        } if projected == &planar_sketch_id
    )));
}

#[test]
fn supplemental_edge_paths_project_into_matching_configuration_state() {
    use cadmpeg_ir::features::{
        ChamferGroup, ChamferSpec, ConfigurationFeatureState, DesignConfiguration, EdgeSelection,
        Feature as NeutralFeature, FeatureDefinition, FeatureId, Length,
    };

    let producer_id = FeatureId("producer".into());
    let consumer_id = FeatureId("consumer".into());
    let unresolved = FeatureDefinition::Chamfer {
        groups: vec![ChamferGroup {
            edges: EdgeSelection::Unresolved,
            spec: ChamferSpec::Distance {
                distance: Length(1.0),
            },
        }],
        flip_direction: false,
    };
    let neutral_feature = |id: FeatureId, ordinal, native_ref: &str, definition| NeutralFeature {
        id,
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
        native_ref: Some(native_ref.into()),
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features = vec![
        neutral_feature(
            producer_id.clone(),
            0,
            "producer-native",
            FeatureDefinition::StoredGeometry,
        ),
        neutral_feature(
            consumer_id.clone(),
            1,
            "consumer-native",
            unresolved.clone(),
        ),
    ];
    ir.model.configurations.push(DesignConfiguration {
        id: cadmpeg_ir::features::ConfigurationId("configuration".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(1),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::from([("id".into(), "1".into())]),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (
                producer_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::StoredGeometry,
                },
            ),
            (
                consumer_id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition: unresolved,
                },
            ),
        ]),
        native_ref: None,
    });
    let mut lane = feature_input_lane("sldprt:feature-input:config-objects#1", Some("1"));
    lane.edge_selections
        .push(crate::records::FeatureInputEdgeSelection {
            id: "selection".into(),
            parent: lane.id.clone(),
            ordinal: 0,
            offset: 100,
            object_name_ref: "name".into(),
            feature_ref: "consumer-native".into(),
            local_edge_ids: vec![7],
            components: Vec::new(),
            references: Vec::new(),
            producer_feature_refs: vec!["producer-native".into()],
            terminal_feature_ref: Some("producer-native".into()),
        });

    project_configuration_supplemental_edge_selections(&mut ir, &[lane]);

    let state = &ir.model.configurations[0].feature_states[&consumer_id];
    assert_eq!(state.dependencies, vec![producer_id.clone()]);
    assert!(matches!(
        &state.definition,
        FeatureDefinition::Chamfer { groups, .. }
            if matches!(
                &groups[0].edges,
                EdgeSelection::Generated { edges, .. }
                    if edges.len() == 1
                        && edges[0].feature == producer_id
                        && edges[0].local_id == "7"
            )
    ));
}

#[test]
fn configuration_hole_inherits_shared_construction_and_placement() {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureId, HoleKind, HolePlacement, Length, Termination,
    };

    let id = FeatureId("test:model:feature#hole".into());
    let base = cadmpeg_ir::features::Feature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            position: None,
            direction: None,
            placements: vec![HolePlacement::Axis {
                origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            }],
            kind: HoleKind::Counterbore {
                diameter: Length(8.0),
                depth: Length(4.0),
            },
            exit_kind: None,
            diameter: Some(Length(5.0)),
            extent: Some(Termination::Blind {
                length: Length(12.0),
            }),
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    };
    let mut configured = base.clone();
    configured.definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        position: None,
        direction: None,
        placements: Vec::new(),
        kind: HoleKind::Simple,
        exit_kind: None,
        diameter: None,
        extent: None,
        bottom: None,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    };

    inherit_configuration_shared_semantics(&mut configured.definition, &base.definition);

    assert_eq!(configured.definition, base.definition);
}

#[test]
fn configuration_lane_does_not_inherit_shared_hole_semantics() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, Feature as NeutralFeature, FeatureDefinition, FeatureId,
        HoleKind, Length, Termination,
    };

    let id = FeatureId("test:model:feature#hole-lane".into());
    let base_definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        position: None,
        direction: None,
        placements: vec![cadmpeg_ir::features::HolePlacement::Axis {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }],
        kind: HoleKind::Counterbore {
            diameter: Length(8.0),
            depth: Length(4.0),
        },
        exit_kind: None,
        diameter: Some(Length(5.0)),
        extent: Some(Termination::Blind {
            length: Length(12.0),
        }),
        bottom: None,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    };
    let local_definition = FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        position: None,
        direction: None,
        placements: Vec::new(),
        kind: HoleKind::Simple,
        exit_kind: None,
        diameter: None,
        extent: None,
        bottom: None,
        taper_angle: None,
        specification: None,
        allow_multi_profile_faces: None,
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(NeutralFeature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: base_definition,
        native_ref: None,
    });
    let mut configuration = design_configuration("configuration", 0, Some(0), None);
    configuration.active = true.into();
    configuration.feature_states.insert(
        id.clone(),
        ConfigurationFeatureState {
            suppressed: false,
            dependencies: Vec::new(),
            outputs: Vec::new(),
            definition: local_definition,
        },
    );
    ir.model.configurations.push(configuration);

    let mut annotations = cadmpeg_ir::Annotations::default();
    project_configuration_sketch_states(
        &mut ir,
        &[],
        &[feature_input_lane("lane", Some("0"))],
        &mut annotations,
    );

    assert!(matches!(
        &ir.model.configurations[0].feature_states[&id].definition,
        FeatureDefinition::Hole {
            placements,
            kind: HoleKind::Simple,
            diameter: None,
            extent: None,
            ..
        } if placements.is_empty()
    ));
}

#[test]
fn configuration_offset_plane_inherits_shared_reference() {
    use cadmpeg_ir::features::{DatumPlaneReference, FaceSelection, FeatureDefinition, Length};

    let base = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face {
            face: FaceSelection::Faces(vec!["test:model:face#1".into()]),
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        }),
        distance: Length(5.0),
    };
    let mut configured = FeatureDefinition::DatumOffsetPlane {
        reference: None,
        distance: Length(8.0),
    };

    inherit_configuration_shared_semantics(&mut configured, &base);

    let FeatureDefinition::DatumOffsetPlane {
        reference,
        distance,
    } = configured
    else {
        panic!("offset-plane definition retained its variant");
    };
    assert!(reference.is_some());
    assert_eq!(distance, Length(8.0));
}

#[test]
fn configuration_offset_plane_replaces_only_an_unresolved_face() {
    use cadmpeg_ir::features::{DatumPlaneReference, FaceSelection, FeatureDefinition, Length};

    let base = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face {
            face: FaceSelection::Faces(vec!["test:model:face#1".into()]),
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        }),
        distance: Length(5.0),
    };
    let configured_origin = cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0);
    let mut configured = FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face {
            face: FaceSelection::Unresolved,
            origin: configured_origin,
            normal: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }),
        distance: Length(8.0),
    };

    inherit_configuration_shared_semantics(&mut configured, &base);

    let FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face { face, origin, .. }),
        distance,
    } = configured
    else {
        panic!("offset-plane definition retained its face reference");
    };
    assert_eq!(face, FaceSelection::Faces(vec!["test:model:face#1".into()]));
    assert_eq!(origin, configured_origin);
    assert_eq!(distance, Length(8.0));
}

#[test]
fn configuration_numeric_override_inherits_parameter_dimension() {
    use cadmpeg_ir::features::{
        ConfigurationId, DesignConfiguration, DesignParameter, FeatureId, ParameterId,
        ParameterValue,
    };

    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let parameter_id = ParameterId("test:model:parameter#depth".into());
    let count_id = ParameterId("test:model:parameter#count".into());
    ir.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: Some(FeatureId("test:model:feature#extrude".into())),
        ordinal: 0,
        name: "Depth".into(),
        expression: "7mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(7.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: count_id.clone(),
        owner: Some(FeatureId("test:model:feature#pattern".into())),
        ordinal: 0,
        name: "Count".into(),
        expression: "7".into(),
        display: None,
        value: Some(ParameterValue::Integer(7)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("test:model:configuration#default".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(0),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::from([
            (parameter_id.clone(), ParameterValue::Integer(7)),
            (count_id.clone(), ParameterValue::Length(Length(0.007))),
        ]),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });

    align_configuration_parameter_kinds(&mut ir);

    assert_eq!(
        ir.model.configurations[0].parameter_values[&parameter_id],
        ParameterValue::Length(Length(7.0))
    );
    assert!(!ir.model.configurations[0]
        .parameter_values
        .contains_key(&count_id));

    ir.model.configurations[0]
        .parameter_values
        .insert(count_id.clone(), ParameterValue::Length(Length(7.0)));
    align_configuration_parameter_kinds(&mut ir);
    assert_eq!(
        ir.model.configurations[0].parameter_values[&count_id],
        ParameterValue::Integer(7)
    );

    ir.model.configurations[0]
        .parameter_values
        .insert(count_id.clone(), ParameterValue::Real(7.0));
    align_configuration_parameter_kinds(&mut ir);
    assert_eq!(
        ir.model.configurations[0].parameter_values[&count_id],
        ParameterValue::Integer(7)
    );

    ir.model.configurations[0]
        .parameter_values
        .insert(count_id.clone(), ParameterValue::Real(7.5));
    align_configuration_parameter_kinds(&mut ir);
    assert!(!ir.model.configurations[0]
        .parameter_values
        .contains_key(&count_id));
}
