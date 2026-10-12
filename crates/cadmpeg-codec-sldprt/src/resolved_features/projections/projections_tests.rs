//! Tests for the `projections` module.

use super::{
    full_round_fillet_selection_triple, project_compact_surface_selections,
    project_unbound_cosmetic_thread_faces, project_unbound_offset_plane_faces, FaceSurfaces,
};
use crate::records::FeatureSource;
use crate::records::{
    Feature, FeatureHistory, FeatureInputComponentPathEntry, FeatureInputLane,
    FeatureInputSurfaceSelection,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, Sense};
use cadmpeg_ir::{
    features::{
        edge_treatments::RadiusSpec, BodySelection, DatumPlaneReference, FaceSelection,
        FeatureDefinition, FeatureId, FeatureOperation, UnresolvedFamily,
    },
    scalar::Length,
};
use std::collections::BTreeMap;

mod character_growth;
mod identity_lookups;
mod limits;
mod patterns;
mod variable_fillets;

fn with_projection_context<R>(
    test: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
) -> R {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("variable fillet test context");
    test(&ctx)
}

#[test]
fn cosmetic_thread_radius_requires_one_topological_cylinder_face() {
    let surface = Surface {
        id: SurfaceId::mint("test:model:entity#cylinder").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                4.0,
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let face = Face {
        id: FaceId::mint("test:model:entity#face").expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    assert_eq!(
        with_projection_context(|ctx| FaceSurfaces::new(
            ctx,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        )
        .and_then(|faces| faces.unique_cylinder(ctx, 4.0)))
        .expect("cylindrical face search"),
        Some(face.id.clone())
    );
    assert_eq!(
        with_projection_context(|ctx| FaceSurfaces::new(
            ctx,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        )
        .and_then(|faces| faces.unique_cylinder(ctx, 3.0)))
        .expect("cylindrical face search"),
        None
    );
    assert_eq!(
        with_projection_context(|ctx| FaceSurfaces::new(
            ctx,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        )
        .and_then(|faces| faces.unique_cylinder_face(ctx)))
        .expect("topological cylinder search"),
        Some(face.id.clone())
    );
    let mut duplicate = face.clone();
    duplicate.id = FaceId::mint("test:model:entity#other-face").expect("identity grammar");
    assert_eq!(
        with_projection_context(|ctx| FaceSurfaces::new(
            ctx,
            &[face.clone(), duplicate.clone()],
            std::slice::from_ref(&surface)
        )
        .and_then(|faces| faces.unique_cylinder(ctx, 4.0)))
        .expect("cylindrical face search"),
        None
    );
    assert_eq!(
        with_projection_context(|ctx| FaceSurfaces::new(ctx, &[face, duplicate], &[surface])
            .and_then(|faces| faces.unique_cylinder_face(ctx)))
        .expect("topological cylinder search"),
        None
    );
}

#[test]
fn frame_only_plane_support_requires_one_coincident_face() {
    let surface = Surface {
        id: SurfaceId::mint("test:model:entity#plane").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 5.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let face = Face {
        id: FaceId::mint("test:model:entity#face").expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };

    assert_eq!(
        with_projection_context(|ctx| FaceSurfaces::new(
            ctx,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        )
        .and_then(|faces| faces.unique_plane(
            ctx,
            Point3::new(4.0, -2.0, 5.0),
            Vector3::new(0.0, 0.0, 1.0)
        )))
        .expect("planar face search"),
        Some(face.id.clone())
    );
    assert_eq!(
        with_projection_context(|ctx| FaceSurfaces::new(
            ctx,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        )
        .and_then(|faces| faces.unique_plane(
            ctx,
            Point3::new(0.0, 0.0, 6.0),
            Vector3::new(0.0, 0.0, 1.0)
        )))
        .expect("planar face search"),
        None
    );
    let mut duplicate = face.clone();
    duplicate.id = FaceId::mint("test:model:entity#other-face").expect("identity grammar");
    assert_eq!(
        with_projection_context(|ctx| FaceSurfaces::new(ctx, &[face, duplicate], &[surface])
            .and_then(|faces| faces.unique_plane(
                ctx,
                Point3::new(0.0, 0.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0)
            )))
        .expect("planar face search"),
        None
    );
}

#[test]
fn resolved_plane_binds_to_a_face_without_retaining_a_duplicate_frame() {
    let surface = Surface {
        id: SurfaceId::mint("test:model:entity#plane").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let face = Face {
        id: FaceId::mint("test:model:entity#face").expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::ResolvedPlane {
                    frame: cadmpeg_ir::features::FeatureSupportPlaneFrame::new(
                        Point3::new(0.0, 0.0, 5.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                }),
                distance: Length::new(4.0).unwrap(),
            }),
        ),
        native_ref: None,
    }];

    with_projection_context(|ctx| {
        project_unbound_offset_plane_faces(
            ctx,
            &mut features,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface),
        )
    })
    .expect("offset plane projection");

    let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face { face }),
        ..
    }) = features[0].evaluation.definition()
    else {
        panic!("expected offset-plane face reference");
    };
    assert_eq!(
        face,
        &FaceSelection::Faces(vec![
            FaceId::mint("test:model:entity#face").expect("identity grammar")
        ])
    );
}

