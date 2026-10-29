// SPDX-License-Identifier: Apache-2.0
use crate::document::CadIr;
use crate::features::{Dimension, DimensionValue, Expression, ParameterValue};
use crate::scalar::{Angle, Length, PositiveLength};

#[test]
fn literal_dimensions_preserve_quantity_and_positive_admission() {
    let document = CadIr::empty();
    let length = PositiveLength::new(5.0).unwrap();
    assert_eq!(
        document.dimension_value(&Dimension::Literal(length)),
        Some(length)
    );
    assert!(
        PositiveLength::from_parameter_value(&ParameterValue::Length(Length::new(-5.0).unwrap()))
            .is_none()
    );
    assert!(
        PositiveLength::from_parameter_value(&ParameterValue::Angle(Angle::new(5.0).unwrap()))
            .is_none()
    );
}

#[test]
fn canonical_literal_expression_uses_the_same_quantity() {
    let length = ParameterValue::Length(Length::new(37.0).unwrap());
    let expression = Expression::literal(&length);
    assert_eq!(expression.literal_text().unwrap(), "37 mm");
    assert_eq!(expression.literal_value_as(&length), Some(length));
}

fn parameter(
    id: &str,
    owner: crate::features::FeatureId,
    value: f64,
) -> crate::features::DesignParameter {
    crate::features::DesignParameter {
        id: crate::features::ParameterId::mint(id).unwrap(),
        owner: Some(owner),
        ordinal: 0,
        name: "width".into(),
        expression: ParameterValue::Length(Length::new(value).unwrap()).literal_expression(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(Length::new(value).unwrap())),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    }
}
fn primitive(id: &str, ordinal: u64, length: Dimension<Length>) -> crate::features::Feature {
    use crate::features::{
        Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, PrimitiveSolid,
        PrimitiveSolidKind,
    };
    Feature {
        id: FeatureId::mint(id).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Primitive {
                placement: None,
                solid: PrimitiveSolid::new(PrimitiveSolidKind::Box {
                    length,
                    width: crate::features::Dimension::Literal(Length::new(2.0).unwrap()),
                    height: crate::features::Dimension::Literal(Length::new(3.0).unwrap()),
                })
                .unwrap(),
                op: crate::features::BooleanOp::NewBody,
            },
        )),
    }
}

#[test]
fn bound_edit_updates_only_the_parameter_and_invalidates_dependents() {
    use crate::features::{FeatureId, ParameterId};
    let first = FeatureId::mint("test:model:feature#first").unwrap();
    let second = FeatureId::mint("test:model:feature#second").unwrap();
    let parameter_id = ParameterId::mint("test:model:parameter#width").unwrap();
    let mut ir = CadIr::empty();
    ir.model
        .parameters
        .push(parameter(parameter_id.as_str(), first.clone(), 10.0));
    ir.model.features.push(primitive(
        first.as_str(),
        0,
        Dimension::Parameter(parameter_id.clone()),
    ));
    let mut consumer = primitive(second.as_str(), 1, Length::new(5.0).unwrap().into());
    consumer.dependencies = vec![first.clone()].try_into().unwrap();
    ir.model.features.push(consumer);
    let before = ir.model.features[0].definition().clone();
    let path = before
        .dimension_paths()
        .into_iter()
        .find(|path| path.ends_with("/length"))
        .unwrap();
    ir.set_dimension(&first, &path, Length::new(37.0).unwrap())
        .unwrap();
    assert_eq!(ir.model.features[0].definition(), &before);
    assert_eq!(
        ir.model.parameters[0].expression.literal_text().as_deref(),
        Some("37 mm")
    );
    assert_eq!(
        ir.model.parameters[0].value,
        Some(ParameterValue::Length(Length::new(37.0).unwrap()))
    );
    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| feature.needs_recompute));
    assert_eq!(
        ir.dimension_value(&Dimension::<Length>::Parameter(parameter_id)),
        Some(Length::new(37.0).unwrap())
    );
}

