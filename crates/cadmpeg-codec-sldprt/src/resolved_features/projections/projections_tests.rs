//! Tests for the `projections` module.

use super::*;
use crate::records::{
    Feature, FeatureHistory, FeatureInputComponentPathEntry, FeatureInputLane,
    FeatureInputSurfaceSelection,
};
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureId, Length};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, Sense};
use std::collections::BTreeMap;
#[test]
fn cosmetic_thread_radius_requires_one_topological_cylinder_face() {
    let surface = Surface {
        id: SurfaceId("cylinder".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        },
        source_object: None,
    };
    let face = Face {
        id: FaceId("face".into()),
        shell: ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };
    assert_eq!(
        unique_cylindrical_face(
            4.0,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        ),
        Some(face.id.clone())
    );
    assert_eq!(
        unique_cylindrical_face(
            3.0,
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        ),
        None
    );
    assert_eq!(
        unique_topological_cylindrical_face(
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface)
        ),
        Some(face.id.clone())
    );
    let mut duplicate = face.clone();
    duplicate.id = FaceId("other-face".into());
    assert_eq!(
        unique_cylindrical_face(
            4.0,
            &[face.clone(), duplicate.clone()],
            std::slice::from_ref(&surface),
        ),
        None
    );
    assert_eq!(
        unique_topological_cylindrical_face(&[face, duplicate], &[surface]),
        None
    );
}

#[test]
fn frame_only_plane_support_requires_one_coincident_face() {
    let surface = Surface {
        id: SurfaceId("plane".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 5.0),
            normal: Vector3::new(0.0, 0.0, -1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let face = Face {
        id: FaceId("face".into()),
        shell: ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };

    assert_eq!(
        unique_planar_face(
            Point3::new(4.0, -2.0, 5.0),
            Vector3::new(0.0, 0.0, 1.0),
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface),
        ),
        Some(face.id.clone())
    );
    assert_eq!(
        unique_planar_face(
            Point3::new(0.0, 0.0, 6.0),
            Vector3::new(0.0, 0.0, 1.0),
            std::slice::from_ref(&face),
            std::slice::from_ref(&surface),
        ),
        None
    );
    let mut duplicate = face.clone();
    duplicate.id = FaceId("other-face".into());
    assert_eq!(
        unique_planar_face(
            Point3::new(0.0, 0.0, 5.0),
            Vector3::new(0.0, 0.0, 1.0),
            &[face, duplicate],
            &[surface],
        ),
        None
    );
}

#[test]
fn cosmetic_thread_uses_consensus_persistent_face_path_before_radius() {
    let native_feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: 0,
        name: id.into(),
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
        id: FeatureId(id.into()),
        ordinal: 0,
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
    let mut features = vec![
        neutral_feature(
            "producer",
            "producer-native",
            cadmpeg_ir::features::FeatureDefinition::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            },
        ),
        neutral_feature(
            "thread",
            "thread-native",
            cadmpeg_ir::features::FeatureDefinition::CosmeticThread {
                face: cadmpeg_ir::features::FaceSelection::Unresolved,
                diameter: None,
                extent: None,
            },
        ),
    ];
    let mut signature = [0; 12];
    signature[4..8].copy_from_slice(&10_u32.to_le_bytes());
    let selection = |parent: &str, offset| FeatureInputSurfaceSelection {
        id: format!("selection-{parent}"),
        parent: parent.into(),
        ordinal: 0,
        offset,
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

    project_unbound_cosmetic_thread_faces(
        &mut features,
        std::slice::from_ref(&history),
        &[lane("lane-a", 40), lane("lane-b", 60)],
        &[],
        &[],
    );

    let cadmpeg_ir::features::FeatureDefinition::CosmeticThread { face, .. } =
        &features[1].definition
    else {
        panic!("expected cosmetic thread");
    };
    assert!(matches!(
        face,
        cadmpeg_ir::features::FaceSelection::Generated { faces, native }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef {
                feature: FeatureId("producer".into()),
                local_id: "7".into(),
            }]
                && native == "sldprt:feature-input:cylinder-reference:lane-a:40,lane-b:60"
    ));
    assert_eq!(features[1].dependencies, [FeatureId("producer".into())]);

    let surface = Surface {
        id: SurfaceId("cylinder".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        },
        source_object: None,
    };
    let topology_face = Face {
        id: FaceId("cylinder-face".into()),
        shell: ShellId("shell".into()),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };
    let cadmpeg_ir::features::FeatureDefinition::CosmeticThread { face, diameter, .. } =
        &mut features[1].definition
    else {
        panic!("expected cosmetic thread");
    };
    *face = cadmpeg_ir::features::FaceSelection::Unresolved;
    *diameter = Some(Length(8.0));
    project_unbound_cosmetic_thread_faces(
        &mut features,
        std::slice::from_ref(&history),
        &[],
        std::slice::from_ref(&topology_face),
        std::slice::from_ref(&surface),
    );
    assert!(matches!(
        &features[1].definition,
        cadmpeg_ir::features::FeatureDefinition::CosmeticThread {
            face: cadmpeg_ir::features::FaceSelection::Faces(faces),
            ..
        } if faces == std::slice::from_ref(&topology_face.id)
    ));
}

