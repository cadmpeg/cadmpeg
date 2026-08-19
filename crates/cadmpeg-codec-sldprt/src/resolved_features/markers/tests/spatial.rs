//! Spatial-point marker and geometry-layout tests.
#![allow(unused_imports)]

use super::super::super::selections::coordinate_marker_local_links;
use super::super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::super::*;
use crate::layout::{
    compact_current_spatial_marker_point as compact_spatial,
    wide_spatial_marker_coordinate_prefix as wide_spatial,
};
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputScalar,
    FeatureInputScalarRole, SketchInputEntity, SketchInputKind, SketchRelationKind,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::SpatialSketchGeometry;
use std::collections::BTreeMap;

fn current_compact_spatial_point_marker(
    native_kind: u32,
    locus: [u8; 4],
    coordinates: [f64; 3],
) -> Vec<u8> {
    let mut marker = vec![0; 82];
    marker[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    marker[5..13].fill(0xff);
    marker[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    marker[17..21].copy_from_slice(&native_kind.to_le_bytes());
    marker[23..27].copy_from_slice(&locus);
    marker[27..29].copy_from_slice(&1u16.to_le_bytes());
    marker[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    marker[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    marker[56..58].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in coordinates.into_iter().enumerate() {
        let start = 58 + index * 8;
        marker[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    marker
}

#[test]
fn reference_cells_bind_reused_lane_local_tokens_to_their_declared_class() {
    let parent = "sldprt:feature-input:resolved-features#synthetic";
    let kind = FeatureInputOperandKind::Native(0x81d5);
    let reference = |offset| FeatureInputOperand {
        offset,
        reference_ref: format!("sldprt:feature-input:reference#synthetic:{offset}"),
        kind,
        entity_index: 0,
        entity_ref: None,
    };
    let scalars = [FeatureInputScalar {
        id: "scalar".into(),
        parent: parent.into(),
        feature_ref: None,
        ordinal: 0,
        offset: 100,
        object_id: 1,
        name: "name".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Driving,
        entity_indices: Vec::new(),
        operands: vec![reference(143), reference(287)],
    }];
    let classes = [FeatureInputClass {
        id: "sldprt:feature-input:class#synthetic:155".into(),
        parent: parent.into(),
        ordinal: 0,
        offset: 155,
        name: "sgEntHandle".into(),
        role: FeatureInputClassRole::SketchEntity,
    }];

    let references = reference_cells(&scalars, &classes);

    assert_eq!(references.len(), 2);
    assert!(references
        .iter()
        .all(|reference| { reference.class_ref.as_deref() == Some(classes[0].id.as_str()) }));

    let mut ambiguous_classes = classes.to_vec();
    ambiguous_classes.push(FeatureInputClass {
        id: "sldprt:feature-input:class#synthetic:299".into(),
        parent: parent.into(),
        ordinal: 1,
        offset: 299,
        name: "sgArcHandle".into(),
        role: FeatureInputClassRole::SketchEntity,
    });
    assert!(reference_cells(&scalars, &ambiguous_classes)
        .iter()
        .all(|reference| reference.class_ref.is_none()));
}

#[test]
fn current_spatial_point_marker_decodes_model_coordinates() {
    let mut payload = vec![0; 90];
    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
    payload[64] = 0x1e;
    assert_eq!(marker_spatial_coordinates(&payload, 0), None);
}

#[test]
fn legacy_spatial_point_marker_decodes_model_coordinates() {
    let offset = 4;
    let mut payload = vec![0; 94];
    payload[..offset].copy_from_slice(&1u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 64..offset + 66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = offset + 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
    payload[offset + 4] = 3;
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
}

#[test]
fn relation_backed_spatial_point_markers_decode_model_coordinates() {
    for (marker, sentinel, coordinates) in [
        (SKETCH_MARKER, 64, 66),
        (LEGACY_SKETCH_MARKER, 64, 66),
        (LEGACY_EXTENDED_SKETCH_MARKER, 56, 58),
    ] {
        let offset = 4;
        let mut payload = vec![0; offset + coordinates + 24];
        payload[..offset].copy_from_slice(&1u32.to_le_bytes());
        payload[offset..offset + marker.len()].copy_from_slice(marker);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 17..offset + 21].copy_from_slice(&3u32.to_le_bytes());
        payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + sentinel..offset + sentinel + 2].copy_from_slice(&[0x0e, 0x00]);
        for (index, value) in [-0.08_f64, 0.075, 0.0055].into_iter().enumerate() {
            let start = offset + coordinates + index * 8;
            payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }

        assert_eq!(
            marker_spatial_coordinates(&payload, offset),
            Some(Point3::new(-80.0, 75.0, 5.5))
        );
        if marker == SKETCH_MARKER {
            payload[offset + 17..offset + 21].copy_from_slice(&86u32.to_le_bytes());
            assert_eq!(marker_spatial_coordinates(&payload, offset), None);
            payload[offset + 17..offset + 21].copy_from_slice(&3u32.to_le_bytes());
            payload[offset + 56] = 1;
            assert_eq!(marker_spatial_coordinates(&payload, offset), None);
        }
    }
}

#[test]
fn packed_legacy_spatial_point_uses_compact_coordinate_offset() {
    let mut payload = vec![0; 74];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&0u32.to_le_bytes());
    payload[19..25].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[29] = 0x05;
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..50].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
        let start = 50 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(125.0, -250.0, 375.0))
    );
    payload[48] = 0x1e;
    assert_eq!(marker_spatial_coordinates(&payload, 0), None);
}

#[test]
fn current_spatial_point_variants_decode_model_coordinates() {
    for (kind, marker, coordinates, length) in [
        (
            0_u32,
            compact_spatial::COORDINATE_TAG,
            compact_spatial::COORDINATES,
            compact_spatial::LEN,
        ),
        (
            1_u32,
            wide_spatial::COORDINATE_TAG,
            wide_spatial::COORDINATES,
            wide_spatial::LEN,
        ),
    ] {
        let mut payload = vec![0; length];
        payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&kind.to_le_bytes());
        payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        payload[27..29].copy_from_slice(&1u16.to_le_bytes());
        payload[marker..marker + 2].copy_from_slice(&[0x0e, 0x00]);
        for (index, value) in [0.125_f64, -0.25, 0.375].into_iter().enumerate() {
            let start = coordinates + index * 8;
            payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }

        assert_eq!(
            marker_spatial_coordinates(&payload, 0),
            Some(Point3::new(125.0, -250.0, 375.0))
        );
    }
}