#[test]
fn generic_native_offset_plane_support_stays_native() {
    let surface = Surface {
        id: SurfaceId::mint("test:model:entity#plane").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let face = Face {
        id: FaceId::mint("test:model:entity#face").expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    let native = "sldprt:feature-input:surface-component-ids:lane:40:200";
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Face {
                    face: FaceSelection::Native(native.into()),
                }),
                distance: Length::new(4.0).unwrap(),
            }),
        ),
        native_ref: None,
    }];

    with_projection_context(|ctx| {
        project_unbound_offset_plane_faces(
            ctx,
            &mut features,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface),
        )
    })
    .expect("offset plane projection");

    let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face { face }),
        ..
    }) = features[0].evaluation.definition()
    else {
        panic!("expected offset-plane face reference");
    };
    assert_eq!(face, &FaceSelection::Native(native.into()));
}

#[test]
fn cosmetic_thread_uses_consensus_persistent_face_path_before_radius() {
    let native_feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::try_from(source_id).expect("test feature source id")),
        ordinal: 0,
        name: id.to_string(),
        kind: "Feature".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer-native", "10"),
            native_feature("thread-native", "20"),
        ],
    };
    let neutral_feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        neutral_feature(
            "synthetic:test:id#producer",
            "producer-native",
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::BaseFeature {
                    bodies: cadmpeg_ir::features::BodySelection::Unresolved,
                },
            ),
        ),
        neutral_feature(
            "synthetic:test:id#thread",
            "thread-native",
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::CosmeticThread {
                    face: cadmpeg_ir::features::FaceSelection::Unresolved,
                    diameter: None,
                    extent: None,
                },
            ),
        ),
    ];
    let mut signature = [0; 12];
    // The explicit producer binding remains authoritative when a lane-local
    // signature carries a different source identity.
    signature[4..8].copy_from_slice(&99_u32.to_le_bytes());
    let selection = |parent: &str, offset| FeatureInputSurfaceSelection {
        id: format!("selection-{parent}"),
        parent: parent.into(),
        ordinal: 0,
        offset,
        selector: 0,
        kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
        object_name_ref: "name".into(),
        feature_ref: "thread-native".into(),
        producer_feature_refs: vec!["producer-native".into()],
        terminal_feature_ref: Some("producer-native".into()),
        components: vec![
            FeatureInputComponentPathEntry {
                instance: Some(0x8020),
                type_signature: signature,
                local_id: Some(7),
            },
            FeatureInputComponentPathEntry {
                instance: Some(0x8021),
                type_signature: signature,
                local_id: Some(u32::try_from(offset / 20).expect("test offset fits u32")),
            },
        ],
    };
    let lane = |id: &str, offset| FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![selection(id, offset)],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    crate::test_support::work_refusal_at("format SLDPRT cylinder reference separator", |ctx| {
        project_unbound_cosmetic_thread_faces(
            ctx,
            &mut features.clone(),
            std::slice::from_ref(&history),
            &[lane("lane-a", 40), lane("lane-b", 60)],
            &[],
            &[],
        )
    });
    with_projection_context(|ctx| {
        project_unbound_cosmetic_thread_faces(
            ctx,
            &mut features,
            std::slice::from_ref(&history),
            &[lane("lane-a", 40), lane("lane-b", 60)],
            &[],
            &[],
        )
    })
    .expect("cosmetic thread face projection");

    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::CosmeticThread { face, .. },
    ) = features[1].evaluation.definition()
    else {
        panic!("expected cosmetic thread");
    };
    assert!(matches!(
        face,
        cadmpeg_ir::features::FaceSelection::Generated { faces, native }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef::new(FeatureId::mint("synthetic:test:id#producer").expect("identity grammar"), "7".into(), &cadmpeg_test_support::service_decode_context(),).expect("selection reference admission").unwrap()]
                && native == "sldprt:feature-input:cylinder-reference:lane-a:40,lane-b:60"
    ));
    assert_eq!(
        features[1].dependencies.as_slice(),
        [FeatureId::mint("synthetic:test:id#producer").expect("identity grammar")]
    );

    let surface = Surface {
        id: SurfaceId::mint("test:model:entity#cylinder").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                4.0,
            )
            .unwrap(),
        )),
        source_object: None,
    };
    let topology_face = Face {
        id: FaceId::mint("test:model:entity#cylinder-face").expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    features[1].evaluation.edit(|definition, _| {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::CosmeticThread { face, diameter, .. },
        ) = definition
        else {
            panic!("expected cosmetic thread");
        };
        *face = cadmpeg_ir::features::FaceSelection::Unresolved;
        *diameter = Some(cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap());
    });
    with_projection_context(|ctx| {
        project_unbound_cosmetic_thread_faces(
            ctx,
            &mut features,
            std::slice::from_ref(&history),
            &[],
            std::slice::from_ref(&topology_face),
            std::slice::from_ref(&surface),
        )
    })
    .expect("cosmetic thread face projection");
    assert!(matches!(
        features[1].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::CosmeticThread {
            face: cadmpeg_ir::features::FaceSelection::Faces(faces),
            ..
        }) if faces == std::slice::from_ref(&topology_face.id)
    ));
}

