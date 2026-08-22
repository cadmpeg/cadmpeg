//! Tests for the `projections` module.

use super::*;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName,
    FeatureInputSurfaceSelection,
};
use cadmpeg_ir::features::{
    BodySelection, DatumPlaneReference, FaceSelection, FeatureDefinition, FeatureId, Length,
};
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
fn legacy_face_alias_support_preserves_native_identity() {
    let surface = Surface {
        id: SurfaceId("plane".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
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
    let native = "sldprt:feature-input:legacy-face-alias#lane:40:200";
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: FeatureId("feature".into()),
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
        definition: FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face {
                face: FaceSelection::Native(native.into()),
                origin: Point3::new(0.0, 0.0, 5.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }),
            distance: Length(4.0),
        },
        native_ref: None,
    }];

    project_unbound_offset_plane_faces(
        &mut features,
        std::slice::from_ref(&face),
        std::slice::from_ref(&surface),
    );

    let FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face { face, .. }),
        ..
    } = &features[0].definition
    else {
        panic!("expected offset-plane face reference");
    };
    assert_eq!(
        face,
        &FaceSelection::Resolved {
            faces: vec![FaceId("face".into())],
            native: native.into(),
        }
    );
}

#[test]
fn generic_native_offset_plane_support_stays_native() {
    let surface = Surface {
        id: SurfaceId("plane".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
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
    let native = "sldprt:feature-input:surface-component-ids:lane:40:200";
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: FeatureId("feature".into()),
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
        definition: FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face {
                face: FaceSelection::Native(native.into()),
                origin: Point3::new(0.0, 0.0, 5.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }),
            distance: Length(4.0),
        },
        native_ref: None,
    }];

    project_unbound_offset_plane_faces(
        &mut features,
        std::slice::from_ref(&face),
        std::slice::from_ref(&surface),
    );

    let FeatureDefinition::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face { face, .. }),
        ..
    } = &features[0].definition
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
    // The explicit producer binding remains authoritative when a lane-local
    // signature carries a different source identity.
    signature[4..8].copy_from_slice(&99_u32.to_le_bytes());
    let selection = |parent: &str, offset| FeatureInputSurfaceSelection {
        id: format!("selection-{parent}"),
        parent: parent.into(),
        ordinal: 0,
        offset,
        selector: 0,
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
fn cosmetic_thread_accepts_repeated_carriers_with_distinct_owner_paths() {
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
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        ),
        feature(
            "thread",
            "thread-native",
            FeatureDefinition::CosmeticThread {
                face: FaceSelection::Unresolved,
                diameter: None,
                extent: None,
            },
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

    project_compact_surface_selections(
        &mut features,
        std::slice::from_ref(&history),
        &[
            lane("one", selection("one", first_tail)),
            lane("two", selection("two", second_tail)),
        ],
    );

    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::CosmeticThread {
            face: FaceSelection::Generated { faces, native },
            ..
        } if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef {
            feature: FeatureId("producer".into()),
            local_id: "7".into(),
        }] && native == "sldprt:feature-input:surface-component-ids:7,8"
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
            selector: 0,
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
fn compact_surface_selection_binds_full_round_fillet_face_sets() {
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
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        ),
        feature(
            "fillet",
            "fillet-native",
            FeatureDefinition::Fillet {
                groups: vec![cadmpeg_ir::features::FilletGroup {
                    edges: cadmpeg_ir::features::EdgeSelection::Unresolved,
                    radius: cadmpeg_ir::features::RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                }],
            },
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
                ordinal: ordinal as u32,
                offset: ordinal as u64,
                selector: 0,
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
    project_compact_surface_selections(&mut features, &[], &[lane, lane_two]);

    let FeatureDefinition::FullRoundFillet { groups } = &features[1].definition else {
        panic!("expected full-round fillet");
    };
    let [group] = groups.as_slice() else {
        panic!("expected one full-round group");
    };
    assert!(matches!(
        &group.center_faces,
        FaceSelection::Generated { faces, .. }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef {
                feature: FeatureId("producer".into()),
                local_id: "2".into(),
            }]
    ));
    assert!(matches!(
        &group.side_one_faces,
        cadmpeg_ir::features::FullRoundSideSelection::Explicit(FaceSelection::Generated {
            faces,
            ..
        }) if faces[0].local_id == "4"
    ));
    assert!(matches!(
        &group.side_two_faces,
        cadmpeg_ir::features::FullRoundSideSelection::Explicit(FaceSelection::Generated {
            faces,
            ..
        }) if faces[0].local_id == "6"
    ));
    assert_eq!(features[1].dependencies, [FeatureId("producer".into())]);
}

