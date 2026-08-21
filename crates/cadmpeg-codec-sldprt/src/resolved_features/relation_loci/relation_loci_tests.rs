//! Tests for the `relation_loci` module.

use super::{
    marker_center_dimensioned_entity, relation_constraint_is_inactive, typed_relation_definition,
    unique_locus,
};
use crate::records::{
    FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use cadmpeg_ir::features::{
    Angle, DesignParameter, DimensionDisplay, Length, ParameterId, ParameterValue,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus,
};
use std::collections::{BTreeMap, HashMap};

fn marker(
    id: &str,
    ordinal: u32,
    offset: u64,
    kind: SketchInputKind,
    coordinates_m: Option<[f64; 2]>,
) -> SketchInputEntity {
    SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    }
}

fn dynamic_relation(
    family: FeatureInputRelationFamily,
    indices: [u16; 2],
) -> FeatureInputRelationInstance {
    FeatureInputRelationInstance {
        id: "relation".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: indices
            .into_iter()
            .enumerate()
            .map(|(index, entity_index)| FeatureInputOperand {
                offset: index as u64,
                reference_ref: format!("reference-{index}"),
                kind: FeatureInputOperandKind::Native(0x812a),
                entity_index,
                entity_ref: None,
            })
            .collect(),
    }
}

fn length_parameter(value: f64) -> DesignParameter {
    DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: value.to_string(),
        display: None,
        value: Some(ParameterValue::Length(Length(value))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn line_entity(id: &str, sketch: &SketchId, start: Point2, end: Point2) -> SketchEntity {
    SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    }
}

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
fn dynamic_point_line_relation_uses_curve_marker_ordinal() {
    let sketch = SketchId("sketch".into());
    let markers = [
        marker(
            "point-marker",
            0,
            10,
            SketchInputKind::Point,
            Some([5.0, 1.0]),
        ),
        marker(
            "first-curve-marker",
            1,
            20,
            SketchInputKind::LineOrCircle,
            None,
        ),
        marker(
            "second-curve-marker",
            2,
            30,
            SketchInputKind::LineOrCircle,
            None,
        ),
    ];
    let point = SketchEntity {
        id: SketchEntityId("point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(5.0, 1.0),
        },
    };
    let first_line = line_entity(
        "first-line",
        &sketch,
        Point2::new(0.0, 10.0),
        Point2::new(10.0, 10.0),
    );
    let second_line = line_entity(
        "second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([
        (
            "point-marker".into(),
            vec![SketchLocus::Entity(point.id.clone())],
        ),
        (
            "first-curve-marker".into(),
            vec![SketchLocus::Entity(first_line.id.clone())],
        ),
        (
            "second-curve-marker".into(),
            vec![SketchLocus::Entity(second_line.id.clone())],
        ),
    ]);
    let relation = dynamic_relation(FeatureInputRelationFamily::PointLineDistance, [0, 1]);
    let parameter = length_parameter(1.0);
    let definition = typed_relation_definition(
        &relation,
        Some(&parameter),
        &sketch,
        &[point.clone(), first_line.clone(), second_line.clone()],
        &markers_by_id,
        &loci_by_marker,
    );

    assert_eq!(
        definition,
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(point.id),
            second: SketchLocus::Entity(second_line.id),
            parameter: parameter.id,
        })
    );
}

#[test]
fn solver_point_relation_requires_materialized_positions() {
    let sketch = SketchId("sketch".into());
    let mut relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [12, 13]);
    for operand in &mut relation.operands {
        operand.kind = FeatureInputOperandKind::Native(0x8100);
    }
    let parameter = length_parameter(7.0);

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[],
            &HashMap::new(),
            &HashMap::new(),
        ),
        None
    );
}

