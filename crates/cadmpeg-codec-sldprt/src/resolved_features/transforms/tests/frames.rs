//! Sketch-frame, dimensioned-circle, and nested-profile tests.

use super::super::*;
use super::marker;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputLane, FeatureInputName, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputReference, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputScalarRole, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use crate::resolved_features::relation_geometry::declared_entity_handle_circular_marker;
use cadmpeg_ir::annotations::ExactnessNote;
use cadmpeg_ir::features::{
    Angle, DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length,
    ParameterId, ParameterValue,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchGeometry, SketchId, SketchLocus, SketchPlacement,
};
use cadmpeg_ir::AnnotationBuilder;
use std::collections::{BTreeMap, HashMap};

#[test]
fn circle_dimension_driver_supplies_the_center_operand() {
    let operand = |index, marker: &str| FeatureInputOperand {
        offset: u64::from(index),
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::Native(0x929d),
        entity_index: index,
        entity_ref: Some(marker.into()),
    };
    let scalar = |id: &str, offset, operands| FeatureInputScalar {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_id: 1,
        name: "dimension-name".into(),
        value: 1.0,
        role: FeatureInputScalarRole::Native,
        entity_indices: Vec::new(),
        operands,
    };
    let display_operand = operand(2, "display-handle");
    let display = FeatureInputScalar {
        role: FeatureInputScalarRole::Display,
        ..scalar("display", 10, vec![display_operand.clone()])
    };
    let driver = scalar(
        "driver",
        20,
        vec![display_operand.clone(), operand(1, "center")],
    );
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "dimension-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            value: "D1".into(),
            object_id: None,
        }],
        scalars: vec![display, driver],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut relations = vec![FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 10,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["display".into()],
        parameter_scalar_ref: None,
        display_scalar_ref: Some("display".into()),
        operands: vec![display_operand],
    }];

    bind_circle_dimension_centers(&mut relations, &lane);

    assert_eq!(relations[0].scalar_refs, ["display", "driver"]);
    assert_eq!(relations[0].operands.len(), 2);
    assert_eq!(
        relations[0].operands[1].entity_ref.as_deref(),
        Some("center")
    );
}

