// SPDX-License-Identifier: Apache-2.0
//! Offset and coincident reference-plane frame projection tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use super::*;

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
fn unresolved_face_frame_resolves_one_preceding_parallel_plane() {
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
        .insert("Reference".into(), "missing".into());
    offset
        .properties
        .insert("Origin".into(), "6mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "1,0,0".into());
    offset.properties.insert("UAxis".into(), "0,0,1".into());
    offset
        .properties
        .insert("ReferenceFaceOrigin".into(), "0mm,0mm,0mm".into());
    offset
        .properties
        .insert("ReferenceFaceNormal".into(), "1,0,0".into());
    offset
        .properties
        .insert("ReferenceFaceUAxis".into(), "0,0,-1".into());
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
fn unresolved_face_frame_resolves_a_later_principal_plane_from_support_geometry() {
    let mut offset = cadmpeg_ir::features::Feature::new(
        "offset".into(),
        0,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, -1.0),
            }),
            distance: Length(6.0),
        },
    );
    offset.native_ref = Some("sldprt:history:feature#0:offset".into());
    offset
        .source_properties
        .insert("Origin".into(), "6mm,0mm,0mm".into());
    offset
        .source_properties
        .insert("Normal".into(), "1,0,0".into());
    offset
        .source_properties
        .insert("UAxis".into(), "0,0,-1".into());
    offset
        .source_properties
        .insert("ReferenceFaceOrigin".into(), "0mm,0mm,0mm".into());
    offset
        .source_properties
        .insert("ReferenceFaceNormal".into(), "1,0,0".into());
    offset
        .source_properties
        .insert("ReferenceFaceUAxis".into(), "0,0,-1".into());
    let mut principal = cadmpeg_ir::features::Feature::new(
        "right".into(),
        1,
        FeatureDefinition::DatumPrincipalPlane {
            plane: cadmpeg_ir::features::PrincipalPlane::Right,
        },
    );
    principal.native_ref = Some("sldprt:history:feature#0:right".into());

    let mut features = vec![offset, principal];
    bind_offset_plane_references(&mut features);

    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            distance: Length(6.0),
        } if reference == &features[1].id
    ));
    assert_eq!(features[0].dependencies, [features[1].id.clone()]);
}

#[test]
fn unresolved_face_frame_collapses_a_zero_offset_plane_alias() {
    let mut base = cadmpeg_ir::features::Feature::new(
        "base".into(),
        0,
        FeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
    );
    base.native_ref = Some("sldprt:history:feature#0:base".into());

    let mut alias = cadmpeg_ir::features::Feature::new(
        "alias".into(),
        1,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(base.id.clone())),
            distance: Length(0.0),
        },
    );
    alias.native_ref = Some("sldprt:history:feature#0:alias".into());
    alias
        .source_properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    alias
        .source_properties
        .insert("Normal".into(), "1,0,0".into());
    alias
        .source_properties
        .insert("UAxis".into(), "0,0,-1".into());

    let mut offset = cadmpeg_ir::features::Feature::new(
        "offset".into(),
        2,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, -1.0),
            }),
            distance: Length(6.0),
        },
    );
    offset.native_ref = Some("sldprt:history:feature#0:offset".into());
    offset
        .source_properties
        .insert("Origin".into(), "6mm,0mm,0mm".into());
    offset
        .source_properties
        .insert("Normal".into(), "1,0,0".into());
    offset
        .source_properties
        .insert("UAxis".into(), "0,0,-1".into());
    offset
        .source_properties
        .insert("ReferenceFaceOrigin".into(), "0mm,0mm,0mm".into());
    offset
        .source_properties
        .insert("ReferenceFaceNormal".into(), "1,0,0".into());
    offset
        .source_properties
        .insert("ReferenceFaceUAxis".into(), "0,0,-1".into());

    let mut features = vec![base, alias, offset];
    bind_offset_plane_references(&mut features);

    assert!(matches!(
        &features[2].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            distance: Length(6.0),
        } if reference == &features[0].id
    ));
    assert_eq!(features[2].dependencies, [features[0].id.clone()]);
}

