//! Tests for the `relation_loci` module.

use super::{marker_center_dimensioned_entity, unique_locus};
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, Length, ParameterId, ParameterValue,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus};
use std::collections::BTreeMap;

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