#[test]
fn cosmetic_thread_accepts_repeated_carriers_with_distinct_owner_paths() {
    let native_feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::try_from(source_id).expect("test feature source id")),
        ordinal: 0,
        name: id.to_string(),
        kind: "Feature".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer-native", "10"),
            native_feature("thread-native", "20"),
        ],
    };
    let feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        feature(
            "synthetic:test:id#producer",
            "producer-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            }),
        ),
        feature(
            "synthetic:test:id#thread",
            "thread-native",
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                face: FaceSelection::Unresolved,
                diameter: None,
                extent: None,
            }),
        ),
    ];
    let mut face_signature = [0; 12];
    face_signature[4..8].copy_from_slice(&10_u32.to_le_bytes());
    let mut first_tail = face_signature;
    first_tail[8..12].copy_from_slice(&11_u32.to_le_bytes());
    let mut second_tail = face_signature;
    second_tail[8..12].copy_from_slice(&12_u32.to_le_bytes());
    let selection = |id: &str, tail: [u8; 12]| FeatureInputSurfaceSelection {
        id: id.into(),
        parent: id.into(),
        ordinal: 0,
        offset: 0,
        selector: 0,
        kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
        object_name_ref: "name".into(),
        feature_ref: "thread-native".into(),
        producer_feature_refs: vec!["producer-native".into()],
        terminal_feature_ref: Some("producer-native".into()),
        components: vec![
            FeatureInputComponentPathEntry {
                instance: Some(1),
                type_signature: face_signature,
                local_id: Some(7),
            },
            FeatureInputComponentPathEntry {
                instance: Some(2),
                type_signature: tail,
                local_id: Some(8),
            },
        ],
    };
    let lane = |id: &str, selection| FeatureInputLane {
        id: id.into(),
        configuration: Some(id.into()),
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![selection],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    with_projection_context(|ctx| {
        project_compact_surface_selections(
            ctx,
            &mut features,
            std::slice::from_ref(&history),
            &[
                lane("one", selection("one", first_tail)),
                lane("two", selection("two", second_tail)),
            ],
        )
    })
    .unwrap();

    assert!(matches!(
        features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face: FaceSelection::Generated { faces, native },
            ..
        }) if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef::new(FeatureId::mint("synthetic:test:id#producer").expect("identity grammar"), "7".into(), &cadmpeg_test_support::service_decode_context(),).expect("selection reference admission").unwrap()] && native == "sldprt:feature-input:surface-component-ids:7,8"
    ));
}