#[test]
fn driven_edit_refuses_with_drivers_and_preserves_the_document() {
    use crate::features::{ExpressionToken, FeatureId, ParameterId};
    let owner = FeatureId::mint("test:model:feature#first").unwrap();
    let driver_id = ParameterId::mint("test:model:parameter#driver").unwrap();
    let target_id = ParameterId::mint("test:model:parameter#target").unwrap();
    let mut ir = CadIr::empty();
    ir.model
        .parameters
        .push(parameter(driver_id.as_str(), owner.clone(), 10.0));
    let mut driven = parameter(target_id.as_str(), owner.clone(), 20.0);
    driven.ordinal = 1;
    driven.expression = Expression {
        tokens: vec![
            ExpressionToken::Parameter(driver_id.clone()),
            ExpressionToken::Text(" * 2".into()),
        ],
    };
    ir.model.parameters.push(driven);
    ir.model.features.push(primitive(
        owner.as_str(),
        0,
        Dimension::Parameter(target_id.clone()),
    ));
    let before = ir.clone();
    let path = ir.model.features[0]
        .definition()
        .dimension_paths()
        .into_iter()
        .find(|path| path.ends_with("/length"))
        .unwrap();
    assert_eq!(
        ir.set_dimension(&owner, &path, Length::new(37.0).unwrap()),
        Err(crate::document::DimensionEditError::DrivenParameter {
            parameter: target_id,
            drivers: vec![driver_id]
        })
    );
    assert_eq!(ir, before);
}

#[test]
fn literal_edit_replaces_the_literal_and_rechecks_local_relations() {
    let mut ir = CadIr::empty();
    let feature = primitive(
        "test:model:feature#literal",
        0,
        Length::new(5.0).unwrap().into(),
    );
    let id = feature.id.clone();
    let path = feature
        .definition()
        .dimension_paths()
        .into_iter()
        .find(|path| path.ends_with("/length"))
        .unwrap();
    ir.model.features.push(feature);
    let before = ir.clone();
    assert!(ir
        .set_dimension(&id, &path, Length::new(-1.0).unwrap())
        .is_err());
    assert_eq!(ir, before);
    ir.set_dimension(&id, &path, Length::new(8.0).unwrap())
        .unwrap();
    assert_eq!(
        ir.resolved_feature_definition(ir.model.features[0].definition())
            .unwrap()["solid"]["length"],
        serde_json::json!(8.0)
    );
    assert!(ir.model.features[0].needs_recompute);
}

#[test]
fn dimension_setter_rejects_wrong_quantity_for_literal_and_bound_slots() {
    use crate::features::{FeatureId, ParameterId};
    for bound in [false, true] {
        let owner = FeatureId::mint("test:model:feature#quantity").unwrap();
        let id = ParameterId::mint("test:model:parameter#length").unwrap();
        let mut ir = CadIr::empty();
        let dimension = if bound {
            ir.model
                .parameters
                .push(parameter(id.as_str(), owner.clone(), 5.0));
            Dimension::Parameter(id)
        } else {
            Dimension::Literal(Length::new(5.0).unwrap())
        };
        ir.model
            .features
            .push(primitive(owner.as_str(), 0, dimension));
        let path = ir.model.features[0]
            .definition()
            .dimension_paths()
            .into_iter()
            .find(|path| path.ends_with("/length"))
            .unwrap();
        let before = ir.clone();
        assert!(ir
            .set_dimension(&owner, &path, Angle::new(2.0).unwrap())
            .is_err());
        assert_eq!(ir, before);
    }
}