#[test]
fn point_line_relation_prefers_materialized_point_over_ambiguous_fallback() {
    let sketch = SketchId("sketch".into());
    let markers = [
        marker(
            "point-marker",
            0,
            10,
            SketchInputKind::Point,
            Some([5.0, 1.0]),
        ),
        marker(
            "first-curve-marker",
            1,
            20,
            SketchInputKind::LineOrCircle,
            None,
        ),
        marker(
            "second-curve-marker",
            2,
            30,
            SketchInputKind::LineOrCircle,
            None,
        ),
    ];
    let point = SketchEntity {
        id: SketchEntityId("point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some("point-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(5.0, 1.0),
        },
    };
    let unrelated_point = SketchEntity {
        id: SketchEntityId("unrelated-point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(15.0, 1.0),
        },
    };
    let line = line_entity(
        "second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(20.0, 0.0),
    );
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([(
        "second-curve-marker".into(),
        vec![SketchLocus::Entity(line.id.clone())],
    )]);
    let relation = dynamic_relation(FeatureInputRelationFamily::PointLineDistance, [0, 1]);
    let parameter = length_parameter(1.0);

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[point.clone(), unrelated_point, line.clone()],
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(point.id),
            second: SketchLocus::Entity(line.id),
            parameter: parameter.id,
        })
    );
}

#[test]
fn dynamic_line_relation_requires_exact_curve_dimension() {
    let sketch = SketchId("sketch".into());
    let markers = [
        marker(
            "first-curve-marker",
            0,
            10,
            SketchInputKind::LineOrCircle,
            None,
        ),
        marker(
            "second-curve-marker",
            1,
            20,
            SketchInputKind::LineOrCircle,
            None,
        ),
    ];
    let first_line = line_entity(
        "first-line",
        &sketch,
        Point2::new(0.0, 10.0),
        Point2::new(10.0, 10.0),
    );
    let second_line = line_entity(
        "second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([
        (
            "first-curve-marker".into(),
            vec![SketchLocus::Entity(first_line.id.clone())],
        ),
        (
            "second-curve-marker".into(),
            vec![SketchLocus::Entity(second_line.id.clone())],
        ),
    ]);
    let relation = dynamic_relation(FeatureInputRelationFamily::LineLineDistance, [0, 1]);
    let entities = [first_line.clone(), second_line.clone()];

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(10.0)),
            &sketch,
            &entities,
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinition::Distance {
            entities: vec![first_line.id.clone(), second_line.id.clone()],
            parameter: ParameterId("parameter".into()),
        })
    );
    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(9.0)),
            &sketch,
            &entities,
            &markers_by_id,
            &loci_by_marker,
        ),
        None
    );
}

#[test]
fn dynamic_point_distance_disambiguates_marker_scoped_points_by_distance() {
    let sketch = SketchId("sketch".into());
    let mut first_marker = marker("first-marker", 0, 10, SketchInputKind::Point, None);
    first_marker.object_index = Some(2);
    let mut second_marker = marker("second-marker", 1, 20, SketchInputKind::Point, None);
    second_marker.object_index = Some(3);
    second_marker.links = vec![
        SketchInputLink {
            local_id: 0,
            entity_ref: "second-target".into(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: "second-near".into(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: "second-other".into(),
        },
    ];
    let second_target_marker = marker("second-target", 2, 30, SketchInputKind::Point, None);
    let second_near_marker = marker("second-near", 3, 40, SketchInputKind::Point, None);
    let second_other_marker = marker("second-other", 4, 50, SketchInputKind::Point, None);
    let first_point = SketchEntity {
        id: SketchEntityId("first-point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some("first-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    };
    let first_edge = line_entity(
        "first-edge",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 10.0),
    );
    let second_target = SketchEntity {
        id: SketchEntityId("second-target".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some("second-target".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(140.0, 0.0),
        },
    };
    let second_near = SketchEntity {
        id: SketchEntityId("second-near".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some("second-near".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 1.0),
        },
    };
    let second_other = SketchEntity {
        id: SketchEntityId("second-other".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some("second-other".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 2.0),
        },
    };
    let markers = [
        first_marker,
        second_marker,
        second_target_marker,
        second_near_marker,
        second_other_marker,
    ];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([(
        "first-marker".into(),
        vec![SketchLocus::Start(first_edge.id.clone())],
    )]);
    let entities = vec![
        first_point,
        first_edge,
        second_target.clone(),
        second_near,
        second_other,
    ];
    let first_locus = SketchLocus::Entity(SketchEntityId("first-point".into()));
    let second_locus = SketchLocus::Entity(second_target.id.clone());
    let relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [2, 3]);

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(140.0)),
            &sketch,
            &entities,
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: first_locus.clone(),
            second: second_locus.clone(),
            parameter: ParameterId("parameter".into()),
        })
    );

    let horizontal = dynamic_relation(
        FeatureInputRelationFamily::PointPointHorizontalDistance,
        [2, 3],
    );
    assert_eq!(
        typed_relation_definition(
            &horizontal,
            Some(&length_parameter(140.0)),
            &sketch,
            &entities,
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance {
            first: first_locus.clone(),
            second: second_locus.clone(),
            parameter: ParameterId("parameter".into()),
        })
    );

    let vertical = dynamic_relation(
        FeatureInputRelationFamily::PointPointVerticalDistance,
        [2, 3],
    );
    assert_eq!(
        typed_relation_definition(
            &vertical,
            Some(&length_parameter(0.0)),
            &sketch,
            &entities,
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinition::VerticalDistance {
            first: first_locus,
            second: second_locus,
            parameter: ParameterId("parameter".into()),
        })
    );
}

