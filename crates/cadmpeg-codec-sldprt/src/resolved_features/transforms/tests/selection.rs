//! Marker-transform selection tests.

use super::super::*;
use super::marker;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputKind, SketchInputLink,
};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length, ParameterId,
    ParameterValue,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus,
};
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn unique_axis_swap_maps_marker_coordinates_to_profile_loci() {
    let markers = [(0, 0), (2, 1), (7, 4), (3, 9)].into_iter().collect();
    let loci = [(0, 0), (1, 2), (4, 7), (9, 3)].into_iter().collect();
    let transform = unique_marker_transform(&markers, &loci).expect("unique transform");
    assert!(transform.swap);
    assert_eq!(transform.u_sign, 1);
    assert_eq!(transform.v_sign, 1);
    assert!(markers
        .into_iter()
        .all(|point| loci.contains(&transform.apply(point).expect("required invariant"))));
}

#[test]
fn relation_point_materializes_under_one_proven_marker_transform() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut entities = [(0.0, 0.0), (1.0, 2.0), (4.0, 7.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (u, v))| {
            SketchEntity::new(
                SketchEntityId(format!("point-{index}")),
                sketch.clone(),
                SketchGeometry::Point {
                    position: Point2::new(u, v),
                },
            )
        })
        .collect::<Vec<_>>();
    let mut markers = [[0.0, 0.0], [0.002, 0.001], [0.007, 0.004]]
        .into_iter()
        .enumerate()
        .map(|(index, coordinates)| {
            let mut value = marker(&format!("anchor-{index}"), Some(coordinates));
            value.offset = (index * 27) as u64;
            value
        })
        .collect::<Vec<_>>();
    let mut relation_point = marker("relation-point", Some([0.005, 0.006]));
    relation_point.offset = 81;
    markers.push(relation_point.clone());
    let mut endpoint_a = marker("endpoint-a", Some([0.002, 0.001]));
    endpoint_a.offset = 82;
    let mut endpoint_b = marker("endpoint-b", Some([0.007, 0.004]));
    endpoint_b.offset = 83;
    let mut relation_line = marker("relation-line", None);
    relation_line.offset = 84;
    relation_line.kind = SketchInputKind::Arc;
    let mut support_handle = marker("support-handle", None);
    support_handle.offset = 85;
    support_handle.links = vec![SketchInputLink {
        local_id: 3,
        entity_ref: relation_line.id.clone(),
    }];
    let mut qualified_curve = marker("qualified-curve", Some([0.0045, 0.0025]));
    qualified_curve.id = "sldprt:feature-input:sketch-entity#qualified-curve".into();
    qualified_curve.offset = 86;
    qualified_curve.kind = SketchInputKind::LineOrCircle;
    relation_line.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: endpoint_a.id.clone(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: qualified_curve.id.clone(),
        },
    ];
    let mut coincident_point = marker("coincident-point", Some([0.002, 0.001]));
    coincident_point.offset = 87;
    let mut self_linked_curve = marker("self-linked-curve", Some([0.006, 0.005]));
    self_linked_curve.offset = 88;
    self_linked_curve.kind = SketchInputKind::Arc;
    self_linked_curve.links = vec![
        SketchInputLink {
            local_id: 8,
            entity_ref: self_linked_curve.id.clone(),
        },
        SketchInputLink {
            local_id: 9,
            entity_ref: endpoint_b.id.clone(),
        },
    ];
    let mut forward_linked_curve = marker("forward-linked-curve", Some([0.009, 0.009]));
    forward_linked_curve.offset = 89;
    forward_linked_curve.kind = SketchInputKind::Arc;
    forward_linked_curve.links = vec![
        SketchInputLink {
            local_id: 10,
            entity_ref: endpoint_a.id.clone(),
        },
        SketchInputLink {
            local_id: 11,
            entity_ref: endpoint_b.id.clone(),
        },
    ];
    markers.extend([
        endpoint_a,
        endpoint_b,
        relation_line.clone(),
        support_handle.clone(),
        qualified_curve.clone(),
        coincident_point.clone(),
        self_linked_curve.clone(),
        forward_linked_curve.clone(),
    ]);
    let mut native_payload = vec![0; 181];
    for offset in [0, 27, 54] {
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    }
    native_payload[84..84 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    native_payload[89..97].fill(0xff);
    native_payload[97..101].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    native_payload[101..105].copy_from_slice(&2u32.to_le_bytes());
    native_payload[107..111].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    native_payload[111..113].copy_from_slice(&1u16.to_le_bytes());
    native_payload[115..123].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    native_payload[132..140].copy_from_slice(&1.0f64.to_le_bytes());
    native_payload[148..150].copy_from_slice(&4u16.to_le_bytes());
    native_payload[150..152].copy_from_slice(&6u16.to_le_bytes());
    native_payload[152..156].copy_from_slice(&1u32.to_le_bytes());
    native_payload[156..164].copy_from_slice(&(-1.0f64).to_le_bytes());
    native_payload[176..176 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![
            FeatureInputRelationInstance {
                id: "relation".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 90,
                family: FeatureInputRelationFamily::CircleDiameter,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![FeatureInputOperand {
                    offset: 91,
                    reference_ref: "reference".into(),
                    kind: FeatureInputOperandKind::Native(0x929d),
                    entity_index: 0,
                    entity_ref: Some(relation_point.id.clone()),
                }],
            },
            FeatureInputRelationInstance {
                id: "qualified-point-relation".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 94,
                family: FeatureInputRelationFamily::PointPointDistance,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![
                    FeatureInputOperand {
                        offset: 95,
                        reference_ref: "qualified-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x837b),
                        entity_index: 16,
                        entity_ref: Some(qualified_curve.id.clone()),
                    },
                    FeatureInputOperand {
                        offset: 96,
                        reference_ref: "point-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x837b),
                        entity_index: 17,
                        entity_ref: Some(relation_point.id.clone()),
                    },
                ],
            },
            FeatureInputRelationInstance {
                id: "line-relation".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 92,
                family: FeatureInputRelationFamily::LineLineDistance,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![FeatureInputOperand {
                    offset: 93,
                    reference_ref: "line-reference".into(),
                    kind: FeatureInputOperandKind::Native(0x8386),
                    entity_index: 0,
                    entity_ref: Some(support_handle.id.clone()),
                }],
            },
            FeatureInputRelationInstance {
                id: "coincident-point-relation".into(),
                parent: "lane".into(),
                ordinal: 3,
                offset: 97,
                family: FeatureInputRelationFamily::PointPointDistance,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![
                    FeatureInputOperand {
                        offset: 98,
                        reference_ref: "coincident-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x837b),
                        entity_index: 18,
                        entity_ref: Some(coincident_point.id.clone()),
                    },
                    FeatureInputOperand {
                        offset: 99,
                        reference_ref: "coincident-pair-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x837b),
                        entity_index: 17,
                        entity_ref: Some(relation_point.id.clone()),
                    },
                ],
            },
            FeatureInputRelationInstance {
                id: "self-linked-curve-relation".into(),
                parent: "lane".into(),
                ordinal: 4,
                offset: 100,
                family: FeatureInputRelationFamily::Angle,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![
                    FeatureInputOperand {
                        offset: 101,
                        reference_ref: "self-linked-curve-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 18,
                        entity_ref: Some(self_linked_curve.id.clone()),
                    },
                    FeatureInputOperand {
                        offset: 102,
                        reference_ref: "support-curve-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 19,
                        entity_ref: Some(support_handle.id.clone()),
                    },
                ],
            },
            FeatureInputRelationInstance {
                id: "forward-linked-curve-relation".into(),
                parent: "lane".into(),
                ordinal: 5,
                offset: 103,
                family: FeatureInputRelationFamily::LineLineDistance,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: Vec::new(),
                parameter_scalar_ref: None,
                display_scalar_ref: None,
                operands: vec![
                    FeatureInputOperand {
                        offset: 104,
                        reference_ref: "forward-linked-curve-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 20,
                        entity_ref: Some(forward_linked_curve.id.clone()),
                    },
                    FeatureInputOperand {
                        offset: 105,
                        reference_ref: "forward-support-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 21,
                        entity_ref: Some(support_handle.id.clone()),
                    },
                ],
            },
        ],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    };
    project_relation_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );
    let projected_len = entities.len();
    project_relation_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );
    assert_eq!(entities.len(), projected_len);
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("relation-point")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(6.0, 5.0)
            )
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("self-linked-curve")
            && entity.endpoint_refs == ["endpoint-b", "self-linked-curve"]
            && matches!(entity.geometry, SketchGeometry::Line { start, end }
                if start == Point2::new(4.0, 7.0) && end == Point2::new(5.0, 6.0))
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("forward-linked-curve")
            && entity.endpoint_refs == ["endpoint-a", "endpoint-b"]
            && matches!(entity.geometry, SketchGeometry::Line { start, end }
                if start == Point2::new(1.0, 2.0) && end == Point2::new(4.0, 7.0))
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("coincident-point")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(1.0, 2.0)
            )
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.is_none()
            && entity.geometry_ref.as_deref()
                == Some("sldprt:feature-input:sketch-entity#qualified-curve")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(2.5, 4.5)
            )
    }));
    assert!(entities.iter().any(|entity| {
        entity.construction
            && entity.native_ref.as_deref() == Some("relation-line")
            && entity.endpoint_refs
                == [
                    "endpoint-a",
                    "sldprt:feature-input:sketch-entity#qualified-curve",
                ]
            && matches!(entity.geometry, SketchGeometry::Line { start, end }
                if start == Point2::new(1.0, 2.0) && end == Point2::new(2.5, 4.5))
    }));
    let loci = profile_loci_by_marker(
        std::slice::from_ref(&feature),
        &[],
        &entities,
        std::slice::from_ref(&lane),
    );
    assert_eq!(
        loci["sldprt:feature-input:sketch-entity#qualified-curve:qualified-point"],
        vec![SketchLocus::End(SketchEntityId(
            "sldprt:model:sketch-entity#relation-line:lane:84".into(),
        ))]
    );
    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_point_locus(
            "sldprt:feature-input:sketch-entity#qualified-curve",
            &markers,
            &loci,
        ),
        Some(SketchLocus::End(SketchEntityId(
            "sldprt:model:sketch-entity#relation-line:lane:84".into(),
        )))
    );
}