#[test]
fn point_distance_preserves_stored_operands_when_geometry_is_inconsistent() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(id.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(u, 0.0),
        },
    };
    let entities = vec![
        point("hint-a", 0.0),
        point("hint-b", 2.0),
        point("solved", 5.0),
    ];
    let hint_a = marker("hint-a", Some([0.0, 0.0]));
    let hint_b = marker("hint-b", Some([0.002, 0.0]));
    let markers = HashMap::from([(hint_a.id.as_str(), &hint_a), (hint_b.id.as_str(), &hint_b)]);
    let mut loci = HashMap::from([
        (
            hint_a.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("hint-a".into()))],
        ),
        (
            hint_b.id.clone(),
            vec![SketchLocus::Entity(SketchEntityId("hint-b".into()))],
        ),
    ]);
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: vec![
            FeatureInputOperand {
                offset: 1,
                reference_ref: "reference-a".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 0,
                entity_ref: Some(hint_a.id.clone()),
            },
            FeatureInputOperand {
                offset: 2,
                reference_ref: "reference-b".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 1,
                entity_ref: Some(hint_b.id.clone()),
            },
        ],
    };
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            ..
        }) if [&first, &second].contains(&&SketchEntityId("hint-a".into()))
            && [&first, &second].contains(&&SketchEntityId("hint-b".into()))
    ));

    let mut horizontal_relation = relation.clone();
    horizontal_relation.family = FeatureInputRelationFamily::PointPointHorizontalDistance;
    assert!(matches!(
        typed_relation_definition(
            &horizontal_relation,
            Some(&parameter),
            &sketch,
            &entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            ..
        }) if [&first, &second].contains(&&SketchEntityId("hint-a".into()))
            && [&first, &second].contains(&&SketchEntityId("hint-b".into()))
    ));

    let mut directional_entities = vec![point("hint-a", 0.0), point("hint-b", 1.0)];
    let mut projected_relation = relation.clone();
    for operand in &mut projected_relation.operands {
        operand.kind = FeatureInputOperandKind::Native(0xbc7c);
    }
    loci.insert(
        super::qualified_point_marker_key(&hint_a.id),
        vec![SketchLocus::Entity(SketchEntityId("hint-a".into()))],
    );
    loci.insert(
        super::qualified_point_marker_key(&hint_b.id),
        vec![SketchLocus::Entity(SketchEntityId("hint-b".into()))],
    );
    directional_entities[1].geometry = SketchGeometry::Point {
        position: Point2::new(1.0, 0.05),
    };
    let mut directional_parameter = parameter.clone();
    directional_parameter.value = Some(ParameterValue::Length(Length(1.0)));
    assert!(matches!(
        typed_relation_definition(
            &projected_relation,
            Some(&directional_parameter),
            &sketch,
            &directional_entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance { .. })
    ));
    directional_parameter.value = Some(ParameterValue::Length(Length(0.05)));
    assert!(matches!(
        typed_relation_definition(
            &projected_relation,
            Some(&directional_parameter),
            &sketch,
            &directional_entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::VerticalDistance { .. })
    ));
    directional_entities[1].geometry = SketchGeometry::Point {
        position: Point2::new(1.0, 1.0),
    };
    directional_parameter.value = Some(ParameterValue::Length(Length(1.0)));
    assert!(matches!(
        typed_relation_definition(
            &projected_relation,
            Some(&directional_parameter),
            &sketch,
            &directional_entities,
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::DistanceLoci { .. })
    ));

    let mut ambiguous_entities = entities;
    ambiguous_entities.push(point("other-solved", -5.0));
    for candidate in [&relation, &horizontal_relation] {
        assert!(typed_relation_definition(
            candidate,
            Some(&parameter),
            &sketch,
            &ambiguous_entities,
            &markers,
            &loci,
        )
        .is_some());
    }

    let unrelated_entities = vec![
        point("hint-a", 0.0),
        point("hint-b", 2.0),
        point("unrelated-a", 10.0),
        point("unrelated-b", 15.0),
    ];
    for candidate in [&relation, &horizontal_relation] {
        assert!(typed_relation_definition(
            candidate,
            Some(&parameter),
            &sketch,
            &unrelated_entities,
            &markers,
            &loci,
        )
        .is_some());
    }
}

