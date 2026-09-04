//! Hole position-sketch and spatial-locus tests.

use super::{cylinder, lane, lane_with_position_reference, model_hole, native_history};
use std::collections::BTreeMap;

use cadmpeg_ir::features::{FeatureDefinition, FeatureId, HolePlacement};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SpatialSketch,
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchId,
};

use super::super::*;
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputName, FeatureInputRelationFamily,
    FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
    SketchRelationKind,
};

#[test]
fn hole_position_carrier_presence_requires_a_serialized_position_source() {
    let history = native_history();
    let hole = model_hole();

    assert!(!hole_position_carrier_present(
        &hole,
        std::slice::from_ref(&history),
        &[lane()]
    ));
    assert!(hole_position_carrier_present(
        &hole,
        &[history],
        &[lane_with_position_reference(12)],
    ));
}

#[test]
fn compact_position_graph_selects_the_unique_bore_loci() {
    use FeatureInputRelationFamily::{
        PointPointDistance, PointPointHorizontalDistance, PointPointVerticalDistance,
    };

    let loci = [
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 16.0),
        Point2::new(0.0, 41.0),
    ];
    let relations = [
        (PointPointDistance, 0, 2, 25.0),
        (PointPointVerticalDistance, 0, 5, 0.0),
        (PointPointHorizontalDistance, 0, 5, 16.0),
    ];
    let placement_loci = [1, 2].into_iter().collect();
    assert_eq!(
        compact_position_loci(&loci, &placement_loci, &relations),
        Some(vec![1, 2])
    );

    let ambiguous = [loci[0], loci[1], loci[2], Point2::new(0.0, -9.0)];
    let ambiguous_placements = [1, 2, 3].into_iter().collect();
    assert_eq!(
        compact_position_loci(&ambiguous, &ambiguous_placements, &relations),
        None
    );
}

#[test]
fn object_indexed_curve_markers_select_a_congruent_bore_pattern() {
    let mut lane = lane();
    lane.sketch_entities = [(1, [0.013, 0.007]), (2, [-0.009, 0.007])]
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (object_index, coordinates_m))| SketchInputEntity {
                id: format!("marker-{ordinal}"),
                parent: "lane".into(),
                feature_ref: Some("position".into()),
                ordinal: ordinal as u32,
                offset: ordinal as u64,
                object_index: Some(object_index),
                local_id: None,
                kind: SketchInputKind::LineOrCircle,
                state_value: Some(1.0),
                coordinates_m: Some(coordinates_m),
                links: Vec::new(),
                link_selector: None,
            },
        )
        .collect();
    let surface = |id, x| Surface {
        id: SurfaceId(format!("surface-{id}")),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(x, 7.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.1,
        },
        source_object: None,
    };
    let mut surfaces = vec![surface(0, -9.0), surface(1, 13.0), surface(2, 100.0)];

    let placements = marker_pattern_bore_axes(&lane, "position", 2.1, &surfaces, None)
        .expect("required invariant");
    assert_eq!(placements.len(), 2);
    assert!(placements.iter().any(|placement| matches!(
        placement,
        cadmpeg_ir::features::HolePlacement::Axis { origin, .. }
            if origin.x == -9.0 && origin.y == 7.0 && origin.z == 10.0
    )));
    assert!(placements.iter().any(|placement| matches!(
        placement,
        cadmpeg_ir::features::HolePlacement::Axis { origin, .. }
            if origin.x == 13.0 && origin.y == 7.0 && origin.z == 10.0
    )));

    for marker in &mut lane.sketch_entities {
        marker.kind = SketchInputKind::Arc;
    }
    lane.sketch_entities.extend([
        SketchInputEntity {
            id: "auxiliary-object-locus".into(),
            parent: "lane".into(),
            feature_ref: Some("position".into()),
            ordinal: 2,
            offset: 2,
            object_index: Some(3),
            local_id: None,
            kind: SketchInputKind::Point,
            state_value: Some(1.0),
            coordinates_m: Some([1.0, 1.0]),
            links: Vec::new(),
            link_selector: None,
        },
        SketchInputEntity {
            id: "auxiliary-anchor".into(),
            parent: "lane".into(),
            feature_ref: Some("position".into()),
            ordinal: 3,
            offset: 3,
            object_index: None,
            local_id: None,
            kind: SketchInputKind::Point,
            state_value: Some(1.0),
            coordinates_m: Some([0.0, 0.0]),
            links: Vec::new(),
            link_selector: None,
        },
    ]);
    assert_eq!(
        marker_pattern_bore_axes(&lane, "position", 2.1, &surfaces, None)
            .expect("object-indexed arc centers form the exact position roster")
            .len(),
        2
    );

    let opposite_side = |id, x| Surface {
        id: SurfaceId(format!("surface-{id}")),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(x, 30.0, 10.0),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.1,
        },
        source_object: None,
    };
    surfaces.extend([opposite_side(3, -9.0), opposite_side(4, 13.0)]);
    assert!(marker_pattern_bore_axes(&lane, "position", 2.1, &surfaces, None).is_none());
    assert_eq!(
        marker_pattern_bore_axes(
            &lane,
            "position",
            2.1,
            &surfaces,
            Some(Vector3::new(0.0, 0.0, 1.0)),
        )
        .expect("required invariant")
        .len(),
        2
    );

    let mut opposite = surface(5, -9.0);
    let SurfaceGeometry::Cylinder { axis, .. } = &mut opposite.geometry else {
        unreachable!();
    };
    *axis = Vector3::new(0.0, 0.0, -1.0);
    surfaces.push(opposite);
    assert_eq!(
        marker_pattern_bore_axes(
            &lane,
            "position",
            2.1,
            &surfaces,
            Some(Vector3::new(0.0, 0.0, 1.0)),
        )
        .expect("required invariant")
        .len(),
        2
    );
}