#[test]
fn relation_point_coexists_with_nonpoint_native_carrier() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut entities = [(0.0, 0.0), (1.0, 2.0), (4.0, 7.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (u, v))| {
            SketchEntity::new(
                SketchEntityId(format!("anchor-{index}")),
                sketch.clone(),
                SketchGeometry::Point {
                    position: Point2::new(u, v),
                },
            )
        })
        .collect::<Vec<_>>();
    let mut markers = [[0.0, 0.0], [0.002, 0.001], [0.007, 0.004]]
        .into_iter()
        .enumerate()
        .map(|(index, coordinates)| {
            let mut value = marker(&format!("anchor-{index}"), Some(coordinates));
            value.offset = (index * 27) as u64;
            value
        })
        .collect::<Vec<_>>();
    let mut point_marker = marker("dimension-point", Some([0.005, 0.006]));
    point_marker.offset = 81;
    markers.push(point_marker.clone());
    entities.push(
        SketchEntity::new(
            SketchEntityId("dimension-carrier".into()),
            sketch.clone(),
            SketchGeometry::Circle {
                center: Point2::new(5.0, 6.0),
                radius: Length(10.0),
            },
        )
        .with_construction(true)
        .with_native_ref(Some(point_marker.id.clone())),
    );
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 90,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalar_refs: Vec::new(),
            parameter_scalar_ref: None,
            display_scalar_ref: None,
            operands: vec![FeatureInputOperand {
                offset: 91,
                reference_ref: "reference".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 0,
                entity_ref: Some(point_marker.id.clone()),
            }],
        }],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    };

    project_relation_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );

    assert!(entities.iter().any(|entity| {
        entity.native_ref.as_deref() == Some(point_marker.id.as_str())
            && matches!(entity.geometry, SketchGeometry::Circle { .. })
    }));
    assert!(entities.iter().any(|entity| {
        entity.native_ref.as_deref() == Some(point_marker.id.as_str())
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(6.0, 5.0)
            )
    }));
    let loci = profile_loci_by_marker(
        std::slice::from_ref(&feature),
        &[],
        &entities,
        std::slice::from_ref(&lane),
    );
    let point_entity = entities
        .iter()
        .find(|entity| {
            entity.construction
                && entity.native_ref.as_deref() == Some(point_marker.id.as_str())
                && matches!(entity.geometry, SketchGeometry::Point { .. })
        })
        .expect("relation point");
    assert_eq!(
        loci[point_marker.id.as_str()],
        vec![SketchLocus::Center(SketchEntityId(
            "dimension-carrier".into()
        ))]
    );
    assert_eq!(
        loci[&super::qualified_point_marker_key(&point_marker.id)],
        vec![SketchLocus::Entity(point_entity.id().clone())]
    );
    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        marker_entities(&point_marker.id, &markers, &loci),
        vec![SketchEntityId("dimension-carrier".into())]
    );
    assert_eq!(
        marker_point_locus(&point_marker.id, &markers, &loci),
        Some(SketchLocus::Entity(point_entity.id().clone()))
    );
}