#[test]
fn display_scalar_name_resolves_one_unclaimed_owner_parameter() {
    let feature = Feature {
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
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: None,
        },
        native_ref: Some("native-feature".into()),
    };
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(feature.id.clone()),
        name: "D1".into(),
        ordinal: 0,
        expression: "12".into(),
        value: Some(ParameterValue::Length(Length(12.0))),
        display: None,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("existing-driver".into()),
        dependencies: Vec::new(),
    };
    let scalar = FeatureInputScalar {
        id: "scalar".into(),
        parent: "lane".into(),
        feature_ref: Some("native-feature".into()),
        ordinal: 0,
        offset: 10,
        object_id: 1,
        name: "name".into(),
        value: 0.012,
        role: FeatureInputScalarRole::Display,
        entity_indices: Vec::new(),
        operands: Vec::new(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            value: "D1".into(),
            object_id: None,
        }],
        scalars: vec![scalar.clone()],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 10,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "native-feature".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: None,
        display_scalar_ref: Some("scalar".into()),
        operands: Vec::new(),
    };
    assert_eq!(
        relation_parameter_by_display_name(
            &relation,
            &lane,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
        )
        .map(|parameter| &parameter.id),
        Some(&parameter.id)
    );
    assert_eq!(
        owned_relation_parameters(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&FeatureInputLane {
                relation_instances: vec![relation.clone()],
                ..lane.clone()
            }),
        )["relation"]
            .as_ref(),
        Some(&parameter.id)
    );
    let mut mismatched_parameter = parameter.clone();
    mismatched_parameter.value = Some(ParameterValue::Length(Length(20.0)));
    assert_eq!(
        relation_parameter_by_display_name(
            &relation,
            &lane,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&mismatched_parameter),
        )
        .map(|parameter| &parameter.id),
        None
    );
    let mut synthesized_parameters = vec![mismatched_parameter.clone()];
    crate::resolved_features::projections::synthesize_display_relation_parameters(
        &mut synthesized_parameters,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&FeatureInputLane {
            relation_instances: vec![relation.clone()],
            ..lane.clone()
        }),
    );
    let synthetic = synthesized_parameters
        .iter()
        .find(|parameter| {
            parameter.properties.get("sldprt_relation_parameter_role") == Some(&"reference".into())
        })
        .expect("display-only relation parameter");
    assert_eq!(synthetic.value, Some(ParameterValue::Length(Length(12.0))));
    assert!(synthetic.native_ref.is_none());
    let nested_relation = FeatureInputRelationInstance {
        id: "sldprt:feature-input:relation-instance#lane:10".into(),
        ..relation.clone()
    };
    let mut nested_parameters = vec![mismatched_parameter];
    crate::resolved_features::projections::synthesize_display_relation_parameters(
        &mut nested_parameters,
        std::slice::from_ref(&feature),
        std::slice::from_ref(&FeatureInputLane {
            relation_instances: vec![nested_relation],
            ..lane.clone()
        }),
    );
    let nested = nested_parameters
        .iter()
        .find(|parameter| {
            parameter.properties.get("sldprt_relation_parameter_role") == Some(&"reference".into())
        })
        .expect("nested display-only relation parameter");
    assert_eq!(
        nested.id,
        ParameterId("sldprt:model:parameter#reference:lane:10".into())
    );
    assert_eq!(
        owned_relation_parameters(
            std::slice::from_ref(&feature),
            &synthesized_parameters,
            std::slice::from_ref(&FeatureInputLane {
                relation_instances: vec![relation.clone()],
                ..lane.clone()
            }),
        )["relation"]
            .as_ref(),
        Some(&synthetic.id)
    );
    let mut exact_parameter = parameter.clone();
    exact_parameter.native_ref = Some("scalar".into());
    let mut exact_lane = lane.clone();
    exact_lane.scalars[0].role = FeatureInputScalarRole::Native;
    exact_lane.relation_instances = vec![FeatureInputRelationInstance {
        display_scalar_ref: None,
        ..relation.clone()
    }];
    assert_eq!(
        owned_relation_parameters(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&exact_parameter),
            std::slice::from_ref(&exact_lane),
        )["relation"]
            .as_ref(),
        Some(&exact_parameter.id)
    );
    let driving_relation = FeatureInputRelationInstance {
        id: "driving-relation".into(),
        parameter_scalar_ref: Some("existing-driver".into()),
        display_scalar_ref: None,
        scalar_refs: vec!["existing-driver".into()],
        ..relation.clone()
    };
    let ownership = owned_relation_parameters(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&FeatureInputLane {
            relation_instances: vec![relation.clone(), driving_relation],
            ..lane.clone()
        }),
    );
    assert_eq!(ownership.len(), 1);
    assert_eq!(ownership["driving-relation"].as_ref(), Some(&parameter.id));

    let mut driving_scalar = scalar.clone();
    driving_scalar.id = "driving-by-name".into();
    driving_scalar.ordinal = 1;
    driving_scalar.offset = 20;
    driving_scalar.object_id = 2;
    driving_scalar.name = "driving-name".into();
    driving_scalar.role = FeatureInputScalarRole::Driving;
    let driving_relation = FeatureInputRelationInstance {
        id: "driving-by-name-relation".into(),
        parameter_scalar_ref: Some(driving_scalar.id.clone()),
        display_scalar_ref: None,
        scalar_refs: vec![driving_scalar.id.clone()],
        ..relation.clone()
    };
    let driving_parameter = DesignParameter {
        id: ParameterId("driving-by-name-parameter".into()),
        name: "D".into(),
        native_ref: None,
        ..parameter.clone()
    };
    let mut driving_name = lane.names[0].clone();
    driving_name.id = driving_scalar.name.clone();
    driving_name.value = driving_parameter.name.clone();
    let ownership = owned_relation_parameters(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&driving_parameter),
        std::slice::from_ref(&FeatureInputLane {
            names: vec![lane.names[0].clone(), driving_name],
            scalars: vec![scalar.clone(), driving_scalar],
            relation_instances: vec![driving_relation],
            ..lane.clone()
        }),
    );
    assert_eq!(
        ownership["driving-by-name-relation"].as_ref(),
        Some(&driving_parameter.id)
    );

    let mut detached = scalar;
    detached.id = "driver".into();
    detached.role = FeatureInputScalarRole::Driving;
    detached.operands.clear();
    let mut detached_lane = lane.clone();
    detached_lane.scalars.push(detached);
    let mut detached_relation = vec![relation.clone()];
    bind_detached_relation_drivers(&mut detached_relation, &detached_lane);
    assert_eq!(
        detached_relation[0].parameter_scalar_ref.as_deref(),
        Some("driver")
    );
    assert_eq!(detached_relation[0].scalar_refs, ["scalar", "driver"]);

    let mut parameter = parameter;
    parameter.value = Some(ParameterValue::Integer(12));
    type_display_relation_parameters(
        std::slice::from_mut(&mut parameter),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&FeatureInputLane {
            relation_instances: vec![FeatureInputRelationInstance {
                family: FeatureInputRelationFamily::CircleDiameter,
                ..relation.clone()
            }],
            ..lane.clone()
        }),
    );
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(12.0))));
    assert_eq!(parameter.expression, "<MOD-DIAM>12mm");
    assert_eq!(parameter.display, Some(DimensionDisplay::Diameter));

    parameter.value = Some(ParameterValue::Real(0.012));
    parameter.expression = "0.012".into();
    parameter.display = None;
    parameter.native_ref = Some("driver".into());
    type_display_relation_parameters(
        std::slice::from_mut(&mut parameter),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&FeatureInputLane {
            relation_instances: vec![
                FeatureInputRelationInstance {
                    family: FeatureInputRelationFamily::PointPointDistance,
                    parameter_scalar_ref: Some("driver".into()),
                    ..relation.clone()
                },
                FeatureInputRelationInstance {
                    id: "other-relation".into(),
                    family: FeatureInputRelationFamily::Angle,
                    parameter_scalar_ref: Some("other-driver".into()),
                    ..relation
                },
            ],
            ..lane
        }),
    );
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(12.0))));
    assert_eq!(parameter.expression, "12mm");
}