#[test]
fn curve_markers_can_contain_unmatched_construction_loci() {
    let mut lane = lane();
    lane.sketch_entities = [[-0.07, 0.011], [0.07, 0.011], [0.0, -0.004], [0.0, 0.011]]
        .into_iter()
        .enumerate()
        .map(|(ordinal, coordinates_m)| SketchInputEntity {
            id: format!("curve-marker-{ordinal}"),
            parent: "lane".into(),
            feature_ref: Some("position".into()),
            ordinal: ordinal as u32,
            offset: ordinal as u64,
            object_index: Some((ordinal + 1) as u32),
            local_id: None,
            kind: SketchInputKind::Arc,
            state_value: Some(1.0),
            coordinates_m: Some(coordinates_m),
            links: Vec::new(),
            link_selector: None,
        })
        .collect();
    let surfaces = [-70.0, 70.0]
        .into_iter()
        .enumerate()
        .map(|(id, x)| Surface {
            id: SurfaceId(format!("carrier-{id}")),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(x, 11.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 3.0,
            },
            source_object: None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        marker_pattern_bore_axes(&lane, "position", 3.0, &surfaces, None),
        Some(vec![
            HolePlacement::Axis {
                origin: Point3::new(-70.0, 11.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            HolePlacement::Axis {
                origin: Point3::new(70.0, 11.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
        ])
    );
}

#[test]
fn paired_object_loci_select_a_congruent_bore_pattern() {
    let marker = |id: &str, ordinal, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("position".into()),
        ordinal,
        offset: u64::from(ordinal) * 10,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = lane();
    lane.sketch_entities = vec![
        marker(
            "first",
            0,
            Some(1),
            SketchInputKind::Arc,
            Some([0.013, 0.0]),
        ),
        marker(
            "first-origin",
            1,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "second",
            2,
            Some(2),
            SketchInputKind::Relation(SketchRelationKind::Horizontal),
            Some([-0.009, 0.0]),
        ),
        marker(
            "second-origin",
            3,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "auxiliary",
            4,
            Some(3),
            SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
        marker(
            "paired-duplicate",
            5,
            Some(4),
            SketchInputKind::Point,
            Some([1.0, 1.0]),
        ),
        marker(
            "paired-duplicate-origin",
            6,
            None,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
    ];

    let paired = paired_object_locus_markers(&lane, "position")
        .into_iter()
        .map(|marker| marker.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paired, ["first", "second", "paired-duplicate"]);

    let mut surfaces = vec![cylinder(0, -9.0), cylinder(1, 13.0), cylinder(2, 100.0)];
    let placements = marker_pattern_bore_axes(&lane, "position", 2.0, &surfaces, None)
        .expect("unique congruent pattern");
    assert_eq!(placements.len(), 2);

    let opposite = surfaces[..2]
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, mut surface)| {
            surface.id = SurfaceId(format!("opposite-{index}"));
            let SurfaceGeometry::Cylinder { origin, axis, .. } = &mut surface.geometry else {
                unreachable!();
            };
            origin.z = 20.0;
            *axis = Vector3::new(0.0, 0.0, -1.0);
            surface
        })
        .collect::<Vec<_>>();
    surfaces.extend(opposite);
    assert_eq!(
        marker_pattern_bore_axes(&lane, "position", 2.0, &surfaces, None)
            .expect("unoriented coincident axes")
            .len(),
        2
    );

    surfaces.push(Surface {
        id: SurfaceId("duplicate-locus-bore".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(1000.0, 1000.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    });
    assert_eq!(
        marker_pattern_bore_axes(&lane, "position", 2.0, &surfaces, None)
            .expect("complete paired roster takes precedence")
            .len(),
        3
    );
}

#[test]
fn hole_temporary_axis_decodes_depth_point_direction_layout() {
    let mut payload = vec![0; 500];
    let declaration = 40;
    payload[declaration..declaration + 4].copy_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    payload[declaration + 4..declaration + 6].copy_from_slice(&15u16.to_le_bytes());
    payload[declaration + 6..declaration + 21].copy_from_slice(b"moTempAxisRef_w");
    payload[declaration + 267..declaration + 275]
        .copy_from_slice(b"\xc7\xcf\xff\xff\xc7\xcf\xff\xff");
    payload[declaration + 279..declaration + 283].copy_from_slice(&4700u32.to_le_bytes());
    for (index, value) in [0.0075, -0.045, 0.028, -0.03, -1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = declaration + 299 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[declaration + 364..declaration + 368].copy_from_slice(&[0xff, 0xfe, 0xff, 0x00]);

    assert_eq!(
        hole_temporary_axis(&payload, 32, payload.len()),
        Some((
            Point3::new(-45.0, 28.0, -30.0),
            Vector3::new(-1.0, 0.0, 0.0),
        ))
    );
}

#[test]
fn embedded_position_sketch_name_resolves_its_typed_source() {
    let history = native_history();
    let mut lane = lane();
    lane.native_payload.resize(200, 0);
    lane.names.push(FeatureInputName {
        id: "hole-name".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        value: "Hole".into(),
        object_id: Some(7),
    });
    let hole_trailer = 6 + "Hole".encode_utf16().count() * 2;
    lane.native_payload[hole_trailer..hole_trailer + 8]
        .copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
    lane.native_payload[hole_trailer + 8..hole_trailer + 12].copy_from_slice(&7u32.to_le_bytes());

    let child_offset = hole_trailer + 32;
    lane.names.push(FeatureInputName {
        id: "position-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: child_offset as u64,
        value: "Position".into(),
        object_id: Some(6),
    });
    let child_trailer = child_offset + 6 + "Position".encode_utf16().count() * 2;
    lane.native_payload[child_trailer..child_trailer + 8]
        .copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
    lane.native_payload[child_trailer + 8..child_trailer + 12].copy_from_slice(&6u32.to_le_bytes());

    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        Some(6)
    );
    let mut classless_history = history.clone();
    classless_history.features[0].input_class = None;
    assert_eq!(
        hole_position_sketch_source(&classless_history.features[0], &lane),
        Some(6)
    );
    lane.native_payload[hole_trailer + 16..hole_trailer + 18].copy_from_slice(&[0, 0xc0]);
    lane.native_payload[hole_trailer + 18..hole_trailer + 22].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        None
    );
    lane.native_payload[hole_trailer + 16..hole_trailer + 28].fill(0);
    lane.native_payload[child_trailer + 8] = 5;
    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        None
    );

    let mut legacy_history = history.clone();
    legacy_history.features[0].source_id = None;
    legacy_history.features.push(crate::records::Feature {
        id: "native-position".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    lane.native_payload[hole_trailer + 16..hole_trailer + 28].fill(0);
    lane.native_payload[hole_trailer + 16..hole_trailer + 28]
        .copy_from_slice(&[0, 0xc0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        hole_position_feature(
            &legacy_history.features[0],
            std::slice::from_ref(&legacy_history),
            &[lane],
        )
        .map(|feature| feature.id.as_str()),
        Some("native-position")
    );
}

#[test]
fn typed_position_sketch_reference_lifts_authored_object_loci() {
    let hole = model_hole();
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("position-sketch".into()),
        ordinal: 1,
        name: Some("Position".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(SketchId("position-geometry".into())),
        },
        native_ref: Some("native-position-sketch".into()),
    };
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-position-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("6".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    let mut lane = lane_with_position_reference(6);
    let trailer = 6 + "Hole".encode_utf16().count() * 2;
    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        Some(6)
    );
    lane.native_payload[trailer + 58..trailer + 60].copy_from_slice(&[0xff, 0xfe]);
    assert_eq!(
        hole_position_sketch_source(&history.features[0], &lane),
        Some(6)
    );
    lane.sketch_entities.push(SketchInputEntity {
        id: "authored-point".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 0,
        offset: 80,
        object_index: Some(1),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some([0.002, 0.003]),
        links: Vec::new(),
        link_selector: None,
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "origin-marker".into(),
        object_index: None,
        ordinal: 1,
        offset: 90,
        coordinates_m: Some([0.0, 0.0]),
        ..lane.sketch_entities[0].clone()
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "point-identity".into(),
        object_index: Some(2),
        ordinal: 4,
        offset: 120,
        coordinates_m: None,
        ..lane.sketch_entities[0].clone()
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "authored-arc-locus".into(),
        object_index: Some(2),
        ordinal: 2,
        offset: 100,
        kind: SketchInputKind::Arc,
        coordinates_m: Some([0.014, 0.025]),
        ..lane.sketch_entities[0].clone()
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "arc-origin-marker".into(),
        object_index: None,
        ordinal: 3,
        offset: 110,
        kind: SketchInputKind::Point,
        coordinates_m: Some([0.0, 0.0]),
        ..lane.sketch_entities[0].clone()
    });
    lane.sketch_entities.sort_by_key(|marker| marker.ordinal);
    let sketch = Sketch {
        id: SketchId("position-geometry".into()),
        name: Some("Position".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 20.0, 30.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    };
    let entities = [SketchEntity::new(
        SketchEntityId("point".into()),
        sketch.id.clone(),
        SketchGeometry::Point {
            position: Point2::new(2.0, 3.0),
        },
    )
    .with_native_ref(Some("authored-point".into()))];
    let mut features = vec![hole, sketch_feature];
    let mut paired_lane = lane.clone();
    paired_lane.sketch_entities.truncate(4);
    paired_lane.sketch_entities[0].kind = SketchInputKind::Arc;
    paired_lane.sketch_entities[0].coordinates_m = Some([0.012, 0.023]);
    let mut alternate_configuration = lane.clone();
    alternate_configuration.id = "alternate-lane".into();
    alternate_configuration.configuration = Some("alternate".into());

    project_hole_position_sketches(
        &mut features,
        std::slice::from_ref(&sketch),
        &entities,
        std::slice::from_ref(&history),
        &[lane, alternate_configuration],
    );

    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        panic!("expected hole");
    };
    let placements = placements.as_deref().expect("resolved placements");
    assert_eq!(placements.len(), 1);
    assert!(matches!(
        placements[0],
        cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3 {
                x: 12.0,
                y: 23.0,
                z: 30.0
            },
            axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));
    assert_eq!(
        features[0].dependencies,
        [FeatureId("position-sketch".into())]
    );

    let mut paired_features = vec![model_hole(), features[1].clone()];
    project_hole_position_sketches(
        &mut paired_features,
        std::slice::from_ref(&sketch),
        &[],
        std::slice::from_ref(&history),
        std::slice::from_ref(&paired_lane),
    );
    let FeatureDefinition::Hole {
        placements: paired_placements,
        ..
    } = &paired_features[0].definition
    else {
        panic!("expected hole");
    };
    let paired_placements = paired_placements
        .as_deref()
        .expect("resolved paired placements");
    assert_eq!(paired_placements.len(), 2);
    assert!(matches!(
        paired_placements[0],
        cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3 {
                x: 12.0,
                y: 23.0,
                z: 30.0
            },
            axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));
    assert!(matches!(
        paired_placements[1],
        cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3 {
                x: 14.0,
                y: 25.0,
                z: 30.0
            },
            axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));

    let mut incomplete_lane = paired_lane;
    incomplete_lane.sketch_entities.push(SketchInputEntity {
        id: "unpaired-object-locus".into(),
        object_index: Some(3),
        ordinal: 4,
        offset: 120,
        kind: SketchInputKind::Arc,
        coordinates_m: Some([0.016, 0.027]),
        ..incomplete_lane.sketch_entities[0].clone()
    });
    let mut incomplete_features = vec![model_hole(), features[1].clone()];
    project_hole_position_sketches(
        &mut incomplete_features,
        std::slice::from_ref(&sketch),
        &[],
        std::slice::from_ref(&history),
        std::slice::from_ref(&incomplete_lane),
    );
    let FeatureDefinition::Hole { placements, .. } = &incomplete_features[0].definition else {
        panic!("expected hole");
    };
    assert!(placements.is_none());
}

#[test]
fn unique_unindexed_point_locus_is_projected() {
    let hole = model_hole();
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("position-sketch".into()),
        ordinal: 1,
        name: Some("Position".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(SketchId("position-geometry".into())),
        },
        native_ref: Some("native-position-sketch".into()),
    };
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-position-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("6".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    let mut lane = lane_with_position_reference(6);
    let marker = |id: &str, ordinal, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal,
        offset: u64::from(ordinal),
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: Some(coordinates_m),
        links: Vec::new(),
        link_selector: None,
    };
    lane.sketch_entities = vec![
        marker("relation-anchor-0", 0, [0.0, 0.0]),
        marker("position-locus", 1, [0.014, 0.025]),
        marker("relation-anchor-1", 2, [0.0, 0.0]),
    ];
    let sketch = Sketch {
        id: SketchId("position-geometry".into()),
        name: Some("Position".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 20.0, 30.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    };
    let mut features = vec![hole, sketch_feature];

    project_hole_position_sketches(
        &mut features,
        std::slice::from_ref(&sketch),
        &[],
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    );

    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        panic!("expected hole");
    };
    assert!(matches!(
        placements.as_deref(),
        Some([HolePlacement::Axis {
            origin: Point3 {
                x: 14.0,
                y: 25.0,
                z: 30.0
            },
            axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }])
    ));

    lane.sketch_entities
        .push(marker("ambiguous-locus", 3, [0.006, 0.007]));
    let mut ambiguous_features = vec![model_hole(), features[1].clone()];
    project_hole_position_sketches(
        &mut ambiguous_features,
        std::slice::from_ref(&sketch),
        &[],
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    );
    let FeatureDefinition::Hole { placements, .. } = &ambiguous_features[0].definition else {
        panic!("expected hole");
    };
    assert!(placements.is_none());
}

#[test]
fn spatial_position_point_uses_unique_radius_matched_bore_axis() {
    let hole = model_hole();
    let sketch_id = SpatialSketchId("position-geometry".into());
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("position-sketch".into()),
        ordinal: 1,
        name: Some("Position".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("native-position-sketch".into()),
    };
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-position-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("6".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "3DSketch".into(),
        input_class: Some("mo3DProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    let mut lane = lane_with_position_reference(6);
    lane.sketch_entities.push(SketchInputEntity {
        id: "authored-point".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 0,
        offset: 80,
        object_index: Some(1),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "same-axis-endpoint".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 1,
        offset: 90,
        object_index: Some(2),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    });
    lane.sketch_entities.push(SketchInputEntity {
        id: "construction-point".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 2,
        offset: 100,
        object_index: Some(3),
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    });
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: Some("Position".into()),
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    };
    let point = Point3::new(12.0, 23.0, 30.0);
    let entity = SpatialSketchEntity::new(
        SpatialSketchEntityId("point".into()),
        sketch_id.clone(),
        SpatialSketchGeometry::Point { position: point },
    )
    .with_native_ref(Some("authored-point".into()));
    let same_axis_endpoint = SpatialSketchEntity::new(
        SpatialSketchEntityId("same-axis-endpoint".into()),
        sketch_id.clone(),
        SpatialSketchGeometry::Point {
            position: Point3::new(12.0, 23.0, 20.0),
        },
    )
    .with_native_ref(Some("same-axis-endpoint".into()));
    let construction_point = SpatialSketchEntity::new(
        SpatialSketchEntityId("construction-point".into()),
        sketch_id,
        SpatialSketchGeometry::Point {
            position: Point3::new(100.0, 100.0, 100.0),
        },
    )
    .with_construction(true)
    .with_native_ref(Some("construction-point".into()));
    let surface = Surface {
        id: SurfaceId("bore".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(12.0, 23.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    };
    let mut features = vec![hole, sketch_feature];

    project_spatial_hole_position_sketches(
        &mut features,
        &[sketch],
        &[entity, same_axis_endpoint, construction_point],
        &[surface],
        &[history],
        &[lane],
    );

    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        panic!("expected hole");
    };
    assert_eq!(
        placements.as_deref(),
        Some(
            &[cadmpeg_ir::features::HolePlacement::Axis {
                origin: Point3::new(12.0, 23.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            }][..]
        )
    );
}

#[test]
fn shared_spatial_sketch_falls_back_to_geometry_without_scoped_markers() {
    let hole = model_hole();
    let sketch_id = SpatialSketchId("position-geometry".into());
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("position-sketch".into()),
        ordinal: 1,
        name: Some("Position".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("native-position-sketch".into()),
    };
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-position-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("6".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "3DSketch".into(),
        input_class: Some("mo3DProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    let mut lane = lane_with_position_reference(6);
    lane.configuration = Some("1".into());
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: Some("Position".into()),
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("shared-lane".into()),
    };
    let positions = [
        Point3::new(12.0, 23.0, 30.0),
        Point3::new(12.0, 33.0, 30.0),
        Point3::new(22.0, 23.0, 30.0),
    ];
    let entities = positions
        .into_iter()
        .enumerate()
        .map(|(index, position)| {
            SpatialSketchEntity::new(
                SpatialSketchEntityId(format!("point-{index}")),
                sketch_id.clone(),
                SpatialSketchGeometry::Point { position },
            )
        })
        .collect::<Vec<_>>();
    let mut features = vec![hole, sketch_feature];

    project_spatial_hole_position_sketches(
        &mut features,
        &[sketch],
        &entities,
        &[],
        &[history],
        &[lane],
    );

    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        panic!("expected hole");
    };
    assert_eq!(
        placements.as_deref(),
        Some(
            &[
                HolePlacement::Axis {
                    origin: Point3::new(12.0, 23.0, 30.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                HolePlacement::Axis {
                    origin: Point3::new(12.0, 33.0, 30.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                HolePlacement::Axis {
                    origin: Point3::new(22.0, 23.0, 30.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
            ][..]
        )
    );
}

#[test]
fn spatial_position_relation_handle_uses_its_model_space_bore_locus() {
    let hole = model_hole();
    let sketch_id = SpatialSketchId("position-geometry".into());
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId("position-sketch".into()),
        ordinal: 1,
        name: Some("Position".into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("native-position-sketch".into()),
    };
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-position-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("6".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Position".into(),
        kind: "3DSketch".into(),
        input_class: Some("mo3DProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    });
    let mut lane = lane_with_position_reference(6);
    lane.sketch_entities.push(SketchInputEntity {
        id: "relation-handle".into(),
        parent: "lane".into(),
        feature_ref: Some("native-position-sketch".into()),
        ordinal: 0,
        offset: 80,
        object_index: Some(1),
        local_id: None,
        kind: SketchInputKind::Relation(SketchRelationKind::Vertical),
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    });
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: Some("Position".into()),
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    };
    let locus = Point3::new(12.0, 23.0, 30.0);
    let entity = SpatialSketchEntity::new(
        SpatialSketchEntityId("relation-locus".into()),
        sketch_id,
        SpatialSketchGeometry::Point { position: locus },
    )
    .with_native_ref(Some("relation-handle".into()));
    let surface = Surface {
        id: SurfaceId("bore".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(12.0, 23.0, 10.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    };
    let mut features = vec![hole, sketch_feature];

    project_spatial_hole_position_sketches(
        &mut features,
        &[sketch],
        &[entity],
        &[surface],
        &[history],
        &[lane],
    );

    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        panic!("expected hole");
    };
    assert_eq!(
        placements.as_deref(),
        Some(
            &[HolePlacement::Axis {
                origin: Point3::new(12.0, 23.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            }][..]
        )
    );
}

#[test]
fn noncollinear_coplanar_spatial_positions_define_one_hole_axis() {
    let points = [
        Point3::new(23.5, 10.0, -75.0),
        Point3::new(23.5, 10.0, -23.0),
        Point3::new(151.5, 10.0, -23.0),
        Point3::new(151.5, 10.0, -75.0),
    ];
    assert_eq!(
        coplanar_spatial_position_placements(&points),
        Some(vec![
            HolePlacement::Axis {
                origin: Point3::new(23.5, 10.0, -23.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
            },
            HolePlacement::Axis {
                origin: Point3::new(23.5, 10.0, -75.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
            },
            HolePlacement::Axis {
                origin: Point3::new(151.5, 10.0, -23.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
            },
            HolePlacement::Axis {
                origin: Point3::new(151.5, 10.0, -75.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
            },
        ])
    );
    assert_eq!(
        coplanar_spatial_position_placements(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 1.0),
            Point3::new(0.0, 2.0, 0.0),
        ]),
        None
    );
    let translated =
        points.map(|point| Point3::new(point.x + 1.0e12, point.y - 1.0e12, point.z + 1.0e12));
    assert!(coplanar_spatial_position_placements(&translated).is_some());
}

#[test]
fn source_intervals_supply_legacy_hole_profiles() {
    let mut history = native_history();
    history.features.push(crate::records::Feature {
        id: "native-profile-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("9".into()),
        parent_source_id: None,
        ordinal: 1,
        name: "Profile".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: [
            ("bore".into(), "<MOD-DIAM>4.2".into()),
            ("depth".into(), "6.8".into()),
            ("tip".into(), "118°".into()),
        ]
        .into(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![
            crate::records::FeatureContent::Dimension("bore".into()),
            crate::records::FeatureContent::Dimension("depth".into()),
            crate::records::FeatureContent::Dimension("tip".into()),
        ],
    });

    let mut lane = lane_with_position_reference(12);
    lane.names.push(FeatureInputName {
        id: "depth-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: 120,
        value: "depth".into(),
        object_id: None,
    });
    lane.names.push(FeatureInputName {
        id: "profile-name".into(),
        parent: "lane".into(),
        ordinal: 2,
        offset: 100,
        value: "Profile".into(),
        object_id: Some(8),
    });
    lane.scalars.push(FeatureInputScalar {
        id: "depth-scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("native-profile-sketch".into()),
        ordinal: 0,
        offset: 150,
        object_id: 1,
        name: "depth-name".into(),
        value: 0.0068,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    });
    let mut histories = [history];
    enrich_history_parameters(&mut histories, [&lane], true);
    assert_eq!(histories[0].features[1].parameters["depth"], "6.8mm");
    enrich_history_hole_constructions(&mut histories, &[lane]);
    assert_eq!(
        histories[0].features[0]
            .properties
            .get("DissectableChildren")
            .map(String::as_str),
        Some("9")
    );

    histories[0].features[0]
        .properties
        .remove("DissectableChildren");
    histories[0].features[1].ordinal = 5;
    let mut next_hole = histories[0].features[0].clone();
    next_hole.id = "next-hole".into();
    next_hole.source_id = Some("20".into());
    next_hole.ordinal = 1;
    histories[0].features.push(next_hole);
    enrich_history_hole_constructions(&mut histories, &[]);
    assert_eq!(
        histories[0].features[0]
            .properties
            .get("DissectableChildren")
            .map(String::as_str),
        Some("9")
    );
}

#[test]
fn serialized_position_successor_owns_legacy_hole_profile() {
    let mut history = native_history();
    let mut position = history.features[0].clone();
    position.id = "native-position-sketch".into();
    position.source_id = Some("12".into());
    position.ordinal = 5;
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.input_class = Some("moProfileFeature_c".into());
    position.parameters.clear();
    position.content.clear();
    history.features.push(position);
    let profile = crate::records::Feature {
        id: "native-profile-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: Some("58".into()),
        parent_source_id: None,
        ordinal: 9,
        name: "Profile".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: [
            ("bore".into(), "<MOD-DIAM>9".into()),
            ("depth".into(), "30".into()),
        ]
        .into(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![
            crate::records::FeatureContent::Dimension("bore".into()),
            crate::records::FeatureContent::Dimension("depth".into()),
        ],
    };
    history.features.push(profile.clone());

    let mut lane = lane_with_position_reference(12);
    lane.native_payload.resize(300, 0);
    lane.names.extend([
        FeatureInputName {
            id: "position-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            value: "Position".into(),
            object_id: Some(12),
        },
        FeatureInputName {
            id: "profile-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 200,
            value: "Profile".into(),
            object_id: Some(58),
        },
    ]);

    enrich_history_hole_constructions(std::slice::from_mut(&mut history), &[lane.clone()]);
    assert_eq!(
        history.features[0]
            .properties
            .get("DissectableChildren")
            .map(String::as_str),
        Some("58")
    );

    history.features[0].properties.remove("DissectableChildren");
    let mut alternate_profile = profile;
    alternate_profile.id = "alternate-profile-sketch".into();
    alternate_profile.source_id = Some("59".into());
    alternate_profile.ordinal = 10;
    history.features.push(alternate_profile);
    let mut alternate_lane = lane_with_position_reference(12);
    alternate_lane.native_payload.resize(300, 0);
    alternate_lane.names.extend([
        FeatureInputName {
            id: "alternate-position-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            value: "Position".into(),
            object_id: Some(12),
        },
        FeatureInputName {
            id: "alternate-profile-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 150,
            value: "Alternate profile".into(),
            object_id: Some(59),
        },
        FeatureInputName {
            id: "later-profile-name".into(),
            parent: "lane".into(),
            ordinal: 3,
            offset: 200,
            value: "Profile".into(),
            object_id: Some(58),
        },
    ]);
    enrich_history_hole_constructions(std::slice::from_mut(&mut history), &[lane, alternate_lane]);
    assert!(!history.features[0]
        .properties
        .contains_key("DissectableChildren"));
}

#[test]
fn ordered_legacy_sketch_children_identify_the_unique_hole_profile() {
    let mut history = native_history();
    let mut position = history.features[0].clone();
    position.id = "native-position-sketch".into();
    position.source_id = Some("8".into());
    position.ordinal = 1;
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.input_class = Some("moProfileFeature_c".into());
    position.parameters.clear();
    position.content.clear();
    history.features.push(position);
    history.features.push(crate::records::Feature {
        id: "native-profile-sketch".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 2,
        name: "Profile".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: [
            ("bore".into(), "<MOD-DIAM>4.2".into()),
            ("depth".into(), "6.8".into()),
        ]
        .into(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![
            crate::records::FeatureContent::Dimension("bore".into()),
            crate::records::FeatureContent::Dimension("depth".into()),
        ],
    });

    enrich_history_hole_constructions(std::slice::from_mut(&mut history), &[]);

    assert_eq!(
        history.features[0]
            .properties
            .get("DissectableChildren")
            .map(String::as_str),
        Some("native-profile-sketch")
    );
    assert_eq!(history.features[2].source_id, None);
}

#[test]
fn parameter_class_supplies_an_operandless_scalar_unit() {
    let mut history = native_history();
    let mut lane = lane_with_position_reference(6);
    lane.classes.push(FeatureInputClass {
        id: "angle-parameter-class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 100,
        name: "moAngleParameter_c".into(),
        role: FeatureInputClassRole::Parameter,
    });
    lane.names.push(FeatureInputName {
        id: "angle-name".into(),
        parent: "lane".into(),
        ordinal: 1,
        offset: 120,
        value: "D1".into(),
        object_id: None,
    });
    lane.scalars.push(FeatureInputScalar {
        id: "angle-scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("native-hole".into()),
        ordinal: 0,
        offset: 150,
        object_id: 1,
        name: "angle-name".into(),
        value: std::f64::consts::TAU,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    });

    enrich_history_parameters(std::slice::from_mut(&mut history), [&lane], true);
    assert_eq!(
        history.features[0].parameters.get("D1").map(String::as_str),
        Some("6.283185307179586rad")
    );
}

#[test]
fn hole_axes_do_not_claim_unowned_same_radius_surfaces() {
    let history = native_history();
    let lane = lane();
    let mut features = vec![model_hole()];
    let surfaces = vec![cylinder(0, -5.0), cylinder(1, 5.0)];

    project_hole_axes(
        &mut features,
        &[],
        &HoleTopology {
            surfaces: &surfaces,
            faces: &[],
            loops: &[],
            coedges: &[],
            edges: &[],
            vertices: &[],
            points: &[],
        },
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    );
    let FeatureDefinition::Hole { placements, .. } = &features[0].definition else {
        unreachable!();
    };
    assert!(placements.is_none());
}