#[test]
fn explicit_later_constructed_plane_survives_without_result_offset_frame() {
    let mut offset = cadmpeg_ir::features::Feature::new(
        "offset".into(),
        0,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature("reference".into())),
            distance: Length(6.0),
        },
    );
    offset.native_ref = Some("sldprt:history:feature#0:offset".into());
    offset
        .source_properties
        .insert("Reference".into(), "reference".into());
    offset
        .source_properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    offset
        .source_properties
        .insert("Normal".into(), "1,0,0".into());
    offset
        .source_properties
        .insert("UAxis".into(), "0,0,-1".into());
    let mut reference = cadmpeg_ir::features::Feature::new(
        "reference".into(),
        1,
        FeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
    );
    reference.native_ref = Some("sldprt:history:feature#0:reference".into());

    let mut features = vec![offset, reference];
    bind_offset_plane_references(&mut features);

    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            distance: Length(6.0),
        } if reference == &features[1].id
    ));
    assert_eq!(features[0].dependencies, [features[1].id.clone()]);
}

#[test]
fn unresolved_face_frame_does_not_resolve_ambiguous_parallel_planes() {
    let mut reference = feature("sldprt:history:feature#0:0", None, 0);
    reference.input_class = Some("moRefPlane_c".into());
    reference
        .properties
        .insert("Origin".into(), "0mm,0mm,0mm".into());
    reference.properties.insert("Normal".into(), "1,0,0".into());
    reference.properties.insert("UAxis".into(), "0,0,-1".into());
    let mut duplicate = reference.clone();
    duplicate.id = "sldprt:history:feature#0:1".into();
    duplicate.ordinal = 1;
    let mut offset = feature("sldprt:history:feature#0:2", None, 2);
    offset.input_class = Some("moRefPlane_c".into());
    offset.parameters.insert("D1".into(), "6mm".into());
    offset
        .properties
        .insert("Reference".into(), "missing".into());
    offset
        .properties
        .insert("Origin".into(), "6mm,0mm,0mm".into());
    offset.properties.insert("Normal".into(), "1,0,0".into());
    offset.properties.insert("UAxis".into(), "0,0,1".into());
    offset
        .properties
        .insert("ReferenceFaceOrigin".into(), "0mm,0mm,0mm".into());
    offset
        .properties
        .insert("ReferenceFaceNormal".into(), "1,0,0".into());
    offset
        .properties
        .insert("ReferenceFaceUAxis".into(), "0,0,-1".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![reference, duplicate, offset],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        &projected[2].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane { .. }),
            distance: Length(6.0),
        }
    ));
    assert!(projected[2].dependencies.is_empty());
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
    let face_ids = HashMap::new();
    let surface_selection_faces = SurfaceSelectionFaceBindings::new();
    let face_selection_context = FaceSelectionContext {
        ids: &face_ids,
        feature_ref: None,
        surface_selection_faces: &surface_selection_faces,
    };

    resolve_offset_plane_face_selection(
        &mut selection,
        origin,
        Vector3::new(0.0, 0.0, 1.0),
        &face_selection_context,
        std::slice::from_ref(&face),
        &surfaces,
    );

    assert_eq!(origin, Point3::new(0.0, 0.0, 5.0));
    assert_eq!(selection, FaceSelection::Native("component-path".into()));
}

#[test]
fn native_face_offset_reference_uses_identity_without_a_duplicate_frame() {
    for (native, source_origin, distance_text, expected_distance) in [
        ("native-face", "0mm,0mm,0mm", "6mm", Length(6.0)),
        (
            "sldprt:feature-input:surface-component-ids:630506365",
            "0mm,0mm,210mm",
            "40mm",
            Length(40.0),
        ),
    ] {
        let mut offset = feature("sldprt:history:feature#0:0", None, 0);
        offset.input_class = Some("moRefPlane_c".into());
        offset.parameters.insert("D1".into(), distance_text.into());
        for (name, value) in [
            ("Origin", source_origin),
            ("Normal", "0,0,1"),
            ("UAxis", "1,0,0"),
            ("ReferenceFaceNative", native),
        ] {
            offset.properties.insert(name.into(), value.into());
        }

        let definition = project_offset_plane(&offset, &HashMap::new()).unwrap();
        let FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face(face)),
            distance,
        } = definition
        else {
            panic!("native face offset did not project as a face reference");
        };
        assert_eq!(face, FaceSelection::Native(native.into()));
        assert_eq!(distance, expected_distance);
    }
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
            reference: Some(DatumPlaneReference::ResolvedPlane { .. }),
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
