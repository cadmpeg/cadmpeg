// SPDX-License-Identifier: Apache-2.0
//! Offset and coincident reference-plane frame projection tests.
#![allow(clippy::unwrap_used)]

use crate::history::bind::order_features_for_regeneration;
use crate::history::project::bind_offset_plane_references;
use crate::history::project::datum::project_offset_plane;
use crate::history::project::project_features;
use crate::history::selections::resolve_offset_plane_face_selection;
use crate::history::selections::resolve_planar_face_selection;
use crate::history::selections::FaceSelectionContext;
use crate::history::selections::SurfaceSelectionFaceBindings;
use crate::history::tests::feature;
use crate::records::FeatureHistory;
use cadmpeg_ir::features::datums::DatumPlaneReference;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::topology::Face;
use std::collections::BTreeMap;
use std::collections::HashMap;

fn with_test_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

#[test]
fn offset_plane_frame_resolves_one_preceding_parallel_plane() {
    let mut reference = feature("sldprt:history:feature#0:0", None, 0);
    reference.input_class = Some("moRefPlane_c".into());
    reference.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
    let mut offset = feature("sldprt:history:feature#0:1", None, 1);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "6mm".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "6mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,1".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![reference, offset],
    };

    let projected = project_features(&[history]).unwrap();
    assert!(matches!(
        projected[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: bound }),
            distance: actual_distance,
        }) if (bound == &projected[0].id) && actual_distance.get() == 6.0
    ));
    assert_eq!(
        projected[1].dependencies.as_slice(),
        [projected[0].id.clone()]
    );
}

#[test]
fn unresolved_face_frame_resolves_one_preceding_parallel_plane() {
    let mut reference = feature("sldprt:history:feature#0:0", None, 0);
    reference.input_class = Some("moRefPlane_c".into());
    reference.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
    let mut offset = feature("sldprt:history:feature#0:1", None, 1);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "6mm".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Reference"),
        "missing".into(),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "6mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,1".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceOrigin"),
        "0mm,0mm,0mm".into(),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceNormal"),
        "1,0,0".into(),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceUAxis"),
        "0,0,-1".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![reference, offset],
    };

    let projected = project_features(&[history]).unwrap();
    assert!(matches!(
        projected[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: bound }),
            distance: actual_distance,
        }) if (bound == &projected[0].id) && actual_distance.get() == 6.0
    ));
    assert_eq!(
        projected[1].dependencies.as_slice(),
        [projected[0].id.clone()]
    );
}

#[test]
fn unresolved_face_frame_resolves_a_later_principal_plane_from_support_geometry() {
    let mut offset = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#offset").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::ResolvedPlane {
                    frame: cadmpeg_ir::features::geometry::FeatureSupportPlaneFrame::new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, -1.0),
                    )
                    .unwrap(),
                }),
                distance: Length::new(6.0).unwrap(),
            }),
        ),
        native_ref: None,
    };
    offset.native_ref = Some("sldprt:history:feature#0:offset".into());
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "6mm,0mm,0mm".into(),
    );
    offset
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceOrigin"),
        "0mm,0mm,0mm".into(),
    );
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceNormal"),
        "1,0,0".into(),
    );
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceUAxis"),
        "0,0,-1".into(),
    );
    let mut principal = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#right").expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                plane: cadmpeg_ir::features::PrincipalPlane::Right,
            }),
        ),
        native_ref: None,
    };
    principal.native_ref = Some("sldprt:history:feature#0:right".into());

    let mut features = vec![offset, principal];
    bind_offset_plane_references(&mut features);

    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            distance: actual_distance,
        }) if (reference == &features[1].id) && actual_distance.get() == 6.0
    ));
    assert_eq!(
        features[0].dependencies.as_slice(),
        [features[1].id.clone()]
    );
}

