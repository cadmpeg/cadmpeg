// SPDX-License-Identifier: Apache-2.0

use super::super::{declared_solver_rows, synchronize_segment_count};
use super::fixtures::base_definition;
use crate::decode::sketch::radii::resolved_section_radii;
use crate::decode::sketch_transfer::constraints::section_dimension_constraints;
use crate::feature::definitions::test_support::with_points;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntityId, SketchId};
use std::collections::BTreeMap;

#[test]
fn section_solver_equal_radius_requires_active_agreeing_sources() {
    let definition = base_definition();
    let mut equal_radius_definition = definition.clone();
    equal_radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| {
            rows[1].radius_ref = Some(101);
            rows[4].radius_ref = Some(102);
        });
    equal_radius_definition.variables = Some(with_points(
        crate::feature::definitions::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 89,
        },
        vec![
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 2,
                u: Some(3.0),
                v: Some(0.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 4,
                u: Some(0.0),
                v: Some(0.0),
            },
        ],
    ));
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
            ctx,
            &equal_radius_definition
        ))
        .expect("test section solve"),
        BTreeMap::from([(101, 3.0), (102, 3.0)])
    );
    let mut disabled_equal_radius = equal_radius_definition.clone();
    disabled_equal_radius
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()
        .iter_mut()
        .find(|skamp| skamp.kind == 6)
        .expect("equal-radius incidence")
        .status = 34;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
            ctx,
            &disabled_equal_radius
        ))
        .expect("test section solve"),
        BTreeMap::from([(101, 3.0)])
    );
    equal_radius_definition
        .variables
        .as_mut()
        .expect("variables")
        .rows
        .push(crate::feature::definitions::FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::Radius,
            key: 102,
            value: crate::feature::definitions::ScalarLane::Value(4.0),
            value_body: Vec::new(),
            guess: crate::feature::definitions::ScalarLane::Undefined,
            guess_body: Vec::new(),

            known: None,
            homogeneity: None,
            uvar_id: None,

            offset: 91,
        });
    equal_radius_definition
        .variables
        .as_mut()
        .expect("variables")
        .declared_count += 1;
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
            ctx,
            &equal_radius_definition
        ))
        .expect("test section solve")
        .is_empty()
    );
    let mut saved_radius_definition = definition.clone();
    saved_radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| rows[1].radius_ref = Some(101));
    saved_radius_definition.order_table = Some(crate::feature::definitions::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::definitions::FeatureOrderRow {
            external_id: 99,
            internal_id: 20,
            bitmask: 1,
            offset: 92,
        }]
        .into(),
        offset: 91,
    });
    saved_radius_definition.saved_section =
        Some(crate::feature::definitions::FeatureSavedSection {
            entities: vec![crate::feature::definitions::FeatureSavedEntity::Circle(
                crate::feature::definitions::FeatureSavedCircle {
                    entity_id: 20,
                    center: [Some(0.0), Some(0.0), Some(0.0)],
                    radius: Some(4.0),
                    body: Vec::new(),
                    offset: 93,
                },
            )],
            offset: 93,
        });
    *declared_solver_rows(
        &mut saved_radius_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    ) = vec![crate::feature::definitions::FeatureSkamp {
        id: 18,
        kind: 6,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 99,
                sense: 0,
            },
        ],
        offset: 94,
    }];
    saved_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
            ctx,
            &saved_radius_definition
        ))
        .expect("test section solve"),
        BTreeMap::from([(101, 4.0)])
    );
}

