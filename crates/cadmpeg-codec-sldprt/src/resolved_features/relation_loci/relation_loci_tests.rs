//! Tests for the `relation_loci` module.

use super::{
    marker_center_dimensioned_entity, relation_constraint_is_inactive, typed_relation_definition,
    unique_locus,
};
use crate::records::{
    FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, Length, ParameterId, ParameterValue,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus};
use std::collections::{BTreeMap, HashMap};

#[test]
fn point_operand_requires_one_profile_locus() {
    let entity = SketchEntityId("entity".into());
    let locus = SketchLocus::Start(entity.clone());
    assert_eq!(unique_locus(std::slice::from_ref(&locus)), Some(locus));
    assert_eq!(unique_locus(&[]), None);
    assert_eq!(
        unique_locus(&[SketchLocus::Start(entity.clone()), SketchLocus::End(entity)]),
        None
    );
}

#[test]
fn explicit_point_center_binds_one_matching_dimensioned_curve() {
    let sketch = SketchId("sketch".into());
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>4".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(4.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let center = SketchEntity {
        id: SketchEntityId("center".into()),
        sketch: sketch.clone(),
        construction: true,
        native_ref: Some("center-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    };
    let circle = SketchEntity {
        id: SketchEntityId("circle".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length(2.0),
        },
    };
    let entities = vec![center, circle.clone()];

    assert_eq!(
        marker_center_dimensioned_entity("center-marker", &sketch, &entities, &parameter),
        Some(circle.id.clone())
    );

    let mut ambiguous = entities;
    let mut duplicate = circle;
    duplicate.id = SketchEntityId("duplicate-circle".into());
    ambiguous.push(duplicate);
    assert_eq!(
        marker_center_dimensioned_entity("center-marker", &sketch, &ambiguous, &parameter),
        None
    );
}

#[test]
fn circle_dimension_ignores_marker_resolved_to_line() {
    let sketch = SketchId("sketch".into());
    let line = SketchEntity {
        id: SketchEntityId("line".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some("line-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    };
    let circle = SketchEntity {
        id: SketchEntityId("circle".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    };
    let marker = SketchInputEntity {
        id: "line-marker".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let markers = HashMap::from([(marker.id.as_str(), &marker)]);
    let loci = HashMap::from([(
        marker.id.clone(),
        vec![SketchLocus::Entity(line.id.clone())],
    )]);
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: Vec::new(),
        parameter_scalar_ref: Some("parameter".into()),
        display_scalar_ref: None,
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::E1,
            entity_index: 0,
            entity_ref: Some(marker.id.clone()),
        }],
    };
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>4".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(4.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[line, circle.clone()],
            &markers,
            &loci,
        ),
        Some(cadmpeg_ir::sketches::SketchConstraintDefinition::Diameter {
            entity: circle.id,
            parameter: parameter.id,
        })
    );
}

#[test]
fn repeated_circle_dimension_binds_generated_circles_by_parameter_identity() {
    let sketch = SketchId("sketch".into());
    let circle = |id: &str, center: Point2| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: Some("driver".into()),
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center,
            radius: Length(2.5),
        },
    };
    let entities = vec![
        circle("first", Point2::new(-12.0, -12.0)),
        circle("second", Point2::new(12.0, -12.0)),
    ];
    let relation = FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::CircleDiameter,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["display".into(), "driver".into()],
        parameter_scalar_ref: Some("driver".into()),
        display_scalar_ref: Some("display".into()),
        operands: vec![FeatureInputOperand {
            offset: 0,
            reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::Native(33065),
            entity_index: 1,
            entity_ref: None,
        }],
    };
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>5".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("driver".into()),
    };

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &entities,
            &HashMap::new(),
            &HashMap::new(),
        ),
        Some(cadmpeg_ir::sketches::SketchConstraintDefinition::RepeatedDiameter {
            entities: repeated,
            parameter: parameter_id,
        }) if repeated == vec![SketchEntityId("first".into()), SketchEntityId("second".into())]
            && parameter_id == parameter.id
    ));
}

#[test]
fn repeated_circle_dimension_is_inactive_when_any_radius_differs() {
    let sketch = SketchId("sketch".into());
    let entity = |id: &str, radius| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: Some("driver".into()),
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(radius),
        },
    };
    let entities = vec![entity("first", 2.5), entity("second", 2.5)];
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "<MOD-DIAM>5".into(),
        display: Some(DimensionDisplay::Diameter),
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("driver".into()),
    };
    let definition = cadmpeg_ir::sketches::SketchConstraintDefinition::RepeatedDiameter {
        entities: entities.iter().map(|entity| entity.id.clone()).collect(),
        parameter: parameter.id.clone(),
    };

    assert!(!relation_constraint_is_inactive(
        Some(&parameter),
        &definition,
        &entities,
    ));

    let mut mismatched = entities;
    mismatched[1].geometry = SketchGeometry::Circle {
        center: Point2::new(0.0, 0.0),
        radius: Length(2.0),
    };
    assert!(relation_constraint_is_inactive(
        Some(&parameter),
        &definition,
        &mismatched,
    ));
}