#[test]
fn unresolved_face_frame_collapses_a_zero_offset_plane_alias() {
    let mut base = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#base").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPlane {
                frame: cadmpeg_ir::features::geometry::FeatureDatumPlaneFrame::new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, -1.0),
                )
                .unwrap(),
            }),
        ),
        native_ref: None,
    };
    base.native_ref = Some("sldprt:history:feature#0:base".into());

    let mut alias = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#alias").expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature {
                    feature: base.id.clone(),
                }),
                distance: Length::new(0.0).unwrap(),
            }),
        ),
        native_ref: None,
    };
    alias.native_ref = Some("sldprt:history:feature#0:alias".into());
    alias.source_properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    alias
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    alias
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());

    let mut offset = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#offset").expect("identity grammar"),
        ordinal: 2,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::ResolvedPlane {
                    frame: cadmpeg_ir::features::geometry::FeatureSupportPlaneFrame::new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, -1.0),
                    )
                    .unwrap(),
                }),
                distance: Length::new(6.0).unwrap(),
            }),
        ),
        native_ref: None,
    };
    offset.native_ref = Some("sldprt:history:feature#0:offset".into());
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "6mm,0mm,0mm".into(),
    );
    offset
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceOrigin"),
        "0mm,0mm,0mm".into(),
    );
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceNormal"),
        "1,0,0".into(),
    );
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceUAxis"),
        "0,0,-1".into(),
    );

    let mut features = vec![base, alias, offset];
    bind_offset_plane_references(&mut features);

    assert!(matches!(
        features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            distance: actual_distance,
        }) if (reference == &features[0].id) && actual_distance.get() == 6.0
    ));
    assert_eq!(
        features[2].dependencies.as_slice(),
        [features[0].id.clone()]
    );
}

#[test]
fn explicit_later_constructed_plane_survives_without_result_offset_frame() {
    let mut offset = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#offset").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature {
                    feature: FeatureId::mint("synthetic:test:id#reference")
                        .expect("identity grammar"),
                }),
                distance: Length::new(6.0).unwrap(),
            }),
        ),
        native_ref: None,
    };
    offset.native_ref = Some("sldprt:history:feature#0:offset".into());
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("Reference"),
        "reference".into(),
    );
    offset.source_properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    offset
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
    let mut reference = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#reference").expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPlane {
                frame: cadmpeg_ir::features::geometry::FeatureDatumPlaneFrame::new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, -1.0),
                )
                .unwrap(),
            }),
        ),
        native_ref: None,
    };
    reference.native_ref = Some("sldprt:history:feature#0:reference".into());

    let mut features = vec![offset, reference];
    bind_offset_plane_references(&mut features);

    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            distance: actual_distance,
        }) if (reference == &features[1].id) && actual_distance.get() == 6.0
    ));
    assert_eq!(
        features[0].dependencies.as_slice(),
        [features[1].id.clone()]
    );
}

#[test]
fn unresolved_face_frame_does_not_resolve_ambiguous_parallel_planes() {
    let mut reference = feature("sldprt:history:feature#0:0", None, 0);
    reference.input_class = Some("moRefPlane_c".into());
    reference.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
    let mut duplicate = reference.clone();
    duplicate.id = "sldprt:history:feature#0:1".into();
    duplicate.ordinal = 1;
    let mut offset = feature("sldprt:history:feature#0:2", None, 2);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "6mm".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Reference"),
        "missing".into(),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "6mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,1".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceOrigin"),
        "0mm,0mm,0mm".into(),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceNormal"),
        "1,0,0".into(),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceUAxis"),
        "0,0,-1".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![reference, duplicate, offset],
    };

    let projected = project_features(&[history]).unwrap();
    assert!(matches!(
        projected[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane { .. }),
            distance: actual_distance,
        }) if actual_distance.get() == 6.0
    ));
    assert!(projected[2].dependencies.is_empty());
}

#[test]
fn coincident_plane_frame_does_not_infer_an_offset_reference() {
    let mut reference = feature("sldprt:history:feature#0:0", None, 0);
    reference.input_class = Some("moRefPlane_c".into());
    reference.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
    let mut offset = feature("sldprt:history:feature#0:1", None, 1);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "0mm".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![reference, offset],
    };

    let projected = project_features(&[history]).unwrap();
    assert!(matches!(
        projected[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: None,
            distance: Length::ZERO,
        })
    ));
}