#[test]
fn relation_point_uses_resolved_sketch_frame_when_marker_transform_is_ambiguous() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let sketch_record = Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let mut first_marker = marker("first-point", Some([-0.005, 0.002]));
    first_marker.offset = 1;
    let mut second_marker = marker("second-point", Some([0.005, 0.002]));
    second_marker.offset = 2;
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 3,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature-native".into(),
        scalar_refs: vec!["distance".into()],
        parameter_scalar_ref: Some("distance".into()),
        display_scalar_ref: None,
        operands: vec![
            FeatureInputOperand {
                offset: 4,
                reference_ref: "first-reference".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 0,
                entity_ref: Some(first_marker.id.clone()),
            },
            FeatureInputOperand {
                offset: 5,
                reference_ref: "second-reference".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 1,
                entity_ref: Some(second_marker.id.clone()),
            },
        ],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![relation],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![first_marker, second_marker],
    };
    let mut entities = Vec::new();

    project_relation_point_geometry(
        &mut entities,
        std::slice::from_ref(&sketch_record),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );

    assert_eq!(entities.len(), 2);
    assert!(entities.iter().any(|entity| {
        entity.native_ref.as_deref() == Some("first-point")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(-5.0, 2.0)
            )
    }));
    assert!(entities.iter().any(|entity| {
        entity.native_ref.as_deref() == Some("second-point")
            && matches!(
                entity.geometry,
                SketchGeometry::Point { position } if position == Point2::new(5.0, 2.0)
            )
    }));
}