#[test]
fn axis_aligned_sketch_frame_projects_native_plane_coordinates() {
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(28.65, -35.0, 0.35),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let transform = sketch_frame_marker_transform(&sketch, 1.0e-8).expect("axis frame");
    assert_eq!(
        transform.apply((2_865_000_000, -2_385_000_000)),
        Some((2_420_000_000, 0))
    );
    let other = MarkerTransform {
        u_sign: 1,
        ..transform
    };
    assert_eq!(
        marker_transforms_with_frame_fallback(&[other, transform], &sketch, 1.0e-8),
        vec![other, transform]
    );
    let translated = MarkerTransform {
        translation: (17, 23),
        ..transform
    };
    assert_eq!(
        marker_transforms_with_frame_fallback(&[other, translated], &sketch, 1.0e-8),
        vec![other, translated]
    );
    assert_eq!(
        marker_transforms_with_frame_fallback(&[other], &sketch, 1.0e-8),
        vec![other]
    );
    assert_eq!(
        marker_transforms_with_frame_fallback(&[], &sketch, 1.0e-8),
        vec![transform]
    );
}

#[test]
fn marker_transform_reports_the_profile_axis_for_each_native_axis() {
    const SCALE: i64 = 1_000_000_000_000;

    let swapped = MarkerTransform {
        swap: true,
        u_sign: -1,
        v_sign: 1,
        affine_matrix: None,
        translation: (0, 0),
    };
    assert_eq!(swapped.profile_axis_for_native(0), Some(ProfileAxis::V));
    assert_eq!(swapped.profile_axis_for_native(1), Some(ProfileAxis::U));

    let direct = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: -1,
        affine_matrix: None,
        translation: (0, 0),
    };
    assert_eq!(direct.profile_axis_for_native(0), Some(ProfileAxis::U));
    assert_eq!(direct.profile_axis_for_native(1), Some(ProfileAxis::V));

    let rotated = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: Some([SCALE / 2, -SCALE / 2, SCALE / 2, SCALE / 2]),
        translation: (0, 0),
    };
    assert_eq!(rotated.profile_axis_for_native(0), None);
    assert_eq!(rotated.profile_axis_for_native(1), None);
    assert_eq!(direct.profile_axis_for_native(2), None);
}