#[test]
fn compact_surface_selection_binds_surface_operation_face_slot() {
    let mut signature = [0; 12];
    signature[4..8].copy_from_slice(&10_u32.to_le_bytes());
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#operation").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
                faces: FaceSelection::Unresolved,
                distance: None,
            }),
        ),
        native_ref: Some("operation-native".into()),
    }];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![FeatureInputSurfaceSelection {
            id: "selection".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 12,
            selector: 0,
            kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "name".into(),
            feature_ref: "operation-native".into(),
            producer_feature_refs: Vec::new(),
            terminal_feature_ref: None,
            components: vec![FeatureInputComponentPathEntry {
                instance: Some(1),
                type_signature: signature,
                local_id: Some(7),
            }],
        }],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    with_projection_context(|ctx| {
        project_compact_surface_selections(ctx, &mut features, &[], &[lane])
    })
    .unwrap();

    let FeatureDefinition::Operation(FeatureOperation::OffsetSurface { faces, .. }) =
        features[0].evaluation.definition()
    else {
        panic!("expected offset surface");
    };
    assert!(matches!(faces, FaceSelection::Native(value) if value.contains(":7")));
}

#[test]
fn compact_surface_selection_binds_full_round_fillet_face_sets() {
    let feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        feature(
            "synthetic:test:id#producer",
            "producer-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            }),
        ),
        feature(
            "synthetic:test:id#fillet",
            "fillet-native",
            FeatureDefinition::Operation(FeatureOperation::Fillet {
                groups: cadmpeg_ir::features::NonEmptyMembers::one(
                    cadmpeg_ir::features::edge_treatments::FilletGroup {
                        edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
                        radius: RadiusSpec::Unresolved { form: None },
                        tangency_weight: None,
                    },
                ),
            }),
        ),
    ];
    let signature = [0x34, 0x80, 1, 0, 1, 0, 0, 0, 2, 0, 0, 0];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: [2u32, 4, 6]
            .into_iter()
            .enumerate()
            .map(|(ordinal, local_id)| FeatureInputSurfaceSelection {
                id: format!("selection-{ordinal}"),
                parent: "lane".into(),
                ordinal: u32::try_from(ordinal).unwrap(),
                offset: cadmpeg_core::decode::u64_from_index(ordinal),
                selector: 0,
                kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
                object_name_ref: "name".into(),
                feature_ref: "fillet-native".into(),
                producer_feature_refs: vec!["producer-native".into()],
                terminal_feature_ref: Some("producer-native".into()),
                components: vec![FeatureInputComponentPathEntry {
                    instance: Some(0x8020),
                    type_signature: signature,
                    local_id: Some(local_id),
                }],
            })
            .collect(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    let mut lane_two = lane.clone();
    lane_two.id = "lane-two".into();
    for selection in &mut lane_two.surface_selections {
        selection.parent = lane_two.id.clone();
    }
    with_projection_context(|ctx| {
        project_compact_surface_selections(ctx, &mut features, &[], &[lane, lane_two])
    })
    .unwrap();

    let FeatureDefinition::Operation(FeatureOperation::FullRoundFillet { groups }) =
        features[1].evaluation.definition()
    else {
        panic!("expected full-round fillet");
    };
    let [group] = groups.as_slice() else {
        panic!("expected one full-round group");
    };
    assert!(matches!(
        group.center_faces(),
        FaceSelection::Generated { faces, .. }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef::new(FeatureId::mint("synthetic:test:id#producer").expect("identity grammar"), "2".into(), &cadmpeg_test_support::service_decode_context(),).expect("selection reference admission").unwrap()]
    ));
    assert!(matches!(
        group.side_one_faces(),
        cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(FaceSelection::Generated {
            faces,
            ..
        }) if faces[0].local_id == "4"
    ));
    assert!(matches!(
        group.side_two_faces(),
        cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(FaceSelection::Generated {
            faces,
            ..
        }) if faces[0].local_id == "6"
    ));
    assert_eq!(
        features[1].dependencies.as_slice(),
        [FeatureId::mint("synthetic:test:id#producer").expect("identity grammar")]
    );
}