#[test]
fn dynamic_point_distance_uses_a_unique_arc_center_carrier() {
    let sketch = SketchId("sketch".into());
    let mut point_marker = marker("point-marker", 0, 10, SketchInputKind::Point, None);
    point_marker.object_index = Some(0);
    let mut arc_marker = marker("arc-marker", 1, 20, SketchInputKind::Arc, None);
    arc_marker.object_index = Some(4);
    let mut wrapper_marker = marker(
        "arc-wrapper",
        2,
        30,
        SketchInputKind::Relation(SketchRelationKind::Distance),
        None,
    );
    wrapper_marker.object_index = Some(4);
    wrapper_marker.links = vec![SketchInputLink {
        local_id: 0,
        entity_ref: arc_marker.id.clone(),
    }];
    let point = SketchEntity {
        id: SketchEntityId("point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(point_marker.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    };
    let arc = SketchEntity {
        id: SketchEntityId("arc".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Arc {
            center: Point2::new(0.0, 52.0),
            radius: Length(10.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
    };
    let markers = [point_marker, arc_marker, wrapper_marker];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([(
        "arc-marker".into(),
        vec![SketchLocus::Entity(arc.id.clone())],
    )]);
    let relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 4]);
    let parameter = length_parameter(52.0);

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[point.clone(), arc.clone()],
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(point.id),
            second: SketchLocus::Center(arc.id),
            parameter: parameter.id,
        })
    );
}