#[test]
fn current_compact_spatial_points_decode_without_object_indices() {
    for (native_kind, locus, coordinates) in [
        (0, [0x04, 0x00, 0x02, 0x00], [0.035, 0.0, 0.1415]),
        (1, [0x04, 0x00, 0x02, 0x00], [0.125, -0.25, 0.375]),
        (0, [0x05, 0x00, 0x01, 0x00], [-0.08, 0.075, 0.0055]),
    ] {
        let payload = current_compact_spatial_point_marker(native_kind, locus, coordinates);
        assert_eq!(
            marker_spatial_coordinates(&payload, 0),
            Some(Point3::new(
                coordinates[0] * 1000.0,
                coordinates[1] * 1000.0,
                coordinates[2] * 1000.0,
            ))
        );
        let entities = sketch_input_entities(&payload, "lane");
        let [entity] = entities.as_slice() else {
            panic!("expected one compact spatial point marker");
        };
        assert_eq!(entity.kind, SketchInputKind::Point);
        assert_eq!(entity.object_index, None);
    }

    let mut planar =
        current_compact_spatial_point_marker(1, [0x04, 0x00, 0x02, 0x00], [0.125, -0.25, 0.375]);
    planar[56..58].copy_from_slice(&[0x1e, 0x00]);
    assert_eq!(marker_spatial_coordinates(&planar, 0), None);
}