#[test]
fn planar_face_reference_requires_one_coincident_face() {
    let surface = Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#surface")
            .expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 12.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    };
    let face = Face {
        id: cadmpeg_ir::ids::FaceId::mint("test:model:entity#face").expect("identity grammar"),
        shell: cadmpeg_ir::ids::ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
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
    duplicate.id =
        cadmpeg_ir::ids::FaceId::mint("test:model:entity#duplicate").expect("identity grammar");
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
        id: cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#surface")
            .expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, -5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    };
    let face = Face {
        id: cadmpeg_ir::ids::FaceId::mint("test:model:entity#face").expect("identity grammar"),
        shell: cadmpeg_ir::ids::ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
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
        (
            "native-face",
            "0mm,0mm,0mm",
            "6mm",
            Length::new(6.0).unwrap(),
        ),
        (
            "sldprt:feature-input:surface-component-ids:630506365",
            "0mm,0mm,210mm",
            "40mm",
            Length::new(40.0).unwrap(),
        ),
    ] {
        let mut offset = feature("sldprt:history:feature#0:0", None, 0);
        offset.input_class = Some("moRefPlane_c".into());
        offset
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), distance_text.into());
        for (name, value) in [
            (cadmpeg_core::nonblank_literal!("Origin"), source_origin),
            (cadmpeg_core::nonblank_literal!("Normal"), "0,0,1"),
            (cadmpeg_core::nonblank_literal!("UAxis"), "1,0,0"),
            (
                cadmpeg_core::nonblank_literal!("ReferenceFaceNative"),
                native,
            ),
        ] {
            offset.properties.insert(name, value.into());
        }

        let definition = project_offset_plane(&offset, &HashMap::new()).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face { face }),
            distance,
        }) = definition
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
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "6mm".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "6mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,1".into());
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

    let projected = project_features(&[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: None,
            distance: actual_distance,
        }) if actual_distance.get() == 6.0
    ));
    assert!(projected[0].dependencies.is_empty());
}

#[test]
fn explicit_offset_plane_reference_cannot_bind_itself() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "0mm".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Reference"), "35".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "0,0,1".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "1,0,0".into());

    let projected = project_features(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![offset],
    }])
    .unwrap();

    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: None,
            distance: Length::ZERO,
        })
    ));
    assert!(projected[0].dependencies.is_empty());
}

#[test]
fn explicit_offset_plane_reference_orders_a_later_serialized_principal_first() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "6mm".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Reference"), "4".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "6mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,0,-1".into());
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

    let mut projected = project_features(&[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            distance: actual_distance,
        }) if (reference == &projected[1].id) && actual_distance.get() == 6.0
    ));
    assert_eq!(
        projected[0].dependencies.as_slice(),
        [projected[1].id.clone()]
    );
    assert!(
        with_test_ctx(|ctx| order_features_for_regeneration(ctx, &mut projected))
            .expect("test feature ordering")
    );
    assert_eq!(projected[1].ordinal, 0);
    assert_eq!(projected[0].ordinal, 1);
}

#[test]
fn explicit_principal_reference_survives_a_coincident_result_frame() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "6mm".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Reference"), "2".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "0,0,1".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "1,0,0".into());
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

    let mut projected = project_features(&[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            distance: actual_distance,
        }) if (reference == &projected[1].id) && actual_distance.get() == 6.0
    ));
    assert!(
        with_test_ctx(|ctx| order_features_for_regeneration(ctx, &mut projected))
            .expect("test feature ordering")
    );
    assert_eq!(projected[1].ordinal, 0);
    assert_eq!(projected[0].ordinal, 1);
}

#[test]
fn incompatible_later_principal_falls_back_to_the_serialized_face_frame() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "0mm".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Reference"), "4".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,5mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "0,1,0".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "1,0,0".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceOrigin"),
        "0mm,5mm,0mm".into(),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceNormal"),
        "0,1,0".into(),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceUAxis"),
        "1,0,0".into(),
    );
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

    let projected = project_features(&[history]).unwrap();

    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane { .. }),
            distance: Length::ZERO,
        })
    ));
    assert!(projected[0].dependencies.is_empty());
}

