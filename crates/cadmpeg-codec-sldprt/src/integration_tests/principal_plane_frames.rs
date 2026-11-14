// SPDX-License-Identifier: Apache-2.0

use crate::history::project::project_features;
use crate::records::{Feature, FeatureHistory, FeatureInputLane, FeatureSource};
use crate::resolved_features::curves::lane_sketch_plane_frames;
use crate::resolved_features::holes::coplanar_plane_frames;
use cadmpeg_ir::features::{
    DatumPlaneReference, FeatureDefinition, FeatureOperation, PrincipalPlane,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;

fn front_offset_pair(distance: f64) {
    let native = |id: &str, source, ordinal, name: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "RefPlane".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal,
        name: name.into(),
        kind: String::new(),
        input_class: Some("moRefPlane_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let principal = native("sldprt:history:feature#0:front", 2, 0, "Front");
    let mut offset = native("sldprt:history:feature#0:offset", 7, 1, "Offset");
    offset.parameters.insert(
        cadmpeg_core::nonblank_literal!("D1"),
        format!("{distance}mm"),
    );
    offset.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        format!("0mm,0mm,{distance}mm"),
    );
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "0,0,1".into());
    offset
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "1,0,0".into());
    let mut history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![principal, offset],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
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
    };
    for explicit_reference in [false, true] {
        if explicit_reference {
            let reference = history.features[0].id.clone();
            history.features[1]
                .properties
                .insert(cadmpeg_core::nonblank_literal!("Reference"), reference);
        }
        let histories = [history.clone()];
        let features = project_features(&histories).expect("synthetic history");
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                plane: PrincipalPlane::Front,
            })
        ));
        assert!(matches!(
            features[1].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature { feature }),
                distance: actual,
            }) if feature == &features[0].id && actual.get() == distance
        ));
        let frames = lane_sketch_plane_frames(&features, &histories, &lane);
        let front = frames.get(&2).expect("Front sketch frame").as_tuple();
        let offset = frames.get(&7).expect("offset sketch frame").as_tuple();
        assert_eq!(front.0, Point3::new(0.0, 0.0, 0.0));
        assert_eq!(front.1, Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(offset.1, Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(offset.2, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(offset.0, Point3::new(0.0, 0.0, distance));
        assert_eq!(front.1.dot(offset.1), 1.0);
        let translated_front = (front.0.translated(front.1, distance), front.1, front.2);
        let opposite = (Point3::new(0.0, 0.0, -distance), front.1, front.2);
        assert!(coplanar_plane_frames(translated_front, offset));
        assert!(!coplanar_plane_frames(opposite, offset));
    }
}

#[test]
fn front_datum_offset_matches_translated_front_sketch_plane() {
    front_offset_pair(6.0);
}

#[test]
fn negative_front_datum_offset_matches_opposite_side() {
    front_offset_pair(-6.0);
}

#[test]
fn serialized_support_offset_preserves_model_frame_and_polarity() {
    let native = |id: &str, source, ordinal| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "ReferencePlane".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal,
        name: id.into(),
        kind: String::new(),
        input_class: Some("moRefPlane_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    for distance in [-6.0, 6.0] {
        let front = native("sldprt:history:feature#0:front", 2, 0);
        let mut offset = native("sldprt:history:feature#0:offset", 7, 1);
        let top = native("sldprt:history:feature#0:top", 3, 2);
        let right = native("sldprt:history:feature#0:right", 4, 3);
        offset.parameters.insert(
            cadmpeg_core::nonblank_literal!("D1"),
            format!("{distance}mm"),
        );
        offset
            .properties
            .insert(cadmpeg_core::nonblank_literal!("Reference"), "3".into());
        offset.properties.insert(
            cadmpeg_core::nonblank_literal!("Origin"),
            format!("0mm,{distance}mm,0mm"),
        );
        offset
            .properties
            .insert(cadmpeg_core::nonblank_literal!("Normal"), "0,1,0".into());
        offset
            .properties
            .insert(cadmpeg_core::nonblank_literal!("UAxis"), "1,0,0".into());
        offset.properties.insert(
            cadmpeg_core::nonblank_literal!("ReferenceFaceOrigin"),
            "0mm,0mm,0mm".into(),
        );
        offset.properties.insert(
            cadmpeg_core::nonblank_literal!("ReferenceFaceNormal"),
            "0,1,0".into(),
        );
        offset.properties.insert(
            cadmpeg_core::nonblank_literal!("ReferenceFaceUAxis"),
            "1,0,0".into(),
        );
        let histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![front, offset, top, right],
        }];
        let features = project_features(&histories).expect("synthetic serialized support frame");
        assert!(matches!(
            features[1].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::ResolvedPlane { frame }),
                distance: actual,
            }) if actual.get() == distance
                && frame.origin().get() == Point3::new(0.0, 0.0, 0.0)
                && frame.normal().get() == Vector3::new(0.0, 1.0, 0.0)
                && frame.u_axis().get() == Vector3::new(1.0, 0.0, 0.0)
        ));
        assert!(features[1].dependencies.as_slice().is_empty());
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new().into(),
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
        };
        let frames = lane_sketch_plane_frames(&features, &histories, &lane);
        let result = frames
            .get(&7)
            .expect("resolved support offset frame")
            .as_tuple();
        let expected = (
            Point3::new(0.0, distance, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        assert_eq!(result, expected);
        assert!(coplanar_plane_frames(result, expected));
        let front = frames.get(&2).expect("Front frame").as_tuple();
        let front_offset = (front.0.translated(front.1, distance), front.1, front.2);
        assert!(!coplanar_plane_frames(result, front_offset));
        let mirrored = (Point3::new(0.0, -distance, 0.0), result.1, result.2);
        assert!(!coplanar_plane_frames(result, mirrored));
    }
}