#[test]
fn compact_spatial_profile_points_project_and_ignore_unindexed_anchors() {
    let native_ref = "sldprt:history:feature#spatial";
    let lane_id = "sldprt:feature-input:resolved-features#spatial";
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.extend(current_compact_spatial_point_marker(
        0,
        [0x04, 0x00, 0x02, 0x00],
        [0.0, 0.015, 0.005],
    ));
    payload.extend([0xff; 4]);
    payload.extend(current_compact_spatial_point_marker(
        0,
        [0x05, 0x00, 0x01, 0x00],
        [0.0, 0.0, 0.0],
    ));
    payload.extend(3u32.to_le_bytes());
    payload.extend(current_compact_spatial_point_marker(
        0,
        [0x04, 0x00, 0x02, 0x00],
        [0.0, -0.015, 0.005],
    ));
    payload.extend([0xff; 4]);
    payload.extend(current_compact_spatial_point_marker(
        0,
        [0x05, 0x00, 0x01, 0x00],
        [0.0, 0.0, 0.0],
    ));
    let mut sketch_entities = sketch_input_entities(&payload, lane_id);
    assert_eq!(sketch_entities.len(), 4);
    assert_eq!(sketch_entities[0].object_index, Some(1));
    assert_eq!(sketch_entities[1].object_index, None);
    assert_eq!(sketch_entities[2].object_index, Some(3));
    assert_eq!(sketch_entities[3].object_index, None);
    for entity in &mut sketch_entities {
        entity.feature_ref = Some(native_ref.into());
        assert_eq!(entity.kind, SketchInputKind::Point);
    }

    let lane = FeatureInputLane {
        id: lane_id.into(),
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
        sketch_entities,
    };
    let history = FeatureHistory {
        id: "sldprt:history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![NativeFeature {
            id: native_ref.into(),
            parent: "sldprt:history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: Some("spatial".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "3D Sketch".into(),
            kind: "3D Sketch".into(),
            input_class: Some("mo3DProfileFeature_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    };
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: FeatureId("sldprt:model:feature#spatial".into()),
        ordinal: 0,
        name: Some("3D Sketch".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch { sketch: None },
        native_ref: Some(native_ref.into()),
    }];

    let (sketches, entities) = spatial_sketches(&mut features, &[history], &[lane]);

    assert_eq!(sketches.len(), 1);
    assert_eq!(entities.len(), 2);
    assert!(matches!(
        &entities[0].geometry,
        SpatialSketchGeometry::Point { position }
            if *position == Point3::new(0.0, 15.0, 5.0)
    ));
    assert!(matches!(
        &entities[1].geometry,
        SpatialSketchGeometry::Point { position }
            if *position == Point3::new(0.0, -15.0, 5.0)
    ));
    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::SpatialSketch { sketch: Some(sketch) }
            if sketch.0 == "sldprt:model:spatial-sketch#spatial"
    ));
}

#[test]
fn object_indexed_spatial_point_uses_compact_coordinates() {
    let offset = 4;
    let mut payload = vec![0; offset + 82];
    payload[..offset].copy_from_slice(&1u32.to_le_bytes());
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&5u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.035_f64, 0.0, 0.1415].into_iter().enumerate() {
        let start = offset + 58 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(35.0, 0.0, 141.5))
    );
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(35.0, 0.0, 141.5))
    );
    payload[..offset].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
    payload[..offset].copy_from_slice(&1u32.to_le_bytes());
    payload[offset + 58..offset + 66].copy_from_slice(&f64::from_bits(1).to_le_bytes());
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
}

#[test]
fn extended_spatial_point_marker_uses_compact_coordinate_offset() {
    let mut payload = vec![0; 82];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [-0.125_f64, 0.25, -0.375].into_iter().enumerate() {
        let start = 58 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(-125.0, 250.0, -375.0))
    );
}

#[test]
fn extended_kind_one_spatial_point_uses_wide_coordinate_offset() {
    let mut payload = vec![0; 90];
    payload[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [-0.125_f64, 0.25, -0.375].into_iter().enumerate() {
        let start = 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, 0),
        Some(Point3::new(-125.0, 250.0, -375.0))
    );
}

#[test]
fn extended_spatial_relation_handle_uses_wide_model_coordinates() {
    let offset = 4;
    let mut payload = vec![0; offset + 90];
    payload[..offset].copy_from_slice(&7u32.to_le_bytes());
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 17..offset + 21].copy_from_slice(&4u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 64..offset + 66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [0.0235_f64, 0.01, -0.075].into_iter().enumerate() {
        let start = offset + 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(23.5, 10.0, -75.0))
    );
    payload[offset + 56] = 1;
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
}