#[test]
fn rotated_sketch_frame_projects_native_plane_coordinates() {
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 3.0, 20.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(diagonal, 0.0, -diagonal),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let transform = sketch_frame_marker_transform(&sketch, 1.0e-8).expect("rotated frame");

    assert!(transform.affine_matrix.is_some());
    assert_eq!(
        transform.apply((1_100_000_000, 1_900_000_000)),
        Some(((std::f64::consts::SQRT_2 / 1.0e-8).round() as i64, 0))
    );
}

#[test]
fn dimensioned_circle_materializes_from_an_alternate_handle_frame() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
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
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let mut entities = vec![
        SketchEntity {
            id: SketchEntityId("horizontal".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(10.0, 20.0),
                end: Point2::new(30.0, 20.0),
            },
        },
        SketchEntity {
            id: SketchEntityId("vertical".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(30.0, 20.0),
                end: Point2::new(30.0, 50.0),
            },
        },
    ];
    let mut horizontal = marker("horizontal-marker", Some([0.020, 0.020]));
    horizontal.kind = SketchInputKind::LineOrCircle;
    horizontal.offset = 0;
    let mut vertical = marker("vertical-marker", Some([0.035, 0.030]));
    vertical.kind = SketchInputKind::LineOrCircle;
    vertical.offset = 32;
    let mut center = marker("circle-center", Some([0.040, 0.015]));
    center.kind = SketchInputKind::LineOrCircle;
    center.offset = 64;
    let mut native_payload = vec![0; 96];
    for offset in [0, 32, 64] {
        native_payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    }
    let relation = FeatureInputRelationInstance {
        id: "circle-relation".into(),
        parent: "lane".into(),
        feature_ref: "feature-native".into(),
        ordinal: 0,
        offset: 80,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "circle-class".into(),
        parameter_scalar_ref: Some("circle-scalar".into()),
        display_scalar_ref: None,
        operands: vec![FeatureInputOperand {
            offset: 81,
            reference_ref: "circle-reference".into(),
            kind: FeatureInputOperandKind::Native(0x8ab6),
            entity_index: 0,
            entity_ref: Some("circle-center".into()),
        }],
        scalar_refs: Vec::new(),
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload,
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
        sketch_entities: vec![horizontal, vertical, center],
    };
    let parameter = DesignParameter {
        id: ParameterId("diameter".into()),
        owner: Some(FeatureId("feature".into())),
        name: "D1".into(),
        ordinal: 0,
        expression: String::new(),
        value: Some(ParameterValue::Length(Length(8.0))),
        display: Some(DimensionDisplay::Diameter),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("circle-scalar".into()),
        dependencies: Vec::new(),
    };

    project_dimensioned_sketch_geometry(
        &mut entities,
        &[],
        &[],
        &[feature],
        &[parameter],
        std::slice::from_ref(&lane),
    );
    assert!(matches!(
        &entities[2].geometry,
        SketchGeometry::Circle { center, radius }
            if *center == Point2::new(15.0, 40.0) && *radius == Length(4.0)
    ));
    assert!(!entities[2].construction);

    let mut implicit_lane = lane;
    let mut implicit_center = marker("implicit-center", Some([0.010, 0.020]));
    implicit_center.local_id = Some(1);
    implicit_center.offset = 100;
    let mut implicit_radial = marker("implicit-radial", Some([0.013, 0.024]));
    implicit_radial.local_id = Some(2);
    implicit_radial.offset = 200;
    implicit_lane.sketch_entities = vec![implicit_center, implicit_radial];
    let (resolved, radius) = implicit_circle_marker(
        std::slice::from_ref(&implicit_lane),
        "feature-native",
        FeatureInputOperandKind::Native(0x83fe),
        0,
        5.0,
    )
    .expect("implicit circle pair");
    assert_eq!(resolved.id, "implicit-center");
    assert!((radius - 5.0).abs() < 1.0e-12);
    assert!(implicit_circle_marker(
        std::slice::from_ref(&implicit_lane),
        "feature-native",
        FeatureInputOperandKind::Native(0x8ab6),
        0,
        5.0,
    )
    .is_none());
}

