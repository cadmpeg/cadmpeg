//! Tests for the `projections` module.

use super::{
    project_unbound_cosmetic_thread_faces, unique_cylindrical_face, unique_planar_face,
    unique_topological_cylindrical_face,
};
use crate::records::{
    Feature, FeatureHistory, FeatureInputComponentPathEntry, FeatureInputLane,
    FeatureInputSurfaceSelection,
};
use cadmpeg_ir::features::{FeatureId, Length};
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