#[test]
fn extended_object_indexed_spatial_point_uses_wide_coordinate_offset() {
    let offset = 4;
    let mut payload = vec![0; offset + 90];
    payload[..offset].copy_from_slice(&1u32.to_le_bytes());
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 64..offset + 66].copy_from_slice(&[0x0e, 0x00]);
    for (index, value) in [-0.125_f64, 0.25, -0.375].into_iter().enumerate() {
        let start = offset + 66 + index * 8;
        payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        marker_spatial_coordinates(&payload, offset),
        Some(Point3::new(-125.0, 250.0, -375.0))
    );
    payload[..offset].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(marker_spatial_coordinates(&payload, offset), None);
}

#[test]
fn relation_binding_requires_family_operand_signature() {
    let class = FeatureInputClass {
        id: "class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 10,
        name: "sgLLDist".into(),
        role: FeatureInputClassRole::SketchConstraint,
    };
    let operand = |kind, entity_index| FeatureInputOperand {
        offset: 0,
        reference_ref: String::new(),
        kind,
        entity_index,
        entity_ref: None,
    };
    let scalar = |kind| FeatureInputScalar {
        id: "scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset: 20,
        object_id: 1,
        name: "name".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Driving,
        entity_indices: vec![0, 1],
        operands: vec![operand(kind, 0), operand(kind, 1)],
    };

    assert_eq!(
        relation_bindings(
            "lane",
            std::slice::from_ref(&class),
            &[scalar(FeatureInputOperandKind::E1)],
        )
        .len(),
        1
    );
    assert!(relation_bindings(
        "lane",
        &[class],
        &[scalar(FeatureInputOperandKind::Native(0x8dda))],
    )
    .is_empty());
}

#[test]
fn relation_binding_with_ambiguous_declarations_is_withheld() {
    let class = |offset: u64, name: &str| FeatureInputClass {
        id: format!("class-{offset}"),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        name: name.into(),
        role: FeatureInputClassRole::SketchConstraint,
    };
    let operand = |entity_index| FeatureInputOperand {
        offset: 0,
        reference_ref: String::new(),
        kind: FeatureInputOperandKind::Native(0x8152),
        entity_index,
        entity_ref: None,
    };
    let scalar = FeatureInputScalar {
        id: "scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset: 30,
        object_id: 1,
        name: "name".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Driving,
        entity_indices: vec![0, 1],
        operands: vec![operand(0), operand(1)],
    };

    assert!(relation_bindings(
        "lane",
        &[class(10, "sgPntPntDist"), class(20, "sgPntPntVertDist")],
        &[scalar],
    )
    .is_empty());
}

#[test]
fn scoped_relation_binding_does_not_cross_feature_interval() {
    let class = FeatureInputClass {
        id: "class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 10,
        name: "sgPntPntDist".into(),
        role: FeatureInputClassRole::SketchConstraint,
    };
    let operand = |entity_index| FeatureInputOperand {
        offset: 0,
        reference_ref: String::new(),
        kind: FeatureInputOperandKind::Native(0x8152),
        entity_index,
        entity_ref: None,
    };
    let scalar = FeatureInputScalar {
        id: "scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("second".into()),
        ordinal: 0,
        offset: 120,
        object_id: 1,
        name: "name".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Driving,
        entity_indices: vec![0, 1],
        operands: vec![operand(0), operand(1)],
    };

    assert!(relation_bindings_scoped(
        "lane",
        &[class],
        &[scalar],
        &[(0, 100, "first".into()), (100, u64::MAX, "second".into())],
    )
    .is_empty());
}

#[test]
fn marker_local_id_is_the_trailing_u32() {
    let mut payload = vec![0; 92];
    payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[88..92].copy_from_slice(&37u32.to_le_bytes());
    assert_eq!(marker_local_id(&payload, 0), Some(37));
    payload[88..92].fill(0xff);
    assert_eq!(marker_local_id(&payload, 0), None);
}

#[test]
fn marker_object_index_precedes_the_marker() {
    let mut payload = 37u32.to_le_bytes().to_vec();
    payload.extend(SKETCH_MARKER);
    assert_eq!(marker_object_index(&payload, 4), Some(37));
    assert_eq!(marker_object_index(&payload, 3), None);
    payload[0..4].fill(0xff);
    assert_eq!(marker_object_index(&payload, 4), None);
}

