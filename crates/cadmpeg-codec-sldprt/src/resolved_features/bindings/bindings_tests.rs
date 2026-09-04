//! Tests for the `bindings` module.

use super::super::{LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use super::{
    bind_detached_legacy_sketch_objects, bind_mirror_surface_planes, bind_pattern_inputs,
    bind_resolved_curve_vertices, bind_scalar_operands, normalize_indexed_curve_entities,
    represented_sketch_features,
};
use crate::layout::temporary_axis_reference_nine_scalar as temporary_axis;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputComponentPathEntry, FeatureInputGeneratedSurfaceIdentity, FeatureInputLane,
    FeatureInputName, FeatureInputScalar, FeatureInputScalarRole, FeatureInputSurfaceSelection,
    SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, PatternForm, PatternKind, PatternSeed,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, Sense};
use std::collections::{BTreeMap, HashSet};

#[test]
fn dissected_profile_scalar_tail_belongs_to_parent_extrusion() {
    let native_feature = |id: &str,
                          source_id: &str,
                          ordinal,
                          name: &str,
                          xml_tag: &str,
                          input_class: Option<&str>| NativeFeature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: xml_tag.into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal,
        name: name.into(),
        kind: name.into(),
        input_class: input_class.map(str::to_string),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut extrusion = native_feature("extrusion", "10", 0, "Cut-Extrude-Thin", "Extrusion", None);
    extrusion.parameters.insert("D5".into(), "0.3".into());
    let mut child = native_feature(
        "profile-child",
        "11",
        1,
        "Sketch4<3>",
        "Sketch",
        Some("moProfileFeature_c"),
    );
    child
        .properties
        .insert("Description".into(), child.name.clone());
    let following = native_feature("following", "12", 2, "Following", "Feature", None);
    let attribute = native_feature(
        "attribute",
        "-1",
        3,
        "Attribute-Definition",
        "Feature",
        None,
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![extrusion, child, following, attribute],
    };
    let name = |id: &str, offset, object_id, value: &str| FeatureInputName {
        id: id.into(),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        object_id: Some(object_id),
        value: value.into(),
    };
    let scalar = |id: &str, offset, name: &str| FeatureInputScalar {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: None,
        ordinal: 0,
        offset,
        object_id: 20,
        name: name.into(),
        value: 0.001,
        role: FeatureInputScalarRole::Driving,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 500],
        classes: Vec::new(),
        names: vec![
            name("extrusion-name", 100, 10, "Cut-Extrude-Thin"),
            name("child-name", 200, 11, "Sketch4<3>"),
            name("d5-name", 240, 20, "D5"),
            name("d6-name", 280, 21, "D6"),
            name("d7-name", 320, 22, "D7"),
            name("attribute-name", 360, 99, "Attribute-Definition"),
            name("following-name", 400, 12, "Following"),
            name("later-name", 440, 23, "later"),
        ],
        scalars: vec![
            scalar("d5", 250, "d5-name"),
            scalar("d6", 290, "d6-name"),
            scalar("d7", 330, "d7-name"),
            scalar("later", 450, "later-name"),
        ],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![SketchInputEntity {
            id: "sketch-marker".into(),
            parent: "lane".into(),
            feature_ref: None,
            ordinal: 0,
            offset: 370,
            object_index: Some(1),
            local_id: None,
            kind: SketchInputKind::Point,
            state_value: Some(1.0),
            coordinates_m: Some([0.0, 0.0]),
            links: Vec::new(),
            link_selector: None,
        }],
    };

    bind_scalar_operands(
        std::slice::from_ref(&history),
        std::slice::from_mut(&mut lane),
    );

    assert!(lane.scalars[..3]
        .iter()
        .all(|scalar| scalar.feature_ref.as_deref() == Some("extrusion")));
    assert_eq!(lane.scalars[3].feature_ref.as_deref(), Some("following"));
    assert_eq!(
        lane.sketch_entities[0].feature_ref.as_deref(),
        Some("profile-child")
    );
    assert_eq!(
        represented_sketch_features(std::slice::from_ref(&history), std::slice::from_ref(&lane)),
        HashSet::from([String::from("profile-child")])
    );
}

