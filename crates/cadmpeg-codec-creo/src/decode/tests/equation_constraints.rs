// SPDX-License-Identifier: Apache-2.0
//! Unit tests for section-equation constraint transfer.

use crate::decode::sketch_transfer::{
    section_equation_equal_distance_constraints, section_equation_point_on_line_constraints,
    section_equation_unsigned_distance_constraints,
};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchDistancePair, SketchEntityId, SketchLocus,
};

#[test]
fn equation_function_thirty_three_emits_equal_distance_pairs() {
    let row = |variable_type, key| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value: Some(0.0),
        value_body: Vec::new(),
        guess: Some(0.0),
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let line = |external_id, point_ids| crate::feature::FeatureSegment {
        kind: crate::feature::FeatureSegmentKind::Line,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x21\xf8\x09\x00\x01\x02\x03\x04\x05\x06\x07\x08\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 9,
            entity_ref: None,
            rows: vec![
                row(1, 1),
                row(2, 1),
                row(1, 2),
                row(2, 2),
                row(1, 3),
                row(2, 3),
                row(1, 4),
                row(2, 4),
                row(7, 0),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![line(10, [1, 2]), line(11, [3, 4])],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_equal_distance_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(
        constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#40:equation:1"
    );
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::EqualDistance {
            first: SketchDistancePair {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#40:10".into(),
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#40:10".into(),
                )),
            },
            second: SketchDistancePair {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#40:11".into(),
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#40:11".into(),
                )),
            },
        }
    );

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_equal_distance_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}

#[test]
fn equation_function_thirty_five_emits_point_on_line() {
    let row = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let segment = |kind, external_id, point_ids| crate::feature::FeatureSegment {
        kind,
        directions: [None; 3],
        point_ids,
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: external_id as usize,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x23\xf8\x09\x00\x01\x02\x03\x04\x05\x06\x07\x08\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 9,
            entity_ref: None,
            rows: vec![
                row(1, 3, None),
                row(2, 3, None),
                row(1, 1, Some(0.0)),
                row(2, 1, Some(0.0)),
                row(1, 2, Some(10.0)),
                row(2, 2, Some(0.0)),
                row(4, 0, Some(0.0)),
                row(5, 0, Some(0.0)),
                row(5, 1, Some(0.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![
                segment(crate::feature::FeatureSegmentKind::Line, 10, [1, 2]),
                segment(crate::feature::FeatureSegmentKind::Point, 12, [3, 3]),
            ],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_point_on_line_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(
        constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#40:equation:1"
    );
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Entity(SketchEntityId("creo:featdefs:sketch_entity#40:12".into(),)),
            entity: SketchEntityId("creo:featdefs:sketch_entity#40:10".into()),
        }
    );

    let mut reference_line_definition = definition.clone();
    let segments = reference_line_definition
        .segments
        .as_mut()
        .expect("segments");
    segments
        .rows
        .retain(|segment| segment.kind == crate::feature::FeatureSegmentKind::Point);
    segments.reference_line_rows = vec![crate::feature::FeatureReferenceLineSegment {
        directions: [None; 3],
        point_ids: [Some(1), Some(2)],
        vertical_horizontal: None,
        external_id: 10,
        offset: 10,
    }];
    let reference_line_constraints =
        section_equation_point_on_line_constraints(&reference_line_definition, &sketch);
    assert_eq!(reference_line_constraints.len(), 1);
    assert_eq!(
        reference_line_constraints[0].0.definition,
        constraints[0].0.definition
    );

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_point_on_line_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}

#[test]
fn equation_function_three_emits_parameterized_coordinate_distance() {
    let variable = |variable_type, key, value| crate::feature::FeatureVariableRow {
        variable_type,
        key,
        value,
        value_body: Vec::new(),
        guess: value,
        guess_body: Vec::new(),
        guess_dimension_driven: false,
        known: Some(0),
        homogeneity: Some(1),
        uvar_id: None,
        dimension_driven: false,
        offset: 0,
    };
    let dimension = crate::feature::FeatureDimension {
        dimension_type: 1,
        value: Some(10.0),
        value_body: Vec::new(),
        unresolved_value_token: None,
        value_unit: crate::feature::DimensionUnit::Millimeters,
        direction_byte: 0,
        auxiliary_value: None,
        auxiliary_body: Vec::new(),
        external_id: 27,
        references: None,
        offset: 0,
    };
    let definition = crate::feature::FeatureDefinition {
        id: 40,
        owner_feature_id: None,
        body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
                \x01\x03\xf8\x03\x00\x01\x02\xf6\xe2"
            .to_vec(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: Some(crate::feature::FeatureVariableTable {
            declared_count: 3,
            entity_ref: None,
            rows: vec![
                variable(1, 1, Some(0.0)),
                variable(1, 2, Some(10.0)),
                variable(0, 0, Some(10.0)),
            ],
            points: Vec::new(),
            offset: 0,
        }),
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 10,
                body: Vec::new(),
                offset: 10,
            }],
            circle_rows: Vec::new(),
            point_rows: Vec::new(),
            centered_line_rows: Vec::new(),
            reference_line_rows: Vec::new(),
            bounded_curve_rows: Vec::new(),
            conic_rows: Vec::new(),
            opaque_rows: Vec::new(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: Some(crate::feature::FeatureDimensionTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![dimension],
            offset: 0,
        }),
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("creo:model:sketch#40".into());
    let constraints = section_equation_unsigned_distance_constraints(&definition, &sketch);
    assert_eq!(constraints.len(), 1);
    assert_eq!(
        constraints[0].0.id.0,
        "creo:featdefs:sketch_constraint#40:equation:1"
    );
    assert_eq!(constraints[0].0.active, Some(true));
    assert_eq!(
        constraints[0].0.definition,
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            second: SketchLocus::End(SketchEntityId("creo:featdefs:sketch_entity#40:10".into(),)),
            parameter: ParameterId("creo:featdefs:parameter#40:27".into()),
        }
    );

    let mut vertical = definition.clone();
    let variables = vertical.variables.as_mut().expect("variables");
    variables.rows[0].variable_type = 2;
    variables.rows[1].variable_type = 2;
    assert!(matches!(
        section_equation_unsigned_distance_constraints(&vertical, &sketch)[0]
            .0
            .definition,
        SketchConstraintDefinition::VerticalDistance { .. }
    ));

    let mut disabled = definition;
    disabled.relations = Some(crate::feature::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: Vec::new(),
        skamps: vec![crate::feature::FeatureSkamp {
            id: 900,
            kind: 0,
            flags: 0,
            status: 0,
            items: Vec::new(),
            offset: 900,
        }],
        skamp_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 901,
            offset: 900,
        }),
        triples: vec![crate::feature::FeatureRelationTriple {
            relation_id: None,
            equation_id: Some(1),
            skamp_id: Some(900),
            offset: 902,
        }],
        triples_header: Some(crate::feature::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 903,
            offset: 902,
        }),
        offset: 899,
    });
    let disabled_constraints = section_equation_unsigned_distance_constraints(&disabled, &sketch);
    assert_eq!(disabled_constraints.len(), 1);
    assert_eq!(disabled_constraints[0].0.active, Some(false));
}