#[test]
fn coordinate_marker_local_id_uses_the_variant_footer() {
    let mut payload = vec![0; 142 + 5];
    payload[..5].copy_from_slice(SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[138..142].copy_from_slice(&41u32.to_le_bytes());
    payload[142..147].copy_from_slice(SKETCH_MARKER);
    assert_eq!(marker_local_id(&payload, 0), Some(41));
}

#[test]
fn coordinate_less_geometry_locus_uses_the_variant_footer() {
    let mut payload = vec![0; 142 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[138..142].copy_from_slice(&41u32.to_le_bytes());
    payload[142..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(marker_local_id(&payload, 0), Some(41));
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(marker_local_id(&payload, 0), None);
}

#[test]
fn legacy_sketch_prefix_uses_the_shared_entity_body() {
    let mut payload = vec![0; 142 + LEGACY_SKETCH_MARKER.len()];
    payload[..5].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
    payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[138..142].copy_from_slice(&41u32.to_le_bytes());
    payload[142..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entities = sketch_input_entities(&payload, "lane");

    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].coordinates_m, Some([1.25, -2.5]));
    assert_eq!(entities[0].local_id, Some(41));
}

#[test]
fn terminal_wide_geometry_locus_coordinate_record_is_a_point() {
    for (prefix, code) in [
        (SKETCH_MARKER, 2u32),
        (LEGACY_SKETCH_MARKER, 1),
        (LEGACY_EXTENDED_SKETCH_MARKER, 2),
    ] {
        let mut payload = vec![0; 142 + prefix.len()];
        payload[..prefix.len()].copy_from_slice(prefix);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&code.to_le_bytes());
        payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[66..74].copy_from_slice(&0.025f64.to_le_bytes());
        payload[74..82].copy_from_slice(&(-0.004f64).to_le_bytes());
        payload[92..96].copy_from_slice(&(-2i32).to_le_bytes());
        payload[138..142].copy_from_slice(&7u32.to_le_bytes());
        payload[142..].copy_from_slice(prefix);

        let entities = sketch_input_entities(&payload, "lane");
        let [entity] = entities.as_slice() else {
            panic!("expected one marker entity");
        };
        assert_eq!(entity.kind, SketchInputKind::Point);
        assert_eq!(entity.coordinates_m, Some([0.025, -0.004]));

        payload[134..138].copy_from_slice(&6u32.to_le_bytes());
        assert_eq!(
            sketch_input_entities(&payload, "lane")[0].kind,
            SketchInputKind::Point
        );
        payload[133] = 1;
        assert!(!super::terminal_wide_geometry_locus_profile_vertex(
            &payload, 0
        ));
    }
}

#[test]
fn compact_legacy_profile_coordinate_pairings_carry_points() {
    let mut payload = vec![0; 120 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&0u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x05, 0x00, 0x01, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&[0x1e, 0x00]);
    payload[44..52].copy_from_slice(&0.025f64.to_le_bytes());
    payload[52..60].copy_from_slice(&(-0.004f64).to_le_bytes());
    payload[120..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(marker_coordinates(&payload, 0), Some([0.025, -0.004]));
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].kind, SketchInputKind::Point);

    payload[19..23].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(marker_coordinates(&payload, 0), Some([0.025, -0.004]));
    assert_eq!(
        sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );

    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[19..23].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(marker_coordinates(&payload, 0), None);
}

#[test]
fn packed_legacy_geometry_locus_carries_profile_coordinates() {
    let mut payload = vec![0; 126 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&0u32.to_le_bytes());
    payload[17..23].copy_from_slice(&[0x00, 0x00, 0x05, 0x00, 0x01, 0x00]);
    payload[23..25].copy_from_slice(&1u16.to_le_bytes());
    payload[29] = 0x04;
    payload[40..48].copy_from_slice(&1.0f64.to_le_bytes());
    payload[48..50].copy_from_slice(&[0x1e, 0x00]);
    payload[50..58].copy_from_slice(&0.025f64.to_le_bytes());
    payload[58..66].copy_from_slice(&(-0.004f64).to_le_bytes());
    payload[126..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(marker_coordinates(&payload, 0), Some([0.025, -0.004]));
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].kind, SketchInputKind::Point);
    assert_eq!(entities[0].coordinates_m, Some([0.025, -0.004]));
    assert_eq!(entities[0].state_value, Some(1.0));
}