#[test]
fn compact_surface_cut_binds_target_body_and_tool_face_by_vector_order() {
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
            "target",
            "target-native",
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        ),
        feature(
            "tool",
            "tool-native",
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        ),
        feature(
            "cut",
            "cut-native",
            FeatureDefinition::CutWithSurface {
                targets: BodySelection::Unresolved,
                tools: FaceSelection::Unresolved,
                reverse: None,
            },
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

    project_compact_surface_selections(&mut features, &[], &[lane, lane2]);

    let FeatureDefinition::CutWithSurface {
        targets,
        tools,
        reverse,
    } = &features[2].definition
    else {
        panic!("expected cut with surface");
    };
    assert!(matches!(
        targets,
        BodySelection::Generated { bodies, native }
            if bodies.as_slice() == [cadmpeg_ir::features::GeneratedBodyRef {
                feature: FeatureId("target".into()),
                local_id: "0,3,2".into(),
            }] && native == "sldprt:feature-input:surface-component-ids:0,3,2"
    ));
    assert!(matches!(
        tools,
        FaceSelection::Generated { faces, native }
            if faces.as_slice() == [cadmpeg_ir::features::GeneratedFaceRef {
                feature: FeatureId("tool".into()),
                local_id: "7".into(),
            }] && native == "sldprt:feature-input:surface-component-ids:0,7"
    ));
    assert!(reverse.is_none());
    assert_eq!(
        features[2].dependencies,
        vec![FeatureId("target".into()), FeatureId("tool".into())]
    );
}