#[test]
fn mirror_plane_binds_through_one_persistent_face_identity() {
    let mut signature = [0; 12];
    signature[4..8].copy_from_slice(&45u32.to_le_bytes());
    let selection = FeatureInputSurfaceSelection {
        id: "selection".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        selector: 0,
        endpoint_selector: None,
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
            input_class: Some("moMirrorSolid_c".into()),
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
fn circular_pattern_seed_binds_from_generated_identity_path() {
    let signature = |source: u32, identity: u32| {
        let mut signature = [0; 12];
        signature[4..8].copy_from_slice(&source.to_le_bytes());
        signature[8..12].copy_from_slice(&identity.to_le_bytes());
        signature
    };
    let pattern_native = NativeFeature {
        id: "pattern-native".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("228".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "CirPattern1".into(),
        kind: "CirPattern".into(),
        input_class: Some("moCirPattern_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let seed_native = NativeFeature {
        id: "seed-native".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("224".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "HoleWizard1".into(),
        kind: "HoleWizard".into(),
        input_class: Some("moHoleWzd_c".into()),
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
        features: vec![pattern_native, seed_native],
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 256],
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "pattern-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            object_id: Some(228),
            value: "CirPattern1".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: vec![FeatureInputGeneratedSurfaceIdentity {
            id: "identity".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 150,
            type_prefix: [0xc2, 0x83, 0xfb, 0x08],
            feature_source_id: 224,
            local_identity: 2,
            components: vec![
                FeatureInputComponentPathEntry {
                    instance: Some(0x8aaa),
                    type_signature: signature(228, 1),
                    local_id: None,
                },
                FeatureInputComponentPathEntry {
                    instance: Some(0x89c9),
                    type_signature: signature(224, 2),
                    local_id: Some(2),
                },
            ],
        }],
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut features = vec![
        Feature {
            id: FeatureId("pattern".into()),
            ordinal: 0,
            name: Some("CirPattern1".into()),
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::Unresolved {
                    form: Some(PatternForm::Circular),
                },
            },
            native_ref: Some("pattern-native".into()),
        },
        Feature {
            id: FeatureId("seed".into()),
            ordinal: 1,
            name: Some("HoleWizard1".into()),
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::Unresolved { form: None },
            },
            native_ref: Some("seed-native".into()),
        },
    ];

    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&history),
        std::slice::from_mut(&mut lane),
    );

    assert_eq!(features[0].dependencies, vec![FeatureId("seed".into())]);
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Pattern { seeds, pattern: PatternKind::Unresolved { form: Some(PatternForm::Circular) } }
            if seeds == &[PatternSeed::Feature(FeatureId("seed".into()))]
    ));
}

#[test]
fn circular_pattern_axis_binds_from_unique_temporary_axis() {
    let declaration = 120;
    let mut payload = vec![0; 512];
    payload[declaration + temporary_axis::CLASS_MARKER
        ..declaration + temporary_axis::CLASS_MARKER + temporary_axis::CLASS_MARKER_VALUE.len()]
        .copy_from_slice(&temporary_axis::CLASS_MARKER_VALUE);
    payload
        [declaration + temporary_axis::NAME_LENGTH..declaration + temporary_axis::NAME_LENGTH + 2]
        .copy_from_slice(&temporary_axis::NAME_LENGTH_VALUE.to_le_bytes());
    payload[declaration + temporary_axis::NAME
        ..declaration + temporary_axis::NAME + temporary_axis::NAME_VALUE.len()]
        .copy_from_slice(&temporary_axis::NAME_VALUE);
    payload[declaration + temporary_axis::HANDLES
        ..declaration + temporary_axis::HANDLES + temporary_axis::HANDLES_VALUE.len()]
        .copy_from_slice(&temporary_axis::HANDLES_VALUE);
    payload[declaration + temporary_axis::STREAM_ADDRESS
        ..declaration + temporary_axis::STREAM_ADDRESS + 4]
        .copy_from_slice(&5000u32.to_le_bytes());
    for (index, value) in [0.012f64, -0.034, 0.056, 0.1, 0.2, 0.3, 0.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = declaration + temporary_axis::AXIS_FRAME + index * 8;
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[declaration + temporary_axis::NEXT_CLASS_MARKER
        ..declaration
            + temporary_axis::NEXT_CLASS_MARKER
            + temporary_axis::NEXT_CLASS_MARKER_VALUE.len()]
        .copy_from_slice(&temporary_axis::NEXT_CLASS_MARKER_VALUE);

    let native_pattern = NativeFeature {
        id: "pattern-native".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("228".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "CirPattern1".into(),
        kind: "CirPattern".into(),
        input_class: Some("moCirPattern_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([
            ("Angle".into(), "90deg".into()),
            ("Count".into(), "4".into()),
        ]),
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
        features: vec![native_pattern],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "pattern-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            object_id: Some(228),
            value: "CirPattern1".into(),
        }],
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
    let mut features = vec![Feature {
        id: FeatureId("pattern".into()),
        ordinal: 0,
        name: Some("CirPattern1".into()),
        suppressed: Some(false),
        dependencies: vec![FeatureId("seed".into())],
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: vec![PatternSeed::Feature(FeatureId("seed".into()))],
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Circular),
            },
        },
        native_ref: Some("pattern-native".into()),
    }];

    bind_pattern_inputs(&mut features, std::slice::from_ref(&history), &[lane]);

    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Circular {
                axis_origin,
                axis_dir,
                angle,
                count,
            },
            ..
        } if *axis_origin == Point3::new(12.0, -34.0, 56.0)
            && *axis_dir == Vector3::new(0.0, 1.0, 0.0)
            && angle.0 == std::f64::consts::FRAC_PI_2
            && *count == 4
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