#[test]
fn compact_profile_curve_role_distinguishes_non_coordinate_lines() {
    let mut payload = vec![0; 92 + LEGACY_SKETCH_MARKER.len()];
    payload[..5].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[27..29].copy_from_slice(&2u16.to_le_bytes());
    payload[64..66].copy_from_slice(&0u16.to_le_bytes());
    payload[66..68].copy_from_slice(&1u16.to_le_bytes());
    payload[92..].copy_from_slice(LEGACY_SKETCH_MARKER);

    let entities = sketch_input_entities(&payload, "lane");

    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].coordinates_m, None);
    assert_eq!(entities[0].kind, SketchInputKind::LineOrCircle);
}

#[test]
fn embedded_class_header_is_not_a_sketch_entity() {
    let mut payload = vec![0; 64];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    payload[31..35].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[35..39].copy_from_slice(CLASS_MARKER);

    assert!(!super::sketch_marker_at(&payload, 0));
    assert!(sketch_input_entities(&payload, "lane").is_empty());
}

#[test]
fn geometry_marker_coordinates_are_selected_by_layout() {
    let mut payload = vec![0; 82];
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&10u32.to_le_bytes());
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
    payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    payload[64..66].copy_from_slice(&[0x14, 0x00]);
    assert_eq!(marker_coordinates(&payload, 0), None);
    payload[64..66].copy_from_slice(&[0x1e, 0x00]);
    payload[5] = 0;
    assert_eq!(marker_coordinates(&payload, 0), None);
}

#[test]
fn legacy_geometry_marker_coordinates_use_the_compact_body_offsets() {
    let mut payload = vec![0; 74];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[56..58].copy_from_slice(&[0x1e, 0x00]);
    payload[58..66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[66..74].copy_from_slice(&(-2.5f64).to_le_bytes());

    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(entities[0].kind, SketchInputKind::LineOrCircle);

    payload[..SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[17..21].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());

    payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    assert_eq!(marker_coordinates(&payload, 0), None);
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(
        entities[0].kind,
        SketchInputKind::Relation(SketchRelationKind::Distance)
    );

    payload[17..21].copy_from_slice(&4u32.to_le_bytes());
    payload[27..29].copy_from_slice(&1u16.to_le_bytes());
    let entities = sketch_input_entities(&payload, "lane");
    assert_eq!(
        entities[0].kind,
        SketchInputKind::Relation(SketchRelationKind::Horizontal)
    );

    payload.resize(154 + LEGACY_SKETCH_MARKER.len(), 0);
    payload[154..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));

    for size in [161, 162] {
        payload.resize(size + LEGACY_SKETCH_MARKER.len(), 0);
        payload[size..].copy_from_slice(LEGACY_SKETCH_MARKER);
        assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    }
}

#[test]
fn compact_legacy_coordinate_value_one_is_a_profile_vertex() {
    let mut payload = vec![0; 68];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&1u32.to_le_bytes());
    payload[17..25].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[25..27].copy_from_slice(&1u16.to_le_bytes());
    payload[31] = 0x04;
    payload[42..44].copy_from_slice(&[0x1e, 0x00]);
    payload[44..52].copy_from_slice(&1.25f64.to_le_bytes());
    payload[52..60].copy_from_slice(&(-2.5f64).to_le_bytes());

    assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
    assert!(compact_legacy_profile_vertex(&payload, 0));
    let entities = sketch_input_entities(&payload, "lane");
    let [entity] = entities.as_slice() else {
        panic!("expected one compact marker");
    };
    assert_eq!(entity.kind, SketchInputKind::Point);
}