#[test]
fn planar_surface_keeps_unresolved_definition_and_adds_defining_dependencies() {
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
            "first",
            "first-native",
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        ),
        feature(
            "second",
            "second-native",
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        ),
        feature(
            "plane",
            "plane-native",
            FeatureDefinition::DatumPlaneUnresolved,
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

    project_compact_surface_selections(&mut features, &[], &[lane]);

    assert!(matches!(
        features[2].definition,
        FeatureDefinition::DatumPlaneUnresolved
    ));
    assert_eq!(
        features[2].dependencies,
        vec![FeatureId("first".into()), FeatureId("second".into())]
    );
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
        selector: 0,
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
            selector: 0,
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

#[test]
fn variable_fillet_radii_join_control_vertices_to_edge_endpoints() {
    let signature = |serial: u32| {
        let mut value = [0u8; 12];
        value[..4].copy_from_slice(&[0x38, 0x80, 0x3b, 0]);
        value[4..8].copy_from_slice(&serial.to_le_bytes());
        value[8..].copy_from_slice(&(serial + 100).to_le_bytes());
        value
    };
    let first_vertex = signature(40);
    let second_vertex = signature(50);
    let mut payload = vec![0; 400];
    let class_name = "moVertDim_c";
    let class_offset = 60;
    payload[56..60].copy_from_slice(&[0x20, 0x81, 0x08, 0]);
    payload[class_offset..class_offset + 4].copy_from_slice(super::super::CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[class_offset + 6..class_offset + 6 + class_name.len()]
        .copy_from_slice(class_name.as_bytes());
    payload[class_offset + 6 + class_name.len()..class_offset + 8 + class_name.len()]
        .copy_from_slice(&0x87d3_u16.to_le_bytes());
    let write_control = |payload: &mut [u8], marker: usize, vertex: [u8; 12]| {
        payload[marker - 12..marker - 8].copy_from_slice(&3u32.to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
        payload[marker..marker + 16]
            .copy_from_slice(&super::super::selections::COMPACT_EDGE_VECTOR_MARKER);
        let mut cursor = marker + 18;
        for (instance, type_signature, local_id) in [
            (0x8521_u16, signature(20), 7_u32),
            (0x8521_u16, signature(20), 6_u32),
            (0x8083_u16, vertex, 1_u32),
        ] {
            payload[cursor..cursor + 2].copy_from_slice(&instance.to_le_bytes());
            payload[cursor + 4..cursor + 16].copy_from_slice(&type_signature);
            payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
            cursor += 20;
        }
    };
    write_control(&mut payload, 130, first_vertex);
    write_control(&mut payload, 240, second_vertex);

    let feature = Feature {
        id: "variable".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("10".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Variable fillet".into(),
        kind: "VarFillet".into(),
        input_class: Some("VarFillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D0".into(), "R2mm".into()), ("D01".into(), "R3mm".into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut next = feature.clone();
    next.id = "next".into();
    next.source_id = Some("11".into());
    next.ordinal = 1;
    next.name = "Next".into();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature, next],
    };
    let name = |id: &str, offset, object_id, value: &str| FeatureInputName {
        id: id.into(),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        object_id: Some(object_id),
        value: value.into(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "vertex-class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: class_offset as u64,
            name: class_name.into(),
            role: FeatureInputClassRole::Dimension,
        }],
        names: vec![
            name("feature-name", 20, 10, "Variable fillet"),
            name("d0-name", 100, 100, "D0"),
            name("d01-name", 210, 101, "D01"),
            name("next-name", 350, 11, "Next"),
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let endpoint = |type_signature| {
        vec![FeatureInputComponentPathEntry {
            instance: Some(0x8083),
            type_signature,
            local_id: Some(1),
        }]
    };
    let selection = FeatureInputEdgeSelection {
        id: "edge".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 80,
        object_name_ref: "feature-name".into(),
        feature_ref: "variable".into(),
        local_edge_ids: vec![7, 6, 1, 0],
        components: Vec::new(),
        references: vec![endpoint(first_vertex), endpoint(second_vertex)],
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
    };

    let groups = variable_fillet_radius_groups("variable", &[history], &[lane], &[&selection])
        .expect("vertex join");
    assert!(matches!(
        groups.as_slice(),
        [(RadiusSpec::Variable { points }, selections)]
            if matches!(points.as_slice(), [
                VariableRadius { parameter: 0.0, radius: Length(2.0) },
                VariableRadius { parameter: 1.0, radius: Length(3.0) },
            ]) && selections.len() == 1
    ));
}

#[test]
fn variable_fillet_legacy_edge_controls_apply_one_profile_to_endpointless_edges() {
    let signature = |serial: u32| {
        let mut value = [0u8; 12];
        value[..4].copy_from_slice(&[0x38, 0x80, 0x3b, 0]);
        value[4..8].copy_from_slice(&serial.to_le_bytes());
        value[8..].copy_from_slice(&(serial + 100).to_le_bytes());
        value
    };
    let mut payload = vec![0; 400];
    let class_name = "moEdgeDim_c";
    let class_offset = 60;
    payload[56..60].copy_from_slice(&[0x20, 0x81, 0x08, 0]);
    payload[class_offset..class_offset + 4].copy_from_slice(super::super::CLASS_MARKER);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&(class_name.len() as u16).to_le_bytes());
    payload[class_offset + 6..class_offset + 6 + class_name.len()]
        .copy_from_slice(class_name.as_bytes());
    payload[class_offset + 6 + class_name.len()..class_offset + 8 + class_name.len()]
        .copy_from_slice(&0x87d3_u16.to_le_bytes());
    let write_control = |payload: &mut [u8], marker: usize, edge_ids: &[u32]| {
        payload[marker - 12..marker - 8].copy_from_slice(&(edge_ids.len() as u32).to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[0, 2, 0, 0]);
        payload[marker..marker + 16]
            .copy_from_slice(&super::super::selections::COMPACT_EDGE_VECTOR_MARKER);
        let mut cursor = marker + 18;
        for local_id in edge_ids {
            payload[cursor..cursor + 2].copy_from_slice(&0x81a5_u16.to_le_bytes());
            payload[cursor + 4..cursor + 16].copy_from_slice(&signature(20));
            payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
            cursor += 20;
        }
    };
    write_control(&mut payload, 130, &[28, 29, 33]);
    write_control(&mut payload, 240, &[28, 29, 4]);

    let feature = Feature {
        id: "variable".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("10".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Variable fillet".into(),
        kind: "VarFillet".into(),
        input_class: Some("VarFillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D0".into(), "R2mm".into()), ("D1".into(), "R3mm".into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut next = feature.clone();
    next.id = "next".into();
    next.source_id = Some("11".into());
    next.ordinal = 1;
    next.name = "Next".into();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature, next],
    };
    let name = |id: &str, offset, object_id, value: &str| FeatureInputName {
        id: id.into(),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        object_id: Some(object_id),
        value: value.into(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "edge-class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: class_offset as u64,
            name: class_name.into(),
            role: FeatureInputClassRole::Dimension,
        }],
        names: vec![
            name("feature-name", 20, 10, "Variable fillet"),
            name("d0-name", 100, u32::MAX, "D0"),
            name("d1-name", 210, u32::MAX, "D1"),
            name("next-name", 350, 11, "Next"),
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let edge_reference = |local_id| {
        vec![FeatureInputComponentPathEntry {
            instance: Some(0x81a5),
            type_signature: signature(20),
            local_id: Some(local_id),
        }]
    };
    let selection = FeatureInputEdgeSelection {
        id: "edge".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 80,
        object_name_ref: "feature-name".into(),
        feature_ref: "variable".into(),
        local_edge_ids: vec![28, 29, 33, 4],
        components: Vec::new(),
        references: vec![
            edge_reference(28),
            edge_reference(29),
            edge_reference(33),
            edge_reference(4),
        ],
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
    };

    let groups = variable_fillet_radius_groups("variable", &[history], &[lane], &[&selection])
        .expect("legacy edge-control join");
    assert!(matches!(
        groups.as_slice(),
        [(RadiusSpec::Variable { points }, selections)]
            if matches!(points.as_slice(), [
                VariableRadius { parameter: 0.0, radius: Length(2.0) },
                VariableRadius { parameter: 1.0, radius: Length(3.0) },
            ]) && selections.len() == 1
    ));
}

#[test]
fn variable_fillet_two_control_roster_rejects_endpoint_collision() {
    let feature = Feature {
        id: "variable".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("10".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Variable fillet".into(),
        kind: "VarFillet".into(),
        input_class: Some("VarFillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("D0".into(), "R50".into()), ("D1".into(), "R4".into())]),
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
        features: vec![feature],
    };
    let component = |instance, local_id| FeatureInputComponentPathEntry {
        instance: Some(instance),
        type_signature: [0x38, 0x80, 0x3b, 0, 20, 0, 0, 0, 100, 0, 0, 0],
        local_id: Some(local_id),
    };
    let selection = FeatureInputEdgeSelection {
        id: "edge".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 80,
        object_name_ref: "name".into(),
        feature_ref: "variable".into(),
        local_edge_ids: vec![28, 36, 4],
        components: Vec::new(),
        references: vec![
            vec![component(0x81a5, 28)],
            vec![component(0x81a5, 36)],
            vec![component(0x81a5, 4)],
        ],
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
    };

    let groups = variable_fillet_radius_groups(
        "variable",
        std::slice::from_ref(&history),
        &[],
        &[&selection],
    )
    .expect("endpoint-less two-control roster");
    assert!(matches!(
        groups.as_slice(),
        [(RadiusSpec::Variable { points }, selections)]
            if matches!(points.as_slice(), [
                VariableRadius { parameter: 0.0, radius: Length(50.0) },
                VariableRadius { parameter: 1.0, radius: Length(4.0) },
            ]) && selections.len() == 1
    ));

    let mut collision = selection;
    collision.references[0][0].instance = Some(0x8083);
    assert!(variable_fillet_radius_groups("variable", &[history], &[], &[&collision]).is_none());
}
