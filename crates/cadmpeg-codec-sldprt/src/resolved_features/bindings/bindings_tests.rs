//! Tests for the `bindings` module.

use super::super::LEGACY_SKETCH_MARKER;
use super::{
    bind_mirror_surface_planes, bind_resolved_curve_vertices, normalize_indexed_curve_entities,
};
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputComponentPathEntry, FeatureInputLane,
    FeatureInputSurfaceSelection, SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PatternForm, PatternKind};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, Sense};
use std::collections::BTreeMap;

#[test]
fn mirror_plane_binds_through_one_persistent_face_identity() {
    let mut signature = [0; 12];
    signature[4..8].copy_from_slice(&45u32.to_le_bytes());
    let selection = FeatureInputSurfaceSelection {
        id: "selection".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_name_ref: "name".into(),
        feature_ref: "mirror-native".into(),
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
        components: vec![FeatureInputComponentPathEntry {
            instance: None,
            type_signature: signature,
            local_id: Some(7),
        }],
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
        surface_selections: vec![selection],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut feature = Feature {
        id: FeatureId("mirror".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Mirror),
            },
        },
        native_ref: Some("mirror-native".into()),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![NativeFeature {
            id: "mirror-native".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: Some("50".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "Mirror".into(),
            kind: "Mirror".into(),
            input_class: Some("moMirrorPattern_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    };
    let face = Face {
        id: FaceId("face".into()),
        shell: ShellId("shell".into()),
        surface: SurfaceId("surface".into()),
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    };
    let mut transform = cadmpeg_ir::transform::Transform::identity();
    transform.rows[0][3] = 12.0;
    let surface = Surface {
        id: SurfaceId("surface".into()),
        geometry: SurfaceGeometry::Transformed {
            basis: Box::new(SurfaceGeometry::Plane {
                origin: Point3::new(1.0, 2.0, 3.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }),
            transform,
        },
        source_object: None,
    };

    bind_mirror_surface_planes(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
        &[("face".into(), 45, 7)],
        std::slice::from_ref(&face),
        std::slice::from_ref(&surface),
    );
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Mirror {
                plane_origin: Point3 {
                    x: 13.0,
                    y: 2.0,
                    z: 3.0
                },
                plane_normal: Vector3 {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0
                },
            },
            ..
        }
    ));

    feature.definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Unresolved {
            form: Some(PatternForm::Mirror),
        },
    };
    let mut nonmirror_history = history.clone();
    nonmirror_history.features[0].input_class = Some("moCirPattern_c".into());
    bind_mirror_surface_planes(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&nonmirror_history),
        std::slice::from_ref(&lane),
        &[("face".into(), 45, 7)],
        std::slice::from_ref(&face),
        std::slice::from_ref(&surface),
    );
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Unresolved { .. },
            ..
        }
    ));

    let mut second_face = face.clone();
    second_face.id = FaceId("other-face".into());
    bind_mirror_surface_planes(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
        &[("face".into(), 45, 7), ("other-face".into(), 45, 7)],
        &[face, second_face],
        std::slice::from_ref(&surface),
    );
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Unresolved { .. },
            ..
        }
    ));
}

#[test]
fn indexed_curve_vertex_binding_follows_the_resolved_coordinate_roster() {
    let mut payload = vec![0; 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for start in (78..94).step_by(4) {
        payload[start..start + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
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
        sketch_entities: vec![
            entity("curve", 0, Some(1), SketchInputKind::Arc, None),
            entity("handle", 1, None, SketchInputKind::Point, Some([-1.0, 0.0])),
            entity(
                "start",
                2,
                Some(2),
                SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            entity("center", 3, None, SketchInputKind::Point, Some([0.5, 0.5])),
            entity(
                "end",
                4,
                Some(3),
                SketchInputKind::LineOrCircle,
                Some([1.0, 0.0]),
            ),
        ],
    };

    normalize_indexed_curve_entities(&mut lane);
    bind_resolved_curve_vertices(&mut lane);

    assert_eq!(lane.sketch_entities[4].kind, SketchInputKind::Point);
}