#[test]
fn section_solver_radius_dimensions_require_circular_unique_semantics() {
    let definition = base_definition();
    let mut legacy_radius_definition = definition.clone();
    let ([first_point, second_point], center) = legacy_radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| {
            let legacy_arc = &mut rows[1];
            legacy_arc.radius_ref = Some(0);
            (
                legacy_arc.point_ids(),
                legacy_arc.center_id.expect("arc center"),
            )
        });
    let legacy_radius_relation = &mut legacy_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    legacy_radius_relation.relation_type = 5;
    legacy_radius_relation.sign = 1;
    legacy_radius_relation.dimension_id = 0;
    legacy_radius_relation.operand_vectors = Some([
        [Some(first_point), Some(0), Some(second_point), Some(0)],
        [Some(center), Some(10), Some(0), Some(1)],
        [Some(16), Some(15), Some(0), Some(0)],
    ]);
    legacy_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 3;
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &legacy_radius_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Radius {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
            ctx,
            &legacy_radius_definition
        ))
        .expect("test section solve"),
        BTreeMap::from([(0, 3.0)])
    );
    legacy_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &legacy_radius_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Diameter {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
            ctx,
            &legacy_radius_definition
        ))
        .expect("test section solve"),
        BTreeMap::from([(0, 1.5)])
    );
    for dimension_type in [1, 2, 5] {
        legacy_radius_definition
            .dimensions
            .as_mut()
            .expect("dimensions")
            .rows[0]
            .dimension_type = dimension_type;
        assert_eq!(
            *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
                ctx,
                &legacy_radius_definition,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            ))
            .expect("test section solve")[0]
                .0
                .definition)
                .kind(),
            SketchConstraintDefinitionInput::Radius {
                entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture"),
                parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                    .expect("identity grammar"),
            }
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
                ctx,
                &legacy_radius_definition
            ))
            .expect("test section solve"),
            BTreeMap::from([(0, 3.0)])
        );
    }
    legacy_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
        [Some(first_point), Some(0), Some(second_point), Some(0)],
        [Some(center), Some(10), Some(0), Some(1)],
        [Some(16), Some(15), Some(0), Some(1)],
    ]);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &legacy_radius_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:relation:5"
    ));

    let mut type6_radius_definition = definition.clone();
    let ([type6_first_point, type6_second_point], type6_center) = type6_radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| {
            let type6_arc = &mut rows[1];
            type6_arc.radius_ref = Some(0);
            (
                type6_arc.point_ids(),
                type6_arc.center_id.expect("arc center"),
            )
        });
    let type6_relation = &mut type6_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    type6_relation.relation_type = 6;
    type6_relation.sign = 1;
    type6_relation.dimension_id = 0;
    type6_relation.operand_vectors = Some([
        [
            Some(type6_first_point),
            Some(type6_second_point),
            Some(0),
            Some(1),
        ],
        [Some(type6_center), Some(0), Some(0), Some(0)],
        [Some(16), Some(15), Some(0), Some(0)],
    ]);
    type6_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 3;
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &type6_radius_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Radius {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
            ctx,
            &type6_radius_definition
        ))
        .expect("test section solve"),
        BTreeMap::from([(0, 3.0)])
    );
    type6_radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0]
        .operand_vectors = Some([
        [
            Some(type6_first_point),
            Some(type6_second_point),
            Some(0),
            Some(1),
        ],
        [Some(type6_center), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &type6_radius_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Radius {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    type6_radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &type6_radius_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Diameter {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
            ctx,
            &type6_radius_definition
        ))
        .expect("test section solve"),
        BTreeMap::from([(0, 1.5)])
    );
    let mut incomplete_type6 = type6_radius_definition.clone();
    incomplete_type6.relations.as_mut().expect("relations").rows[0].operand_vectors = Some([
        [
            Some(type6_first_point),
            Some(type6_second_point),
            Some(0),
            Some(1),
        ],
        [Some(type6_center), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), None],
    ]);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &incomplete_type6,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:relation:6"
    ));
    let mut ambiguous_type6 = type6_radius_definition.clone();
    let mut duplicate_type6_arc = ambiguous_type6
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[1]
        .clone();
    duplicate_type6_arc.offset = 500;
    ambiguous_type6
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
            duplicate_type6_arc,
        ));
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &ambiguous_type6,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:relation:6"
    ));

    let mut radius_definition = definition.clone();
    radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 3;
    radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| rows[1].radius_ref = Some(101));
    let radius_relation = &mut radius_definition
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    radius_relation.relation_type = 14;
    radius_relation.sign = 1;
    radius_relation.dimension_id = 0;
    radius_relation.operand_vectors = Some([
        [Some(101), Some(0), Some(0), Some(0)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(0), Some(0), Some(0)],
    ]);
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &radius_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Radius {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    let mut noncircular_dimension = radius_definition.clone();
    for dimension_type in [1, 2, 5] {
        noncircular_dimension
            .dimensions
            .as_mut()
            .expect("dimensions")
            .rows[0]
            .dimension_type = dimension_type;
        assert_eq!(
            *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
                ctx,
                &noncircular_dimension,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            ))
            .expect("test section solve")[0]
                .0
                .definition)
                .kind(),
            SketchConstraintDefinitionInput::Radius {
                entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture"),
                parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                    .expect("identity grammar"),
            }
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(
                ctx,
                &noncircular_dimension
            ))
            .expect("test section solve"),
            BTreeMap::from([(101, 3.0)])
        );
    }
    let mut opaque_circle_definition = radius_definition.clone();
    let segments = opaque_circle_definition
        .segments
        .as_mut()
        .expect("segments");
    let arc = segments.rows.edit_ordinary(|rows| rows.remove(1));
    segments
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Circle(
            crate::feature::definitions::FeatureCircleSegment {
                center_id: arc.center_id.expect("arc center"),
                radius_ref: arc.radius_ref.expect("arc radius"),
                external_id: arc.external_id,
                offset: arc.offset,
            },
        ));
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &opaque_circle_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Radius {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    let mut incomplete_radius_segments = radius_definition.clone();
    incomplete_radius_segments
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &incomplete_radius_segments,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Radius {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    let mut relation_orientation = definition.clone();
    relation_orientation
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .value = crate::feature::definitions::DimensionValue::Resolved(0.0);
    relation_orientation
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 0;
    let relation = relation_orientation.relations.as_mut().expect("relations");
    relation.rows[0].relation_type = 0;
    relation.rows[0].operand_vectors = Some([
        [Some(1), Some(2), None, Some(1)],
        [Some(0), Some(0), Some(0), Some(0)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    relation_orientation
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| rows[0].vertical_horizontal = Some(1));
    relation.triples = Some(crate::feature::definitions::SolverSubtable::Declared {
        header: crate::feature::definitions::FeatureSolverTableHeader {
            declared_count: 1,
            entity_ref: 2,
            offset: 94,
        },
        rows: vec![crate::feature::definitions::FeatureRelationTriple {
            relation_id: Some(8),
            equation_id: None,
            skamp_id: Some(3),
            offset: 94,
        }],
    });

    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &relation_orientation,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Horizontal {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    let mut incomplete_relation_orientation = relation_orientation.clone();
    incomplete_relation_orientation
        .segments
        .as_mut()
        .expect("segments")
        .declared_count += 1;
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &incomplete_relation_orientation,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Horizontal {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 4;
    assert_eq!(
        *(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(
            ctx,
            &radius_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        ))
        .expect("test section solve")[0]
            .0
            .definition)
            .kind(),
        SketchConstraintDefinitionInput::Diameter {
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string())
                .expect("identity grammar"),
        }
    );
    radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 2;
    let duplicate = radius_definition
        .segments
        .as_ref()
        .expect("segments")
        .rows
        .ordinary()
        .cloned()
        .collect::<Vec<_>>()[1]
        .clone();
    radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
            duplicate,
        ));
    synchronize_segment_count(&mut radius_definition);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx, &radius_definition, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))).expect("test section solve")[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:relation:14"
    ));
    radius_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(Vec::pop);
    synchronize_segment_count(&mut radius_definition);
    radius_definition
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0]
        .dimension_type = 10;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx, &radius_definition, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))).expect("test section solve")[0]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ..
        } if native_kind.as_str() == "creo:relation:14"
    ));
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| resolved_section_radii(ctx, &radius_definition))
            .expect("test section solve")
            .contains_key(&101)
    );
}