#[test]
fn compact_surface_cut_binds_target_body_and_tool_face_by_vector_order() {
    let feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        feature(
            "synthetic:test:id#target",
            "target-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            }),
        ),
        feature(
            "synthetic:test:id#tool",
            "tool-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            }),
        ),
        feature(
            "synthetic:test:id#cut",
            "cut-native",
            FeatureDefinition::Operation(FeatureOperation::CutWithSurface {
                targets: BodySelection::Unresolved,
                tools: FaceSelection::Unresolved,
                reverse: None,
            }),
        ),
    ];
    let signature = |source: u32| {
        let mut value = [0; 12];
        value[4..8].copy_from_slice(&source.to_le_bytes());
        value
    };
    let selection = |ordinal: u32, selector: u8, producer: &str, local_ids: &[u32]| {
        FeatureInputSurfaceSelection {
            id: format!("selection-{ordinal}"),
            parent: "lane".into(),
            ordinal,
            offset: u64::from(ordinal),
            selector,
            kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "cut-name".into(),
            feature_ref: "cut-native".into(),
            producer_feature_refs: vec![producer.into()],
            terminal_feature_ref: Some(producer.into()),
            components: local_ids
                .iter()
                .map(|local_id| FeatureInputComponentPathEntry {
                    instance: Some(0x81a5),
                    type_signature: signature(if producer == "target-native" { 10 } else { 20 }),
                    local_id: Some(*local_id),
                })
                .collect(),
        }
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![
            // These low selector bytes are lane-local subtypes.  They do not
            // identify the target/tool roles; native vector order does.
            selection(0, 7, "target-native", &[0, 3, 2]),
            selection(1, 1, "tool-native", &[0, 7]),
        ],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut lane2 = lane.clone();
    lane2.id = "lane-configuration-2".into();
    for selection in &mut lane2.surface_selections {
        selection.parent = lane2.id.clone();
    }

    crate::test_support::work_refusal_at("format SLDPRT surface cut body separator", |ctx| {
        project_compact_surface_selections(
            ctx,
            &mut features.clone(),
            &[],
            &[lane.clone(), lane2.clone()],
        )
    });
    with_projection_context(|ctx| {
        project_compact_surface_selections(ctx, &mut features, &[], &[lane, lane2])
    })
    .unwrap();

    let FeatureDefinition::Operation(FeatureOperation::CutWithSurface {
        targets,
        tools,
        reverse,
    }) = features[2].evaluation.definition()
    else {
        panic!("expected cut with surface");
    };
    assert!(matches!(
        targets,
        BodySelection::Generated { bodies, native }
            if bodies.as_slice() == [cadmpeg_ir::features::GeneratedBodyRef::new(FeatureId::mint("synthetic:test:id#target").expect("identity grammar"), "0,3,2".into(), &cadmpeg_test_support::service_decode_context(),).expect("selection reference admission").unwrap()] && native == "sldprt:feature-input:surface-component-ids:0,3,2"
    ));
    assert!(matches!(
        tools,
        FaceSelection::Generated { faces, native }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef::new(FeatureId::mint("synthetic:test:id#tool").expect("identity grammar"), "7".into(), &cadmpeg_test_support::service_decode_context(),).expect("selection reference admission").unwrap()] && native == "sldprt:feature-input:surface-component-ids:0,7"
    ));
    assert!(reverse.is_none());
    assert_eq!(
        features[2].dependencies.as_slice(),
        vec![
            FeatureId::mint("synthetic:test:id#target").expect("identity grammar"),
            FeatureId::mint("synthetic:test:id#tool").expect("identity grammar")
        ]
    );
}