#[test]
fn unique_zero_translation_resolves_symmetric_axis_swaps() {
    let markers = [(0, 0), (48, 0), (48, 24), (0, 24)].into_iter().collect();
    let loci = [(0, 0), (24, 0), (24, 48), (0, 48)].into_iter().collect();
    assert_eq!(
        unique_marker_transform(&markers, &loci),
        Some(MarkerTransform {
            swap: true,
            u_sign: 1,
            v_sign: 1,
            affine_matrix: None,
            translation: (0, 0),
        })
    );
}

#[test]
fn marker_kinds_disambiguate_axis_swaps() {
    let compatible = HashMap::from([
        ((0, 0), HashSet::from([(10, 20)])),
        ((0, 2), HashSet::from([(12, 20)])),
        ((3, 1), HashSet::from([(11, 23)])),
    ]);
    let transform = unique_compatible_marker_transform(&compatible).expect("required invariant");
    assert!(transform.swap);
    assert_eq!(transform.u_sign, 1);
    assert_eq!(transform.v_sign, 1);
    assert_eq!(transform.translation, (10, 20));
}

#[test]
fn symmetric_frames_require_the_same_dimensioned_circle_set() {
    let identity = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: None,
        translation: (0, 0),
    };
    let swap = MarkerTransform {
        swap: true,
        ..identity
    };
    assert_eq!(
        dimensioned_circle_transform(&[swap, identity], &[((10, 20), 5), ((20, 10), 5)]),
        Some(identity)
    );
    assert_eq!(
        dimensioned_circle_transform(&[identity, swap], &[((10, 20), 5), ((20, 10), 7)]),
        None
    );
}

#[test]
fn cylinder_centers_resolve_dimensioned_circle_frame() {
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(20.0, 20.0, 0.0),
            normal: Vector3::new(-1.0, 0.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let circles = [((6, 14), 3), ((14, 14), 3), ((14, 7), 3), ((6, 7), 3)];
    let surfaces = [(14.0, -6.0), (14.0, -14.0), (7.0, -14.0), (7.0, -6.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (y, z))| Surface {
            id: SurfaceId(format!("cylinder-{index}")),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(19.5, y, z),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 3.0,
            },
            source_object: None,
        })
        .collect::<Vec<_>>();
    let candidates = dimensioned_circle_surface_transforms(&sketch, &surfaces, &circles, 1.0);
    let transform =
        dimensioned_circle_transform(&candidates, &circles).expect("required invariant");
    let transformed = circles
        .iter()
        .map(|(center, _)| transform.apply(*center).expect("required invariant"))
        .collect::<HashSet<_>>();
    assert_eq!(
        transformed,
        HashSet::from([(-6, -6), (-14, -6), (-14, -13), (-6, -13)])
    );
}

#[test]
fn circular_profile_binds_by_unique_diameter_signature() {
    let sketch_id = SketchId("circle-profile".into());
    let entity_id = SketchEntityId("circle".into());
    let feature = |id: &str, name: &str, sketch| Feature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: Some(name.into()),
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch { sketch },
        native_ref: Some(format!("native-{id}")),
    };
    let mut features = vec![
        feature("first", "Sketch1", None),
        feature("second", "Sketch2", Some(sketch_id.clone())),
    ];
    let parameter = |id: &str, owner: &str, diameter: f64| DesignParameter {
        id: ParameterId(id.into()),
        owner: Some(FeatureId(owner.into())),
        ordinal: 0,
        name: "D1".into(),
        expression: format!("<MOD-DIAM>{diameter}"),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(diameter))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let parameters = [
        parameter("first-diameter", "first", 4.0),
        parameter("second-diameter", "second", 5.0),
    ];
    let mut sketches = [Sketch {
        id: sketch_id.clone(),
        name: Some("Sketch2".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    }];
    let entities = [SketchEntity::new(
        entity_id,
        sketch_id.clone(),
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    )];

    bind_circular_profile_by_dimension(&mut features, &mut sketches, &entities, &parameters);

    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::Sketch { sketch: Some(id), .. } if id == &sketch_id
    ));
    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert_eq!(sketches[0].name.as_deref(), Some("Sketch1"));
}