#[test]
fn local_link_promotes_a_coordinate_bearing_curve_to_a_profile_vertex() {
    let mut payload = vec![0; 157];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[82..86].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    payload[86..88].copy_from_slice(&0xbc87u16.to_le_bytes());
    payload[88..90].copy_from_slice(&22u16.to_le_bytes());
    payload[90..94].fill(0xff);
    payload[102..106].copy_from_slice(&(-2i32).to_le_bytes());
    payload[152..].copy_from_slice(SKETCH_MARKER);
    let entity = |id: &str, offset, local_id, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id,
        kind,
        state_value: None,
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
            entity(
                "line",
                0,
                Some(1),
                SketchInputKind::LineOrCircle,
                Some([2.0, 3.0]),
            ),
            entity(
                "curve-vertex",
                152,
                Some(22),
                SketchInputKind::LineOrCircle,
                Some([4.0, 5.0]),
            ),
        ],
    };

    bind_resolved_curve_vertices(&mut lane);

    assert_eq!(lane.sketch_entities[0].kind, SketchInputKind::LineOrCircle);
    assert_eq!(lane.sketch_entities[1].kind, SketchInputKind::Point);
}

#[test]
fn detached_spatial_relation_group_binds_by_its_complete_dimension_signature() {
    let feature = NativeFeature {
        id: "spatial".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("7".into()),
        parent_source_id: None,
        ordinal: 0,
        name: "Position".into(),
        kind: "3DSketch".into(),
        input_class: Some("mo3DProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([
            ("D1".into(), "10".into()),
            ("D2".into(), "20".into()),
            ("D3".into(), "30".into()),
            ("Mode".into(), "authored".into()),
        ]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let class = |offset, name: &str| FeatureInputClass {
        id: format!("class-{offset}"),
        parent: "sldprt:feature-input:config-objects#1".into(),
        ordinal: 0,
        offset,
        name: name.into(),
        role: FeatureInputClassRole::Native,
    };
    let name = |index, value: &str| FeatureInputName {
        id: format!("name-{index}"),
        parent: "sldprt:feature-input:config-objects#1".into(),
        ordinal: index,
        offset: 250 + u64::from(index),
        object_id: None,
        value: value.into(),
    };
    let scalar = |index, value| FeatureInputScalar {
        id: format!("scalar-{index}"),
        parent: "sldprt:feature-input:config-objects#1".into(),
        feature_ref: None,
        ordinal: index,
        offset: 300 + 20 * u64::from(index),
        object_id: index,
        name: format!("name-{index}"),
        value,
        role: FeatureInputScalarRole::Driving,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    };
    let entity = |id: &str, offset| SketchInputEntity {
        id: id.into(),
        parent: "sldprt:feature-input:config-objects#1".into(),
        feature_ref: None,
        ordinal: 0,
        offset,
        object_index: Some(1),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "sldprt:feature-input:config-objects#1".into(),
        configuration: Some("0".into()),
        native_payload: vec![0; 700],
        classes: vec![
            class(100, "moRelMgr_c"),
            class(200, "sg3DPlaneHandle"),
            class(500, "suObList"),
        ],
        names: vec![name(0, "D1"), name(1, "D2"), name(2, "D3")],
        scalars: vec![scalar(0, 0.01), scalar(1, 0.02), scalar(2, 0.03)],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![entity("inside", 150), entity("outside", 550)],
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature.clone()],
    };

    bind_detached_legacy_sketch_objects(
        std::slice::from_ref(&history),
        &HashSet::default(),
        &mut lane,
    );
    assert_eq!(
        lane.sketch_entities[0].feature_ref.as_deref(),
        Some("spatial")
    );
    assert_eq!(lane.sketch_entities[1].feature_ref, None);
    assert!(lane
        .scalars
        .iter()
        .all(|scalar| scalar.feature_ref.as_deref() == Some("spatial")));

    let mut ambiguous = lane;
    for entity in &mut ambiguous.sketch_entities {
        entity.feature_ref = None;
    }
    for scalar in &mut ambiguous.scalars {
        scalar.feature_ref = None;
    }
    let mut second = feature;
    second.id = "other-spatial".into();
    let mut ambiguous_history = history;
    ambiguous_history.features.push(second);
    bind_detached_legacy_sketch_objects(
        std::slice::from_ref(&ambiguous_history),
        &HashSet::default(),
        &mut ambiguous,
    );
    assert!(ambiguous
        .sketch_entities
        .iter()
        .all(|entity| entity.feature_ref.is_none()));
}