#[test]
fn extended_geometry_values_share_the_coordinate_record_layout() {
    let offset = 4;
    for size in [134, 138, 140, 144] {
        let mut payload = vec![0; offset + size + LEGACY_EXTENDED_SKETCH_MARKER.len()];
        payload[..offset].copy_from_slice(&7u32.to_le_bytes());
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
        payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
        payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
        payload[offset + size..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

        for native_code in 0u32..=2 {
            payload[offset + 17..offset + 21].copy_from_slice(&native_code.to_le_bytes());
            assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
        }
    }
}

#[test]
fn linked_profile_point_carries_coordinates_for_compact_and_long_tails() {
    let offset = 4;
    let mut payload = vec![0; offset + 154 + SKETCH_MARKER.len()];
    payload[..offset].copy_from_slice(&7u32.to_le_bytes());
    payload[offset + 5..offset + 13].fill(0xff);
    payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[offset + 23..offset + 29].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00]);
    payload[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    payload[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    payload[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    payload[offset + 76..offset + 78].copy_from_slice(&2u16.to_le_bytes());
    for (start, id) in [(78, 2u16), (90, 3u16)] {
        payload[offset + start..offset + start + 2].copy_from_slice(&0x8178u16.to_le_bytes());
        payload[offset + start + 2..offset + start + 4].copy_from_slice(&id.to_le_bytes());
        payload[offset + start + 4..offset + start + 8].fill(0xff);
    }
    payload[offset + 102..offset + 108].copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    for prefix in [SKETCH_MARKER, LEGACY_EXTENDED_SKETCH_MARKER] {
        payload[offset..offset + prefix.len()].copy_from_slice(prefix);
        payload[offset + 154..offset + 154 + prefix.len()].copy_from_slice(prefix);

        assert_eq!(
            linked_profile_point(&payload, offset),
            Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
        );
        assert_eq!(marker_coordinates(&payload, offset), Some([1.25, -2.5]));
        let entities = super::sketch_input_entities(&payload, "lane");
        let point = entities
            .iter()
            .find(|entity| entity.offset == offset as u64)
            .expect("linked profile point");
        assert_eq!(point.kind, SketchInputKind::Point);
        assert_eq!(point.coordinates_m, Some([1.25, -2.5]));
    }
    payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[offset + 154..offset + 154 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        linked_profile_point(&payload, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    assert_eq!(
        super::sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[offset + 154..offset + 154 + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 17..offset + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[offset + 92..offset + 94].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(
        linked_profile_point(&payload, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 4)]))
    );
    assert_eq!(
        super::sketch_input_entities(&payload, "lane")[0].kind,
        SketchInputKind::Point
    );
    payload[offset + 17..offset + 21].fill(0);
    payload[offset + 92..offset + 94].copy_from_slice(&3u16.to_le_bytes());

    payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[offset + 74..offset + 78].copy_from_slice(&[0x01, 0x00, 0x03, 0x00]);
    assert_eq!(
        additional_linked_profile_point_coordinates(&payload, offset),
        Some([1.25, -2.5])
    );
    assert_eq!(linked_profile_point(&payload, offset), None);
    payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[offset + 74..offset + 78].copy_from_slice(&[0x00, 0x00, 0x02, 0x00]);
    assert_eq!(
        additional_linked_profile_point_coordinates(&payload, offset),
        Some([1.25, -2.5])
    );
    assert_eq!(linked_profile_point(&payload, offset), None);
    payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);

    let mut extended = vec![0; offset + 158 + LEGACY_EXTENDED_SKETCH_MARKER.len()];
    extended[..offset + 108].copy_from_slice(&payload[..offset + 108]);
    extended[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    extended[offset + 144..offset + 148].copy_from_slice(&3u32.to_le_bytes());
    extended[offset + 148..offset + 152].copy_from_slice(&2u32.to_le_bytes());
    extended[offset + 154..offset + 158].copy_from_slice(&1u32.to_le_bytes());
    extended[offset + 158..].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);

    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    assert_eq!(marker_coordinates(&extended, offset), Some([1.25, -2.5]));
    let entities = super::sketch_input_entities(&extended, "lane");
    let point = entities
        .iter()
        .find(|entity| entity.offset == offset as u64)
        .expect("extended-tail linked profile point");
    assert_eq!(point.kind, SketchInputKind::Point);
    assert_eq!(point.coordinates_m, Some([1.25, -2.5]));

    extended[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    extended[offset + 158..].copy_from_slice(SKETCH_MARKER);
    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    assert_eq!(
        super::sketch_input_entities(&extended, "lane")[0].kind,
        SketchInputKind::Point
    );

    extended[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    extended[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    extended[offset + 154..offset + 158].fill(0xff);
    extended[offset + 158..].copy_from_slice(LEGACY_SKETCH_MARKER);
    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    assert_eq!(
        super::sketch_input_entities(&extended, "lane")[0].kind,
        SketchInputKind::Point
    );
    extended[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    extended[offset + 17..offset + 21].fill(0);
    assert_eq!(linked_profile_point(&extended, offset), None);
    extended[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    extended[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    extended[offset + 76..offset + 78].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        linked_profile_point(&extended, offset),
        Some(([1.25, -2.5], [(0x8178, 2), (0x8178, 3)]))
    );
    extended[offset + 144..offset + 148].fill(0);
    assert_eq!(linked_profile_point(&extended, offset), None);

    let mut legacy_geometry = vec![0; offset + 154 + LEGACY_SKETCH_MARKER.len()];
    legacy_geometry[..offset].copy_from_slice(&7u32.to_le_bytes());
    legacy_geometry[offset..offset + LEGACY_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_SKETCH_MARKER);
    legacy_geometry[offset + 5..offset + 13].fill(0xff);
    legacy_geometry[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    legacy_geometry[offset + 17..offset + 21].copy_from_slice(&1u32.to_le_bytes());
    legacy_geometry[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    legacy_geometry[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
    legacy_geometry[offset + 31..offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    legacy_geometry[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    legacy_geometry[offset + 56..offset + 58].copy_from_slice(&[0x1e, 0x00]);
    legacy_geometry[offset + 58..offset + 66].copy_from_slice(&1.25f64.to_le_bytes());
    legacy_geometry[offset + 66..offset + 74].copy_from_slice(&(-2.5f64).to_le_bytes());
    legacy_geometry[offset + 76..offset + 78].copy_from_slice(&2u16.to_le_bytes());
    for (start, local_id) in [(78, 1u16), (90, 0)] {
        legacy_geometry[offset + start..offset + start + 2]
            .copy_from_slice(&0x8139u16.to_le_bytes());
        legacy_geometry[offset + start + 2..offset + start + 4]
            .copy_from_slice(&local_id.to_le_bytes());
        legacy_geometry[offset + start + 4..offset + start + 8].fill(0xff);
    }
    legacy_geometry[offset + 102..offset + 108]
        .copy_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
    legacy_geometry[offset + 150..offset + 154].copy_from_slice(&11u32.to_le_bytes());
    legacy_geometry[offset + 154..].copy_from_slice(LEGACY_SKETCH_MARKER);

    assert_eq!(
        linked_profile_point(&legacy_geometry, offset),
        Some(([1.25, -2.5], [(0x8139, 1), (0x8139, 0)]))
    );
    assert_eq!(
        coordinate_marker_local_links(&legacy_geometry, offset),
        Some((vec![1, 0], 0x8139))
    );
    assert_eq!(
        marker_coordinates(&legacy_geometry, offset),
        Some([1.25, -2.5])
    );
    let entity = super::sketch_input_entities(&legacy_geometry, "lane")
        .into_iter()
        .find(|entity| entity.offset == offset as u64)
        .expect("legacy geometry linked profile point");
    assert_eq!(entity.kind, SketchInputKind::Point);
    assert_eq!(entity.coordinates_m, Some([1.25, -2.5]));
}

#[test]
fn spatial_vertex_record_decodes_model_coordinates() {
    let mut payload = vec![0x55; 7];
    payload.extend([
        0xff, 0xfe, 0xff, 0x06, b'V', 0x00, b'e', 0x00, b'r', 0x00, b't', 0x00, b'e', 0x00, b'x',
        0x00,
    ]);
    payload.extend([0x00; 27]);
    payload.extend([0x0e, 0x00]);
    for value in [1.25f64, -2.5, 3.75] {
        payload.extend(value.to_le_bytes());
    }
    assert_eq!(
        crate::resolved_features::markers::spatial_vertex_coordinates(&payload),
        vec![cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)]
    );
    payload[7 + 43] = 0x1e;
    assert!(crate::resolved_features::markers::spatial_vertex_coordinates(&payload).is_empty());
}