#[test]
fn explicit_offset_plane_reference_orders_a_later_derived_plane_first() {
    let mut offset = feature("sldprt:history:feature#0:0", Some("35"), 0);
    offset.input_class = Some("moRefPlane_c".into());
    offset
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "6mm".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Reference"), "40".into());
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "6mm,0mm,0mm".into(),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,1,0".into());
    let mut reference = feature("sldprt:history:feature#0:1", Some("40"), 1);
    reference.input_class = Some("moRefPlane_c".into());
    reference.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,0mm,0mm".into(),
    );
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "1,0,0".into());
    reference
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "0,1,0".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![offset, reference],
    };

    let mut projected = project_features(&[history]).unwrap();

    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            distance: actual_distance,
        }) if (reference == &projected[1].id) && actual_distance.get() == 6.0
    ));
    assert!(
        with_test_ctx(|ctx| order_features_for_regeneration(ctx, &mut projected))
            .expect("test feature ordering")
    );
    assert_eq!(projected[1].ordinal, 0);
    assert_eq!(projected[0].ordinal, 1);
}

fn front_and_offset(stored_origin: &str) -> Vec<cadmpeg_ir::features::Feature> {
    let datum = |id: &str, ordinal, definition| cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            definition,
        ),
        native_ref: None,
    };
    let front = datum(
        "synthetic:test:id#front",
        0,
        FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
            plane: cadmpeg_ir::features::PrincipalPlane::Front,
        }),
    );
    let mut offset = datum(
        "synthetic:test:id#offset",
        1,
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature {
                feature: front.id.clone(),
            }),
            distance: Length::new(5.0).unwrap(),
        }),
    );
    for (key, value) in [
        (cadmpeg_core::nonblank_literal!("Reference"), "2"),
        (cadmpeg_core::nonblank_literal!("Origin"), stored_origin),
        (cadmpeg_core::nonblank_literal!("Normal"), "0,0,1"),
        (cadmpeg_core::nonblank_literal!("UAxis"), "1,0,0"),
    ] {
        offset.source_properties.insert(key, value.into());
    }
    vec![front, offset]
}

fn offset_distance(feature: &cadmpeg_ir::features::Feature) -> f64 {
    let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { distance, .. }) =
        feature.evaluation.definition()
    else {
        panic!("offset plane");
    };
    distance.get()
}

/// `D1` is a magnitude: a stored frame at `-D1` from the source plane places
/// the offset plane on the negative side of the source normal.
#[test]
fn offset_plane_takes_the_side_its_stored_frame_states() {
    for (stored_origin, expected) in [("0mm,0mm,-5mm", -5.0), ("0mm,0mm,5mm", 5.0)] {
        let mut features = front_and_offset(stored_origin);
        let report = bind_offset_plane_references(&mut features);
        assert_eq!(offset_distance(&features[1]), expected);
        assert!(report.unstated.is_empty() && report.contradicted.is_empty());
    }
}

/// A stored frame in the source plane states no side: the distance stays and
/// the plane is reported.
#[test]
fn offset_plane_whose_frame_states_no_side_keeps_its_distance_and_is_reported() {
    let mut features = front_and_offset("0mm,0mm,0mm");
    let report = bind_offset_plane_references(&mut features);
    assert_eq!(offset_distance(&features[1]), 5.0);
    assert_eq!(report.unstated.count(), 1);
    assert!(report.contradicted.is_empty());
}

/// A stored frame parallel to the source plane at another distance than `D1`
/// is not the placed plane: the distance stays and the disagreement is named.
#[test]
fn offset_plane_whose_frame_lies_at_another_distance_is_reported_as_contradicted() {
    let mut features = front_and_offset("0mm,0mm,8mm");
    let report = bind_offset_plane_references(&mut features);
    assert_eq!(offset_distance(&features[1]), 5.0);
    assert!(report.unstated.is_empty());
    assert_eq!(report.contradicted.count(), 1);
    assert!(report
        .contradicted
        .to_string()
        .contains("distance other than D1"));
}