#[test]
fn dynamic_point_distance_rejects_ambiguous_arc_centers() {
    let sketch = SketchId("sketch".into());
    let mut point_marker = marker("point-marker", 0, 10, SketchInputKind::Point, None);
    point_marker.object_index = Some(0);
    let first_marker = marker("first-arc", 1, 20, SketchInputKind::Arc, None);
    let second_marker = marker("second-arc", 2, 30, SketchInputKind::Arc, None);
    let mut wrapper_marker = marker(
        "arc-wrapper",
        3,
        40,
        SketchInputKind::Relation(SketchRelationKind::Distance),
        None,
    );
    wrapper_marker.object_index = Some(4);
    wrapper_marker.links = vec![
        SketchInputLink {
            local_id: 0,
            entity_ref: first_marker.id.clone(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: second_marker.id.clone(),
        },
    ];
    let point = SketchEntity {
        id: SketchEntityId("point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some(point_marker.id.clone()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    };
    let first_arc = SketchEntity {
        id: SketchEntityId("first-arc-entity".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Arc {
            center: Point2::new(0.0, 52.0),
            radius: Length(10.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
    };
    let second_arc = SketchEntity {
        id: SketchEntityId("second-arc-entity".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Arc {
            center: Point2::new(0.0, 53.0),
            radius: Length(10.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
    };
    let markers = [point_marker, first_marker, second_marker, wrapper_marker];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([
        (
            "first-arc".into(),
            vec![SketchLocus::Entity(first_arc.id.clone())],
        ),
        (
            "second-arc".into(),
            vec![SketchLocus::Entity(second_arc.id.clone())],
        ),
    ]);
    let relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 4]);

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(52.0)),
            &sketch,
            &[point, first_arc, second_arc],
            &markers_by_id,
            &loci_by_marker,
        ),
        None
    );
}

#[test]
fn dynamic_point_line_relation_disambiguates_marker_scoped_lines_by_distance() {
    let sketch = SketchId("sketch".into());
    let point_marker = marker("point-marker", 0, 10, SketchInputKind::Point, None);
    let mut line_marker = marker("line-marker", 1, 20, SketchInputKind::LineOrCircle, None);
    line_marker.object_index = Some(1);
    line_marker.links = vec![
        SketchInputLink {
            local_id: 0,
            entity_ref: "line-start".into(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: "line-end".into(),
        },
    ];
    let line_start = marker("line-start", 2, 30, SketchInputKind::Point, None);
    let line_end = marker("line-end", 3, 40, SketchInputKind::Point, None);
    let point = SketchEntity {
        id: SketchEntityId("point".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: Some("point-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(5.0, 3.0),
        },
    };
    let mut first_line = line_entity(
        "first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    first_line.endpoint_refs = vec!["line-start".into(), "line-end".into()];
    let mut alternate_line = line_entity(
        "alternate-line",
        &sketch,
        Point2::new(0.0, 1.0),
        Point2::new(10.0, 1.0),
    );
    alternate_line.endpoint_refs = vec!["line-start".into()];
    let markers = [point_marker, line_marker, line_start, line_end];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let relation = dynamic_relation(FeatureInputRelationFamily::PointLineDistance, [0, 1]);

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(2.0)),
            &sketch,
            &[point.clone(), first_line, alternate_line.clone()],
            &markers_by_id,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(point.id),
            second: SketchLocus::Entity(alternate_line.id),
            parameter: ParameterId("parameter".into()),
        })
    );
}

#[test]
fn dynamic_line_distance_disambiguates_two_marker_scoped_line_sets() {
    let sketch = SketchId("sketch".into());
    let mut first_marker = marker("first-marker", 0, 10, SketchInputKind::LineOrCircle, None);
    first_marker.links = vec![
        SketchInputLink {
            local_id: 0,
            entity_ref: "first-start".into(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: "first-end".into(),
        },
    ];
    let mut second_marker = marker("second-marker", 1, 20, SketchInputKind::LineOrCircle, None);
    second_marker.links = vec![
        SketchInputLink {
            local_id: 2,
            entity_ref: "second-start".into(),
        },
        SketchInputLink {
            local_id: 3,
            entity_ref: "second-end".into(),
        },
    ];
    let first_start = marker("first-start", 2, 30, SketchInputKind::Point, None);
    let first_end = marker("first-end", 3, 40, SketchInputKind::Point, None);
    let second_start = marker("second-start", 4, 50, SketchInputKind::Point, None);
    let second_end = marker("second-end", 5, 60, SketchInputKind::Point, None);
    let mut first_line = line_entity(
        "first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    first_line.endpoint_refs = vec!["first-start".into(), "first-end".into()];
    let mut first_alternate = line_entity(
        "first-alternate",
        &sketch,
        Point2::new(0.0, 1.0),
        Point2::new(10.0, 1.0),
    );
    first_alternate.endpoint_refs = vec!["first-start".into()];
    let mut second_line = line_entity(
        "second-line",
        &sketch,
        Point2::new(0.0, 3.0),
        Point2::new(10.0, 3.0),
    );
    second_line.endpoint_refs = vec!["second-start".into(), "second-end".into()];
    let mut second_alternate = line_entity(
        "second-alternate",
        &sketch,
        Point2::new(0.0, 4.0),
        Point2::new(10.0, 4.0),
    );
    second_alternate.endpoint_refs = vec!["second-start".into()];
    let markers = [
        first_marker,
        second_marker,
        first_start,
        first_end,
        second_start,
        second_end,
    ];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let relation = dynamic_relation(FeatureInputRelationFamily::LineLineDistance, [0, 1]);
    let entities = vec![
        first_line,
        first_alternate.clone(),
        second_line.clone(),
        second_alternate,
    ];

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(2.0)),
            &sketch,
            &entities,
            &markers_by_id,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::Distance {
            entities: vec![first_alternate.id, second_line.id],
            parameter: ParameterId("parameter".into()),
        })
    );
    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(3.0)),
            &sketch,
            &entities,
            &markers_by_id,
            &HashMap::new(),
        ),
        None
    );
}