#[test]
fn planar_surface_keeps_unresolved_definition_and_adds_defining_dependencies() {
    let feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        feature(
            "synthetic:test:id#first",
            "first-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            }),
        ),
        feature(
            "synthetic:test:id#second",
            "second-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            }),
        ),
        feature(
            "synthetic:test:id#plane",
            "plane-native",
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            }),
        ),
    ];
    let component = |source: u32, local_id: u32| {
        let mut type_signature = [0; 12];
        type_signature[4..8].copy_from_slice(&source.to_le_bytes());
        FeatureInputComponentPathEntry {
            instance: Some(0x8675),
            type_signature,
            local_id: Some(local_id),
        }
    };
    let selection =
        |ordinal: u32, producer: &str, source: u32, local_id: u32| FeatureInputSurfaceSelection {
            id: format!("selection-{ordinal}"),
            parent: "lane".into(),
            ordinal,
            offset: u64::from(ordinal),
            selector: if ordinal == 0 { 6 } else { 4 },
            kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "plane-name".into(),
            feature_ref: "plane-native".into(),
            producer_feature_refs: vec![producer.into()],
            terminal_feature_ref: Some(producer.into()),
            components: vec![component(source, local_id)],
        };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![
            selection(0, "first-native", 230, 16),
            selection(1, "second-native", 218, 12),
        ],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    with_projection_context(|ctx| {
        project_compact_surface_selections(ctx, &mut features, &[], &[lane])
    })
    .unwrap();

    assert!(matches!(
        features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane
        })
    ));
    assert_eq!(
        features[2].dependencies.as_slice(),
        vec![
            FeatureId::mint("synthetic:test:id#first").expect("identity grammar"),
            FeatureId::mint("synthetic:test:id#second").expect("identity grammar")
        ]
    );
}