#[test]
fn implicit_circle_uses_its_solver_relation_in_a_mixed_point_roster() {
    let mut unrelated = marker("unrelated", Some([0.0, 0.0]));
    unrelated.offset = 10;
    unrelated.object_index = Some(7);
    let mut center = marker("center", Some([0.010, 0.020]));
    center.offset = 20;
    center.object_index = Some(9);
    center.local_id = Some(11);
    let mut radial = marker("radial", Some([0.013, 0.024]));
    radial.offset = 30;
    radial.object_index = Some(8);
    radial.local_id = Some(12);
    let mut relation = marker("circle-owner", None);
    relation.offset = 40;
    relation.object_index = Some(1);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    relation.links = vec![
        SketchInputLink {
            local_id: 11,
            entity_ref: center.id.clone(),
        },
        SketchInputLink {
            local_id: 11,
            entity_ref: center.id.clone(),
        },
    ];
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
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![unrelated, center, radial, relation],
    };

    let lanes = [lane];
    let (resolved, radius) = implicit_circle_marker(
        &lanes,
        "feature-native",
        FeatureInputOperandKind::Native(0x83fe),
        0,
        5.0,
    )
    .expect("solver-owned implicit circle");

    assert_eq!(resolved.id, "center");
    assert!((radius - 5.0).abs() < 1.0e-12);
}

#[test]
fn implicit_circle_uses_unique_terminal_radial_point() {
    let mut unrelated = marker("unrelated", Some([0.0, 0.0]));
    unrelated.offset = 10;
    unrelated.local_id = Some(1);
    let mut center = marker("center", Some([0.010, 0.010]));
    center.offset = 20;
    center.local_id = Some(2);
    let mut another = marker("another", Some([0.020, 0.020]));
    another.offset = 30;
    another.local_id = Some(3);
    let mut radial = marker("radial", Some([0.013, 0.014]));
    radial.offset = 40;
    radial.local_id = None;
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
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![unrelated, center, another, radial],
    };
    let lanes = [lane];

    let (resolved, radius) = implicit_circle_marker(
        &lanes,
        "feature-native",
        FeatureInputOperandKind::Native(0x83fe),
        0,
        5.0,
    )
    .expect("unique terminal radial pair");

    assert_eq!(resolved.id, "center");
    assert!((radius - 5.0).abs() < 1.0e-12);
}