#[test]
fn dynamic_angle_disambiguates_one_marker_scoped_line_by_angle() {
    let sketch = SketchId("sketch".into());
    let mut first_marker = marker("first-marker", 0, 10, SketchInputKind::LineOrCircle, None);
    first_marker.links = vec![
        SketchInputLink {
            local_id: 0,
            entity_ref: "first-start".into(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: "first-end".into(),
        },
    ];
    let second_marker = marker("second-marker", 1, 20, SketchInputKind::LineOrCircle, None);
    let first_start = marker("first-start", 2, 30, SketchInputKind::Point, None);
    let first_end = marker("first-end", 3, 40, SketchInputKind::Point, None);
    let mut first_line = line_entity(
        "first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    first_line.endpoint_refs = vec!["first-start".into(), "first-end".into()];
    let mut alternate_line = line_entity(
        "alternate-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.5, 0.866_025_403_784_438_6),
    );
    alternate_line.endpoint_refs = vec!["first-start".into()];
    let second_line = line_entity(
        "second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 1.0),
    );
    let markers = [first_marker, second_marker, first_start, first_end];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([(
        "second-marker".into(),
        vec![SketchLocus::Entity(second_line.id.clone())],
    )]);
    let mut relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    relation.operands[0].entity_ref = Some("first-marker".into());
    relation.operands[1].entity_ref = Some("second-marker".into());
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "90deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2))),
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
            &[first_line.clone(), alternate_line, second_line.clone()],
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinition::Angle {
            first: first_line.id,
            second: second_line.id,
            parameter: parameter.id,
        })
    );
}

#[test]
fn dynamic_angle_uses_the_unoriented_solver_line_witness() {
    let sketch = SketchId("sketch".into());
    let mut first = line_entity(
        "first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    first.geometry_ref = Some("feature:solver-line:0".into());
    let mut second = line_entity(
        "second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(-1.0, 1.0),
    );
    second.geometry_ref = Some("feature:solver-line:1".into());
    let relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "45deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_4))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[first, second],
            &HashMap::new(),
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::Angle { .. })
    ));
}

#[test]
fn dynamic_angle_uses_solver_lines_for_indirect_operand_references() {
    let sketch = SketchId("sketch".into());
    let mut first = line_entity(
        "first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    first.geometry_ref = Some("feature:solver-line:0".into());
    let mut second = line_entity(
        "second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(-1.0, 1.0),
    );
    second.geometry_ref = Some("feature:solver-line:1".into());
    let mut relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    relation.operands[0].entity_ref = Some("indirect-marker".into());
    relation.operands[1].entity_ref = Some("indirect-point".into());
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "45deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_4))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[first, second],
            &HashMap::new(),
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::Angle { .. })
    ));
}

#[test]
fn dynamic_angle_prefers_an_explicit_line_over_a_conflicting_solver_alias() {
    let sketch = SketchId("sketch".into());
    let mut explicit = line_entity(
        "explicit-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    explicit.native_ref = Some("line-marker".into());
    let mut conflicting = line_entity(
        "conflicting-solver-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 1.0),
    );
    conflicting.geometry_ref = Some("feature:solver-line:0".into());
    let mut second = line_entity(
        "second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.866_025_403_784_438_6, 0.5),
    );
    second.geometry_ref = Some("feature:solver-line:1".into());
    let mut relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    relation.operands[0].entity_ref = Some("line-marker".into());
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "30deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(Angle(std::f64::consts::PI / 6.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };

    assert!(matches!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[explicit, conflicting, second],
            &HashMap::new(),
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::Angle { .. })
    ));
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