#[test]
fn compact_surface_selection_accepts_semantic_lane_consensus() {
    let native_feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::try_from(source_id).expect("test feature source id")),
        ordinal: 0,
        name: id.to_string(),
        kind: "Feature".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer-native", "10"),
            native_feature("thread-native", "20"),
        ],
    };
    let feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        feature(
            "synthetic:test:id#producer",
            "producer-native",
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::BaseFeature {
                    bodies: cadmpeg_ir::features::BodySelection::Unresolved,
                },
            ),
        ),
        feature(
            "synthetic:test:id#thread",
            "thread-native",
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::CosmeticThread {
                    face: cadmpeg_ir::features::FaceSelection::Unresolved,
                    diameter: None,
                    extent: None,
                },
            ),
        ),
    ];
    let mut first_signature = [0; 12];
    first_signature[4..8].copy_from_slice(&10_u32.to_le_bytes());
    let mut second_signature = first_signature;
    second_signature[0] = 0x24;
    let selection = |parent: &str, signature| FeatureInputSurfaceSelection {
        id: format!("selection-{parent}"),
        parent: parent.into(),
        ordinal: 0,
        offset: 0,
        selector: 0,
        kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
        object_name_ref: "name".into(),
        feature_ref: "thread-native".into(),
        producer_feature_refs: vec!["producer-native".into()],
        terminal_feature_ref: Some("producer-native".into()),
        components: vec![FeatureInputComponentPathEntry {
            instance: Some(1),
            type_signature: signature,
            local_id: Some(7),
        }],
    };
    let lane = |id: &str, selection| FeatureInputLane {
        id: id.into(),
        configuration: Some(id.into()),
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![selection],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    with_projection_context(|ctx| {
        project_compact_surface_selections(
            ctx,
            &mut features,
            std::slice::from_ref(&history),
            &[
                lane("one", selection("one", first_signature)),
                lane("two", selection("two", second_signature)),
            ],
        )
    })
    .unwrap();

    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::CosmeticThread { face, .. },
    ) = features[1].evaluation.definition()
    else {
        panic!("expected cosmetic thread");
    };
    assert!(matches!(
        face,
        cadmpeg_ir::features::FaceSelection::Generated { faces, native }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef::new(FeatureId::mint("synthetic:test:id#producer").expect("identity grammar"), "7".into(), &cadmpeg_test_support::service_decode_context(),).expect("selection reference admission").unwrap()]
                && native == "sldprt:feature-input:surface-component-ids:7"
    ));

    features[1].dependencies.clear();
    features[1].evaluation.edit(|definition, _| {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::CosmeticThread { face, .. },
        ) = definition
        else {
            panic!("expected cosmetic thread");
        };
        *face = cadmpeg_ir::features::FaceSelection::Unresolved;
    });
    let mut conflicting = selection("conflicting", first_signature);
    conflicting.components[0].local_id = Some(8);
    with_projection_context(|ctx| {
        project_compact_surface_selections(
            ctx,
            &mut features,
            std::slice::from_ref(&history),
            &[
                lane("one", selection("one", first_signature)),
                lane("conflicting", conflicting),
            ],
        )
    })
    .unwrap();
    assert!(matches!(
        features[1].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::CosmeticThread {
                face: cadmpeg_ir::features::FaceSelection::Unresolved,
                ..
            }
        )
    ));
}

#[test]
fn split_face_collects_distinct_generated_target_faces() {
    let native_feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::try_from(source_id).expect("test feature source id")),
        ordinal: 0,
        name: id.to_string(),
        kind: "Feature".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native_feature("producer-a-native", "10"),
            native_feature("producer-b-native", "20"),
            native_feature("split-native", "30"),
        ],
    };
    let neutral_feature = |id: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref.into()),
    };
    let mut features = vec![
        neutral_feature(
            "synthetic:test:id#producer-a",
            "producer-a-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            }),
        ),
        neutral_feature(
            "synthetic:test:id#producer-b",
            "producer-b-native",
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            }),
        ),
        neutral_feature(
            "synthetic:test:id#split",
            "split-native",
            FeatureDefinition::Operation(FeatureOperation::SplitFace {
                targets: FaceSelection::Unresolved,
                tool: cadmpeg_ir::features::SplitFaceTool::Path(
                    cadmpeg_ir::features::PathRef::Native("tool".into()),
                ),
            }),
        ),
    ];
    let selection = |ordinal: u32, producer: &str, source: u32, local_id: u32| {
        let mut first_signature = [0; 12];
        first_signature[4..8].copy_from_slice(&30_u32.to_le_bytes());
        let mut last_signature = [0; 12];
        last_signature[4..8].copy_from_slice(&source.to_le_bytes());
        FeatureInputSurfaceSelection {
            id: format!("selection-{ordinal}"),
            parent: "lane".into(),
            ordinal,
            offset: u64::from(ordinal),
            selector: 0,
            kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "split-name".into(),
            feature_ref: "split-native".into(),
            producer_feature_refs: vec![producer.into()],
            terminal_feature_ref: Some(producer.into()),
            components: vec![
                FeatureInputComponentPathEntry {
                    instance: None,
                    type_signature: first_signature,
                    local_id: None,
                },
                FeatureInputComponentPathEntry {
                    instance: Some(0x8020 + u16::try_from(ordinal).unwrap()),
                    type_signature: last_signature,
                    local_id: Some(local_id),
                },
            ],
        }
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![
            selection(0, "producer-a-native", 10, 7),
            selection(1, "producer-b-native", 20, 9),
        ],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    with_projection_context(|ctx| {
        project_compact_surface_selections(ctx, &mut features, &[history], &[lane])
    })
    .unwrap();

    let FeatureDefinition::Operation(FeatureOperation::SplitFace { targets, .. }) =
        features[2].evaluation.definition()
    else {
        panic!("expected split face");
    };
    assert!(matches!(
        targets,
        FaceSelection::Generated { faces, native }
            if faces.as_slice() == [
                cadmpeg_ir::features::GeneratedFaceRef::new(FeatureId::mint("synthetic:test:id#producer-a").expect("identity grammar"), "7".into(), &cadmpeg_test_support::service_decode_context(),).expect("selection reference admission").unwrap(),
                cadmpeg_ir::features::GeneratedFaceRef::new(FeatureId::mint("synthetic:test:id#producer-b").expect("identity grammar"), "9".into(), &cadmpeg_test_support::service_decode_context(),).expect("selection reference admission").unwrap(),
            ] && native == "sldprt:feature-input:surface-selection-vectors:sldprt:feature-input:surface-component-ids:_,7;sldprt:feature-input:surface-component-ids:_,9"
    ));
    assert_eq!(
        features[2].dependencies.as_slice(),
        vec![
            FeatureId::mint("synthetic:test:id#producer-a").expect("identity grammar"),
            FeatureId::mint("synthetic:test:id#producer-b").expect("identity grammar")
        ]
    );
}