#[test]
fn compact_surface_selection_binds_surface_operation_face_slot() {
    let mut signature = [0; 12];
    signature[4..8].copy_from_slice(&10_u32.to_le_bytes());
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: FeatureId("operation".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
        },
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

    project_compact_surface_selections(&mut features, &[], &[lane]);

    let FeatureDefinition::OffsetSurface { faces, .. } = &features[0].definition else {
        panic!("expected offset surface");
    };
    assert!(matches!(faces, FaceSelection::Native(value) if value.contains(":7")));
}

#[test]
fn compact_surface_selection_accepts_semantic_lane_consensus() {
    let native_feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: 0,
        name: id.into(),
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
        id: FeatureId(id.into()),
        ordinal: 0,
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
    let mut features = vec![
        feature(
            "producer",
            "producer-native",
            cadmpeg_ir::features::FeatureDefinition::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            },
        ),
        feature(
            "thread",
            "thread-native",
            cadmpeg_ir::features::FeatureDefinition::CosmeticThread {
                face: cadmpeg_ir::features::FaceSelection::Unresolved,
                diameter: None,
                extent: None,
            },
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

    project_compact_surface_selections(
        &mut features,
        std::slice::from_ref(&history),
        &[
            lane("one", selection("one", first_signature)),
            lane("two", selection("two", second_signature)),
        ],
    );

    let cadmpeg_ir::features::FeatureDefinition::CosmeticThread { face, .. } =
        &features[1].definition
    else {
        panic!("expected cosmetic thread");
    };
    assert!(matches!(
        face,
        cadmpeg_ir::features::FaceSelection::Generated { faces, native }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef {
                feature: FeatureId("producer".into()),
                local_id: "7".into(),
            }]
                && native == "sldprt:feature-input:surface-component-ids:7"
    ));

    features[1].dependencies.clear();
    let cadmpeg_ir::features::FeatureDefinition::CosmeticThread { face, .. } =
        &mut features[1].definition
    else {
        panic!("expected cosmetic thread");
    };
    *face = cadmpeg_ir::features::FaceSelection::Unresolved;
    let mut conflicting = selection("conflicting", first_signature);
    conflicting.components[0].local_id = Some(8);
    project_compact_surface_selections(
        &mut features,
        std::slice::from_ref(&history),
        &[
            lane("one", selection("one", first_signature)),
            lane("conflicting", conflicting),
        ],
    );
    assert!(matches!(
        &features[1].definition,
        cadmpeg_ir::features::FeatureDefinition::CosmeticThread {
            face: cadmpeg_ir::features::FaceSelection::Unresolved,
            ..
        }
    ));
}

#[test]
fn split_face_collects_distinct_generated_target_faces() {
    let native_feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: 0,
        name: id.into(),
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
        id: FeatureId(id.into()),
        ordinal: 0,
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
    let mut features = vec![
        neutral_feature(
            "producer-a",
            "producer-a-native",
            FeatureDefinition::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            },
        ),
        neutral_feature(
            "producer-b",
            "producer-b-native",
            FeatureDefinition::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            },
        ),
        neutral_feature(
            "split",
            "split-native",
            FeatureDefinition::SplitFace {
                targets: FaceSelection::Unresolved,
                tool: cadmpeg_ir::features::SplitFaceTool::Path(
                    cadmpeg_ir::features::PathRef::Native("tool".into()),
                ),
            },
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
                    instance: Some(0x8020 + ordinal as u16),
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

    project_compact_surface_selections(&mut features, &[history], &[lane]);

    let FeatureDefinition::SplitFace { targets, .. } = &features[2].definition else {
        panic!("expected split face");
    };
    assert!(matches!(
        targets,
        FaceSelection::Generated { faces, native }
            if faces == &vec![
                cadmpeg_ir::features::GeneratedFaceRef {
                    feature: FeatureId("producer-a".into()),
                    local_id: "7".into(),
                },
                cadmpeg_ir::features::GeneratedFaceRef {
                    feature: FeatureId("producer-b".into()),
                    local_id: "9".into(),
                },
            ] && native == "sldprt:feature-input:surface-selection-vectors:sldprt:feature-input:surface-component-ids:_,7;sldprt:feature-input:surface-component-ids:_,9"
    ));
    assert_eq!(
        features[2].dependencies,
        vec![
            FeatureId("producer-a".into()),
            FeatureId("producer-b".into())
        ]
    );
}