#[test]
fn declared_entity_handle_uses_one_linked_center_radial_pair() {
    let kind = FeatureInputOperandKind::Native(0x81d5);
    let operand = FeatureInputOperand {
        offset: 100,
        reference_ref: "reference".into(),
        kind,
        entity_index: 0,
        entity_ref: None,
    };
    let class = FeatureInputClass {
        id: "class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 112,
        name: "sgEntHandle".into(),
        role: FeatureInputClassRole::SketchEntity,
    };
    let reference = FeatureInputReference {
        id: operand.reference_ref.clone(),
        parent: "lane".into(),
        feature_ref: Some("feature-native".into()),
        ordinal: 0,
        offset: operand.offset,
        kind,
        class_ref: Some(class.id.clone()),
        object_index: 0,
    };
    let mut center = marker("center", Some([0.010, 0.020]));
    center.offset = 10;
    center.object_index = Some(50);
    center.local_id = Some(49);
    let mut radial = marker("radial", Some([0.013, 0.024]));
    radial.offset = 20;
    radial.object_index = Some(49);
    radial.local_id = Some(0);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: vec![class],
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: vec![reference],
        sketch_entities: vec![center, radial],
    };

    let (resolved, radius) = declared_entity_handle_circular_marker(
        std::slice::from_ref(&lane),
        "feature-native",
        &operand,
        5.0,
    )
    .expect("declared entity-handle circle");

    assert_eq!(resolved.id, "center");
    assert!((radius - 5.0).abs() < 1.0e-12);

    for kind in [SketchInputKind::LineOrCircle, SketchInputKind::Arc] {
        let mut lane = lane.clone();
        lane.sketch_entities[0].kind = kind;
        assert!(declared_entity_handle_circular_marker(
            std::slice::from_ref(&lane),
            "feature-native",
            &operand,
            5.0,
        )
        .is_some());
    }
    let mut invalid_radial = lane.clone();
    invalid_radial.sketch_entities[1].kind = SketchInputKind::Arc;
    assert!(declared_entity_handle_circular_marker(
        std::slice::from_ref(&invalid_radial),
        "feature-native",
        &operand,
        5.0,
    )
    .is_none());

    let mut ambiguous = lane;
    let mut second_center = marker("second-center", Some([0.020, 0.030]));
    second_center.offset = 30;
    second_center.object_index = Some(52);
    second_center.local_id = Some(51);
    let mut second_radial = marker("second-radial", Some([0.023, 0.034]));
    second_radial.offset = 40;
    second_radial.object_index = Some(51);
    second_radial.local_id = Some(0);
    ambiguous
        .sketch_entities
        .extend([second_center, second_radial]);
    assert!(declared_entity_handle_circular_marker(
        std::slice::from_ref(&ambiguous),
        "feature-native",
        &operand,
        5.0,
    )
    .is_none());
}