#[test]
fn a_full_round_fillet_triple_needs_three_ordered_selections_per_lane() {
    with_projection_context(|ctx| {
        let selection = |parent: &str, offset: u64, local_id: u32| FeatureInputSurfaceSelection {
            id: format!("{parent}-{offset}"),
            parent: parent.into(),
            ordinal: 0,
            offset,
            selector: 0,
            kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "name".into(),
            feature_ref: "fillet-native".into(),
            producer_feature_refs: vec!["producer-native".into()],
            terminal_feature_ref: Some("producer-native".into()),
            components: vec![FeatureInputComponentPathEntry {
                instance: Some(0x8020),
                type_signature: [0; 12],
                local_id: Some(local_id),
            }],
        };

        let lane = [
            selection("lane-one", 40, 3),
            selection("lane-one", 20, 2),
            selection("lane-one", 60, 1),
        ];
        let borrowed = lane.iter().collect::<Vec<_>>();
        let [center, side_one, side_two] = full_round_fillet_selection_triple(ctx, &borrowed)
            .expect("grouping")
            .expect("one lane of three is a triple");
        assert_eq!(
            [center.offset, side_one.offset, side_two.offset],
            [20, 40, 60]
        );

        let short = lane[..2].iter().collect::<Vec<_>>();
        assert!(full_round_fillet_selection_triple(ctx, &short)
            .expect("grouping")
            .is_none());

        let short_lane = [selection("lane-two", 20, 2), selection("lane-two", 40, 3)];
        let with_short_lane = lane.iter().chain(&short_lane).collect::<Vec<_>>();
        assert!(full_round_fillet_selection_triple(ctx, &with_short_lane)
            .expect("grouping")
            .is_none());

        let other_lane = [
            selection("lane-two", 20, 9),
            selection("lane-two", 40, 8),
            selection("lane-two", 60, 7),
        ];
        let disagreeing = lane.iter().chain(&other_lane).collect::<Vec<_>>();
        assert!(full_round_fillet_selection_triple(ctx, &disagreeing)
            .expect("grouping")
            .is_none());

        let fourth = [selection("lane-one", 80, 4)];
        let over_long = lane.iter().chain(&fourth).collect::<Vec<_>>();
        assert!(full_round_fillet_selection_triple(ctx, &over_long)
            .expect("grouping")
            .is_none());
    });
}

mod unresolved_fillet_groups;
