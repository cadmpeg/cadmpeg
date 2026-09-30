// SPDX-License-Identifier: Apache-2.0

use super::{dynamic_relation, line_entity, marker, typed_relation_definition};
use crate::records::{FeatureInputRelationFamily, SketchInputKind, SketchInputLink};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchId, SketchLocus};
use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
use cadmpeg_ir::scalar::Angle;
use std::collections::{BTreeMap, HashMap};

#[test]
fn dynamic_angle_disambiguates_one_marker_scoped_line_by_angle() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut first_marker = marker("first-marker", 0, 10, SketchInputKind::LineOrCircle, None);
    first_marker.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 0,
                entity_ref: "first-start".into(),
            },
            SketchInputLink {
                local_id: 1,
                entity_ref: "first-end".into(),
            },
        ],
    );
    let second_marker = marker("second-marker", 1, 20, SketchInputKind::LineOrCircle, None);
    let first_start = marker("first-start", 2, 30, SketchInputKind::Point, None);
    let first_end = marker("first-end", 3, 40, SketchInputKind::Point, None);
    let mut first_line = line_entity(
        "synthetic:test:id#first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    first_line.endpoint_refs = vec!["first-start".into(), "first-end".into()];
    let mut alternate_line = line_entity(
        "synthetic:test:id#alternate-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.5, 0.866_025_403_784_438_6),
    );
    alternate_line.endpoint_refs = vec!["first-start".into()];
    let second_line = line_entity(
        "synthetic:test:id#second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 1.0),
    );
    let markers = [first_marker, second_marker, first_start, first_end];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([(
        "second-marker".into(),
        vec![SketchLocus::Entity(second_line.id().clone())],
    )]);
    let mut relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    relation.operands[0].entity_ref = Some("first-marker".into());
    relation.operands[1].entity_ref = Some("second-marker".into());
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "90deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(
            Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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
        Some(SketchConstraintDefinitionInput::Angle {
            first: first_line.id().clone(),
            second: second_line.id().clone(),
            parameter: parameter.id,
        })
    );
}

#[test]
fn dynamic_angle_uses_the_unoriented_solver_line_witness() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut first = line_entity(
        "synthetic:test:id#first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    first.geometry_ref = Some("feature:solver-line:0".into());
    let mut second = line_entity(
        "synthetic:test:id#second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(-1.0, 1.0),
    );
    second.geometry_ref = Some("feature:solver-line:1".into());
    let relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "45deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(
            Angle::new(std::f64::consts::FRAC_PI_4).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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
        Some(SketchConstraintDefinitionInput::Angle { .. })
    ));
}

#[test]
fn dynamic_angle_uses_unique_complete_roster_when_no_line_resolves() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let first = line_entity(
        "synthetic:test:id#first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let second = line_entity(
        "synthetic:test:id#second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 10.0),
    );
    let relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "45deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(
            Angle::new(std::f64::consts::FRAC_PI_4).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[first.clone(), second.clone()],
            &HashMap::new(),
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinitionInput::Angle {
            first: first.id().clone(),
            second: second.id().clone(),
            parameter: parameter.id,
        })
    );
}

#[test]
fn dynamic_angle_repairs_one_resolved_line_from_the_profile_roster() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let known = line_entity(
        "synthetic:test:id#known-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let partner = line_entity(
        "synthetic:test:id#partner-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 10.0),
    );
    let unrelated = line_entity(
        "synthetic:test:id#unrelated-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 10.0),
    );
    let known_marker = marker("known-marker", 0, 10, SketchInputKind::LineOrCircle, None);
    let markers = [known_marker];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([(
        "known-marker".into(),
        vec![SketchLocus::Entity(known.id().clone())],
    )]);
    let mut relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    relation.operands[0].entity_ref = Some("known-marker".into());
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "45deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(
            Angle::new(std::f64::consts::FRAC_PI_4).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&parameter),
            &sketch,
            &[known.clone(), partner.clone(), unrelated],
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinitionInput::Angle {
            first: known.id().clone(),
            second: partner.id().clone(),
            parameter: parameter.id,
        })
    );
}

#[test]
fn dynamic_angle_uses_solver_lines_for_indirect_operand_references() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut first = line_entity(
        "synthetic:test:id#first-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    first.geometry_ref = Some("feature:solver-line:0".into());
    let mut second = line_entity(
        "synthetic:test:id#second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(-1.0, 1.0),
    );
    second.geometry_ref = Some("feature:solver-line:1".into());
    let mut relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    relation.operands[0].entity_ref = Some("indirect-marker".into());
    relation.operands[1].entity_ref = Some("indirect-point".into());
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "45deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(
            Angle::new(std::f64::consts::FRAC_PI_4).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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
        Some(SketchConstraintDefinitionInput::Angle { .. })
    ));
}

#[test]
fn dynamic_angle_prefers_an_explicit_line_over_a_conflicting_solver_alias() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut explicit = line_entity(
        "synthetic:test:id#explicit-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    explicit.native_ref = Some("line-marker".into());
    let mut conflicting = line_entity(
        "synthetic:test:id#conflicting-solver-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 1.0),
    );
    conflicting.geometry_ref = Some("feature:solver-line:0".into());
    let mut second = line_entity(
        "synthetic:test:id#second-line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(0.866_025_403_784_438_6, 0.5),
    );
    second.geometry_ref = Some("feature:solver-line:1".into());
    let mut relation = dynamic_relation(FeatureInputRelationFamily::Angle, [0, 1]);
    relation.operands[0].entity_ref = Some("line-marker".into());
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "30deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(
            Angle::new(std::f64::consts::PI / 6.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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
        Some(SketchConstraintDefinitionInput::Angle { .. })
    ));
}