#[test]
fn nested_profile_must_contain_its_declared_entity_handle_circular_carrier() {
    let sketch_id = SketchId("nested".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    };
    let circle = SketchEntity {
        id: SketchEntityId("circle".into()),
        sketch: sketch_id,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(10.0, 20.0),
            radius: Length(5.0),
        },
    };
    let declared = [([0.010, 0.020], 5.0)];

    assert!(nested_profile_contains_declared_circular_carriers(
        &sketch,
        std::slice::from_ref(&circle),
        &declared,
    ));
    let mut arc = circle;
    arc.geometry = SketchGeometry::Arc {
        center: Point2::new(10.0, 20.0),
        radius: Length(5.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    assert!(nested_profile_contains_declared_circular_carriers(
        &sketch,
        std::slice::from_ref(&arc),
        &declared,
    ));
    assert!(!nested_profile_contains_declared_circular_carriers(
        &sketch,
        &[],
        &declared,
    ));
}

#[test]
fn declared_entity_handle_circular_carrier_replaces_nested_support_geometry() {
    let native_feature = NativeFeature {
        id: "feature-native".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some("7".into()),
        parent_source_id: None,
        ordinal: 7,
        name: "Sketch1".into(),
        kind: String::new(),
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
        features: vec![native_feature],
    };
    let mut features = vec![Feature {
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
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: None,
        },
        native_ref: Some("feature-native".into()),
    }];
    let parameter = DesignParameter {
        id: ParameterId("diameter".into()),
        owner: Some(features[0].id.clone()),
        ordinal: 0,
        name: "diameter".into(),
        expression: "10".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(10.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("parameter-scalar".into()),
    };
    let kind = FeatureInputOperandKind::Native(0x81d5);
    let operand = FeatureInputOperand {
        offset: 300,
        reference_ref: "reference".into(),
        kind,
        entity_index: 0,
        entity_ref: None,
    };
    let class = FeatureInputClass {
        id: "class".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 312,
        name: "sgEntHandle".into(),
        role: FeatureInputClassRole::SketchEntity,
    };
    let mut center = marker("center", Some([0.010, 0.020]));
    center.offset = 400;
    center.object_index = Some(50);
    center.local_id = Some(49);
    let mut radial = marker("radial", Some([0.013, 0.024]));
    radial.offset = 410;
    radial.object_index = Some(49);
    radial.local_id = Some(0);
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: vec![class.clone()],
        names: vec![FeatureInputName {
            id: "feature-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            value: "Sketch1".into(),
            object_id: Some(7),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![FeatureInputRelationInstance {
            id: "circle-dimension".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 280,
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: class.id.clone(),
            feature_ref: "feature-native".into(),
            scalar_refs: Vec::new(),
            parameter_scalar_ref: Some("parameter-scalar".into()),
            display_scalar_ref: None,
            operands: vec![operand.clone()],
        }],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: vec![FeatureInputReference {
            id: operand.reference_ref,
            parent: "lane".into(),
            feature_ref: Some("feature-native".into()),
            ordinal: 0,
            offset: operand.offset,
            kind,
            class_ref: Some(class.id),
            object_index: 0,
        }],
        sketch_entities: vec![center, radial],
    };
    let sketch_id = SketchId("support-sketch".into());
    let entity_id = SketchEntityId("support-entity".into());
    let constraint_id = SketchConstraintId("support-constraint".into());
    let mut sketches = vec![Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: Some("lane".into()),
    }];
    let mut entities = vec![SketchEntity {
        id: entity_id.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    }];
    let mut constraints = vec![SketchConstraint {
        id: constraint_id.clone(),
        sketch: sketch_id.clone(),
        definition: SketchConstraintDefinition::Native {
            native_kind: "endpoint".into(),
            native_state: None,
            native_flags: None,
            native_properties: BTreeMap::new(),
            entities: vec![entity_id.clone()],
            parameter: None,
            operands: Vec::new(),
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    }];
    let mut builder = AnnotationBuilder::new();
    let stream = builder.stream("test:support");
    builder.note(&sketch_id.0, stream, 200).tag("support");
    let mut annotations = builder.build();
    for id in [&sketch_id.0, &entity_id.0, &constraint_id.0] {
        annotations
            .exactness
            .insert(id.clone(), ExactnessNote::default());
    }

    bind_sketch_profiles(
        &mut features,
        &mut sketches,
        &mut entities,
        &mut constraints,
        &[parameter],
        &[history],
        &[lane],
        &mut annotations,
    );

    assert!(sketches.is_empty());
    assert!(entities.is_empty());
    assert!(constraints.is_empty());
    assert!(annotations.provenance.is_empty());
    assert!(annotations.exactness.is_empty());
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
}