/// An offset plane whose source is a resolved plane frame (no source feature)
/// takes its side from its stored frame against that frame, and a stored
/// frame whose normal is not parallel to it is named as a disagreement.
#[test]
fn resolved_plane_offset_takes_the_side_its_stored_frame_states() {
    let resolved = |origin: &str, normal: &str| {
        let mut features = front_and_offset("0mm,0mm,0mm");
        let mut offset = features.remove(1);
        offset.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference,
                ..
            }) = definition
            {
                *reference = None;
            }
        });
        for (key, value) in [
            (cadmpeg_core::nonblank_literal!("Origin"), origin),
            (cadmpeg_core::nonblank_literal!("Normal"), normal),
            (
                cadmpeg_core::nonblank_literal!("ReferenceFaceOrigin"),
                "0mm,0mm,10mm",
            ),
            (
                cadmpeg_core::nonblank_literal!("ReferenceFaceNormal"),
                "0,0,1",
            ),
            (
                cadmpeg_core::nonblank_literal!("ReferenceFaceUAxis"),
                "1,0,0",
            ),
        ] {
            offset.source_properties.insert(key, value.into());
        }
        let mut features = vec![offset];
        let report = bind_offset_plane_references(&mut features);
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::ResolvedPlane { .. }),
                ..
            })
        ));
        (offset_distance(&features[0]), report)
    };
    let (distance, report) = resolved("0mm,0mm,5mm", "0,0,1");
    assert_eq!(distance, -5.0);
    assert!(report.unstated.is_empty() && report.contradicted.is_empty());

    let (distance, report) = resolved("0mm,0mm,10mm", "0,0,1");
    assert_eq!(distance, 5.0);
    assert_eq!(report.unstated.count(), 1);

    let (distance, report) = resolved("0mm,0mm,5mm", "0,1,0");
    assert_eq!(distance, 5.0);
    assert!(report.unstated.is_empty());
    assert!(report
        .contradicted
        .to_string()
        .contains("normal not parallel to the source plane"));
}

/// The Front offset plane of [`front_and_offset`] whose stored frame lies at
/// `stored_origin` and whose plane-data object stores the point
/// `plane_point` with the Front normal.
fn front_and_offset_with_plane_point(
    stored_origin: &str,
    plane_point: &str,
) -> Vec<cadmpeg_ir::features::Feature> {
    let mut features = front_and_offset(stored_origin);
    for (key, value) in [
        (cadmpeg_core::nonblank_literal!("PlanePoint"), plane_point),
        (cadmpeg_core::nonblank_literal!("PlaneNormal"), "0,0,1"),
    ] {
        features[1].source_properties.insert(key, value.into());
    }
    features
}

/// Where the stored frame is the source plane's, the point of the plane-data
/// object states the side, on either side of the source normal.
#[test]
fn offset_plane_takes_the_side_its_plane_data_point_states() {
    for (plane_point, expected) in [("2mm,1mm,-5mm", -5.0), ("0mm,0mm,5mm", 5.0)] {
        let mut features = front_and_offset_with_plane_point("0mm,0mm,0mm", plane_point);
        let report = bind_offset_plane_references(&mut features);
        assert_eq!(offset_distance(&features[1]), expected);
        assert!(report.unstated.is_empty() && report.contradicted.is_empty());
    }
}

/// A stored frame that states one side outranks a plane-data point on the
/// other: the frame's side stays and the disagreement is named.
#[test]
fn offset_plane_keeps_its_frame_side_against_a_plane_data_point_and_names_it() {
    let mut features = front_and_offset_with_plane_point("0mm,0mm,5mm", "0mm,0mm,-5mm");
    let report = bind_offset_plane_references(&mut features);
    assert_eq!(offset_distance(&features[1]), 5.0);
    assert!(report.unstated.is_empty());
    assert_eq!(report.contradicted.count(), 1);
    assert!(report
        .contradicted
        .to_string()
        .contains("plane-data point on the other side of the stored frame"));
}

/// A plane-data point at another distance than `D1` states no side: the sign
/// of `D1` stays and the disagreement is named.
#[test]
fn offset_plane_data_point_at_another_distance_is_named_and_states_no_side() {
    let mut features = front_and_offset_with_plane_point("0mm,0mm,0mm", "0mm,0mm,-7mm");
    let report = bind_offset_plane_references(&mut features);
    assert_eq!(offset_distance(&features[1]), 5.0);
    assert!(report.unstated.is_empty());
    assert!(report
        .contradicted
        .to_string()
        .contains("plane-data point parallel at a distance other than D1"));
}

/// A plane whose distance is the `D1` display is reported.
#[test]
fn offset_plane_with_a_displayed_distance_is_reported() {
    let mut features = front_and_offset("0mm,0mm,5mm");
    features[1].source_properties.insert(
        cadmpeg_core::nonblank_literal!("DistanceDisplayed"),
        "5".into(),
    );
    let report = bind_offset_plane_references(&mut features);
    assert_eq!(report.displayed.count(), 1);
    assert_eq!(offset_distance(&features[1]), 5.0);
}