#[test]
fn reference_parameter_with_literal_cache_is_not_an_editable_driver() {
    use crate::features::{FeatureId, ParameterId};
    let owner = FeatureId::mint("test:model:feature#measured").unwrap();
    let parameter_id = ParameterId::mint("test:model:parameter#measurement").unwrap();
    let mut ir = CadIr::empty();
    let mut measured = parameter(parameter_id.as_str(), owner.clone(), 10.0);
    measured.driving = false;
    ir.model.parameters.push(measured);
    ir.model.features.push(primitive(
        owner.as_str(),
        0,
        Dimension::Parameter(parameter_id.clone()),
    ));
    let path = ir.model.features[0]
        .definition()
        .dimension_paths()
        .into_iter()
        .find(|path| path.ends_with("/length"))
        .unwrap();
    let before = ir.clone();
    let error = ir
        .set_dimension(&owner, &path, Length::new(37.0).unwrap())
        .unwrap_err();
    assert_eq!(
        error,
        super::DimensionEditError::DrivenParameter {
            parameter: parameter_id,
            drivers: Vec::new()
        }
    );
    assert_eq!(ir, before);
}

#[test]
fn sketch_constraint_parameters_invalidate_sketch_consumers_and_dependents() {
    use crate::features::{
        FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, ParameterId,
        SketchFeatureBinding,
    };
    use crate::sketches::{
        SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntityId,
        SketchId, SpatialSketchConstraint, SpatialSketchConstraintDefinitionInput,
        SpatialSketchEntityId, SpatialSketchId,
    };
    for spatial in [false, true] {
        let mut ir = CadIr::empty();
        let parameter_id = ParameterId::mint("test:model:parameter#diameter").unwrap();
        let feature_id = FeatureId::mint("test:model:feature#sketch").unwrap();
        let mut driver = parameter(parameter_id.as_str(), feature_id.clone(), 10.0);
        driver.owner = None;
        ir.model.parameters.push(driver);
        let mut sketch_feature =
            primitive(feature_id.as_str(), 0, Length::new(1.0).unwrap().into());
        let operation = if spatial {
            let sketch = SpatialSketchId::mint("test:model:spatial-sketch#one").unwrap();
            ir.model
                .spatial_sketch_constraints
                .push(SpatialSketchConstraint {
                    id: SketchConstraintId::mint("test:model:constraint#spatial").unwrap(),
                    sketch: sketch.clone(),
                    definition: SpatialSketchConstraintDefinitionInput::LineLength {
                        entity: SpatialSketchEntityId::mint("test:model:spatial-entity#line")
                            .unwrap(),
                        parameter: Dimension::Parameter(parameter_id.clone()),
                    }
                    .try_into()
                    .unwrap(),
                    native_ref: None,
                });
            FeatureOperation::SpatialSketch {
                sketch: Some(sketch),
            }
        } else {
            let sketch = SketchId::mint("test:model:sketch#one").unwrap();
            ir.model.sketch_constraints.push(SketchConstraint {
                id: SketchConstraintId::mint("test:model:constraint#diameter").unwrap(),
                sketch: sketch.clone(),
                definition: SketchConstraintDefinitionInput::Diameter {
                    entity: SketchEntityId::mint("test:model:sketch-entity#circle").unwrap(),
                    parameter: Dimension::Parameter(parameter_id.clone()),
                }
                .try_into()
                .unwrap(),
                name: None,
                driving: Some(true),
                active: Some(true),
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: None,
            });
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch)),
            }
        };
        sketch_feature.evaluation =
            FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation));
        ir.model.features.push(sketch_feature);
        let mut consumer = primitive(
            "test:model:feature#consumer",
            1,
            Length::new(1.0).unwrap().into(),
        );
        consumer.dependencies = vec![feature_id].try_into().unwrap();
        ir.model.features.push(consumer);
        ir.model.features.push(primitive(
            "test:model:feature#unrelated",
            2,
            Length::new(1.0).unwrap().into(),
        ));
        let value = ParameterValue::Length(Length::new(20.0).unwrap());
        ir.set_parameter_expression(&parameter_id, Expression::literal(&value), Some(value))
            .unwrap();
        assert!(ir.model.features[0].needs_recompute);
        assert!(ir.model.features[1].needs_recompute);
        assert!(!ir.model.features[2].needs_recompute);
    }
}
