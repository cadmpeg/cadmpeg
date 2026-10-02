// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntityId, SketchId, SketchLocus, SketchNativeOperand};
use cadmpeg_ir::features::{ParameterId};
use super::super::{declared_solver_rows};
use super::fixtures::{base_definition};
use crate::decode::sketch_transfer::constraints::{joined_relation_incidence, relation_incidence, section_dimension_constraints};

#[test]
fn section_solver_relation_incidence_and_angular_dimensions_require_complete_joins() {
    let definition = base_definition();
    let mut distance_definition = super::fixtures::distance_definition(&definition);
    declared_solver_rows(&mut distance_definition.relations.as_mut().expect("relations").skamps).clear();
    let mut incidence_distance = distance_definition.clone();
    let incidence_relations = incidence_distance.relations.as_mut().expect("relations");
    incidence_relations.rows[0].operand_vectors = None;
    incidence_relations.skamps = Some(crate::feature::definitions::SolverSubtable::Declared { header: crate::feature::definitions::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 1,
        offset: 80,
    }, rows: vec![crate::feature::definitions::FeatureSkamp {
        id: 81,
        kind: 0,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
        ],
        offset: 81,
    }] });
    incidence_relations.triples = Some(crate::feature::definitions::SolverSubtable::Declared { header: crate::feature::definitions::FeatureSolverTableHeader {
        declared_count: 1,
        entity_ref: 2,
        offset: 82,
    }, rows: vec![crate::feature::definitions::FeatureRelationTriple {
        relation_id: Some(8),
        equation_id: None,
        skamp_id: Some(81),
        offset: 82,
    }] });


    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &incidence_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::DistanceLoci {
            first: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:12".to_string(),
            ).expect("valid test fixture")),
            second: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:13".to_string(),
            ).expect("valid test fixture")),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string()).expect("identity grammar"),
        }
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &incidence_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .active,
        Some(true)
    );
    let mut inactive_incidence = incidence_distance.clone();
    inactive_incidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .status = 2;
    assert!(joined_relation_incidence(&inactive_incidence, 8).is_some());
    assert!(relation_incidence(&inactive_incidence, 8).is_none());
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &inactive_incidence,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .active,
        Some(false)
    );
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &inactive_incidence,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::DistanceLoci { .. }
    ));
    inactive_incidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .items
        .push(crate::feature::definitions::FeatureSkampItem {
            entity_id: 15,
            sense: 2,
        });
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &inactive_incidence,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::Distance {
            entities: [12, 13, 15]
                .map(|entity_id| {
                    SketchEntityId::mint(format!("creo:featdefs:sketch_entity#917:{entity_id}")).expect("valid test fixture")
                })
                .to_vec(),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string()).expect("identity grammar"),
        }
    );
    let mut incomplete_triples = incidence_distance.clone();
    *incomplete_triples
        .relations
        .as_mut()
        .expect("relations")
        .triples.as_mut().and_then(|table| table.header_mut()).expect("triples header") = crate::feature::definitions::FeatureSolverTableHeader {
        declared_count: 2,
        entity_ref: 2,
        offset: 82,
    };
    assert!(relation_incidence(&incomplete_triples, 8).is_none());
    let mut duplicate_join = incidence_distance.clone();
    let duplicate_relations = duplicate_join.relations.as_mut().expect("relations");
    let duplicate = crate::feature::definitions::FeatureRelationTriple { offset: 83, ..duplicate_relations.triples()[0].clone() };
    declared_solver_rows(&mut duplicate_relations.triples).push(duplicate);
    duplicate_relations
        .triples
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("triples header")
        .declared_count = 2;
    assert!(relation_incidence(&duplicate_join, 8).is_none());
    let mut null_join = incidence_distance.clone();
    let null_relations = null_join.relations.as_mut().expect("relations");
    declared_solver_rows(&mut null_relations.triples).push(crate::feature::definitions::FeatureRelationTriple {
            relation_id: Some(8),
            equation_id: None,
            skamp_id: None,
            offset: 83,
        });
    null_relations
        .triples
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("triples header")
        .declared_count = 2;
    assert_eq!(
        relation_incidence(&null_join, 8).map(|row| row.id),
        Some(81)
    );
    incidence_distance
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .items[0]
        .sense = 2;
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &incidence_distance,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::DistanceLoci {
            first: SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:12".to_string(),
            ).expect("valid test fixture")),
            second: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:13".to_string(),
            ).expect("valid test fixture")),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string()).expect("identity grammar"),
        }
    );
    let mut solver_only_incidence = incidence_distance.clone();
    solver_only_incidence
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[0]
        .items[1]
        .entity_id = 999;
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &solver_only_incidence,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::DistanceLoci {
            first: SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:12".to_string(),
            ).expect("valid test fixture")),
            second: SketchLocus::Entity(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:999".to_string(),
            ).expect("valid test fixture")),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string()).expect("identity grammar"),
        }
    );
    let mut angular_dimension = definition.clone();
    angular_dimension.segments.as_mut().expect("segments").rows.edit_ordinary(|rows| {
        let second_angular_line = &mut rows[1];
        second_angular_line.kind = crate::feature::definitions::FeatureSegmentKind::Line(second_angular_line.point_ids());
        second_angular_line.center_id = None;
        second_angular_line.radius_ref = None;
    });
    let angle_dimension = &mut angular_dimension
        .dimensions
        .as_mut()
        .expect("dimensions")
        .rows[0];
    angle_dimension.dimension_type = 10;
    let angle_relation = &mut angular_dimension
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    angle_relation.relation_type = 1;
    angle_relation.operand_vectors = Some([
        [Some(4), Some(5), None, Some(1)],
        [Some(1), None, Some(1), Some(1)],
        [Some(15), Some(16), Some(15), Some(24)],
    ]);
    angular_dimension.order_table = Some(crate::feature::definitions::FeatureOrderTable {
        declared_count: 2,
        has_prototype: false,
        entity_ref: None,
        rows: vec![
            crate::feature::definitions::FeatureOrderRow {
                external_id: 12,
                internal_id: 4,
                bitmask: 1,
                offset: 90,
            },
            crate::feature::definitions::FeatureOrderRow {
                external_id: 13,
                internal_id: 5,
                bitmask: 1,
                offset: 91,
            },
        ],
        offset: 89,
    });
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &angular_dimension,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::Angle {
            first: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string()).expect("valid test fixture"),
            second: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string()).expect("valid test fixture"),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string()).expect("identity grammar"),
        }
    );
    let mut incomplete_angle_order = angular_dimension.clone();
    incomplete_angle_order
        .order_table
        .as_mut()
        .expect("order table")
        .declared_count = 3;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &incomplete_angle_order,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )).expect("test section solve")[0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut ambiguous_angle = angular_dimension.clone();
    ambiguous_angle
        .order_table
        .as_mut()
        .expect("order table")
        .rows
        .push(crate::feature::definitions::FeatureOrderRow {
            external_id: 13,
            internal_id: 5,
            bitmask: 1,
            offset: 92,
        });
    ambiguous_angle
        .order_table
        .as_mut()
        .expect("order table")
        .declared_count = 3;
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx, &ambiguous_angle, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))).expect("test section solve")
            [0]
        .0
        .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let relations =
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx, &definition, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))).expect("test section solve");
    assert_eq!(*(relations[0].0.definition).kind(),
        SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_core::text::NonBlankString::new("creo:relation:99").expect("nonempty native kind"),
            native_state: Some(1),
            native_flags: None,
            native_properties: std::collections::BTreeMap::from([
                ("dimension_id".to_string(), "0".to_string()),
                ("sign".to_string(), "1".to_string()),
            ]),
            entities: Vec::new(),
            parameter: Some(ParameterId::mint("creo:featdefs:parameter#917:42".to_string(),).expect("identity grammar")),
            operands: vec![SketchNativeOperand {
                native_kind: cadmpeg_core::text::NonBlankString::new("relat_ptr").expect("source operand kind is nonempty"),
                field: None,
                object_index: Some(8),
                native_ref: Some("creo:featdefs:sketch#917".to_string()),
            }],
        }
    );
    let mut vector_native = definition.clone();
    vector_native.relations.as_mut().expect("relations").rows[0].operand_vectors = Some([
        [Some(12), None, Some(0), Some(1)],
        [None; 4],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    let vector_relation =
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx, &vector_native, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))).expect("test section solve");
    let SketchConstraintDefinitionInput::Native {
        native_properties,
        operands,
        ..
    } = vector_relation[0].0.definition.kind()
    else {
        panic!("native relation");
    };
    assert_eq!(
        native_properties,
        &std::collections::BTreeMap::from([
            ("dimension_id".to_string(), "0".to_string()),
            ("sign".to_string(), "1".to_string()),
        ])
    );
    assert_eq!(
        operands
            .iter()
            .map(|operand| (operand.field.as_ref().map(|field| field.name.as_str()), operand.object_index))
            .collect::<Vec<_>>(),
        vec![
            (None, Some(8)),
            (Some("a[0]"), Some(12)),
            (Some("a[2]"), Some(0)),
            (Some("a[3]"), Some(1)),
            (Some("c[0]"), Some(15)),
            (Some("c[1]"), Some(16)),
            (Some("c[2]"), Some(15)),
            (Some("c[3]"), Some(1)),
        ]
    );
    let mut special_endpoint_dimension = definition.clone();
    let special_segments = special_endpoint_dimension
        .segments
        .as_mut()
        .expect("segments");
    special_segments.rows.edit_ordinary(Vec::clear);
    special_segments.rows.edit_circles(Vec::clear);
    special_segments.rows.edit_points(Vec::clear);
    special_segments.rows.edit_centered_lines(Vec::clear);
    special_segments.rows.edit_reference_lines(|rows| *rows =  vec![crate::feature::definitions::FeatureReferenceLineSegment {
        directions: [None; 3],
        point_ids: [Some(1), Some(4)],
        vertical_horizontal: None,
        external_id: 101,
        offset: 91,
    }]);
    special_segments.rows.edit_bounded_curves(|rows| *rows =  vec![crate::feature::definitions::FeatureBoundedCurveSegment {
        directions: [None; 3],
        point_ids: [5, 6],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 102,
        offset: 92,
    }]);
    special_segments.rows.edit_conics(Vec::clear);
    special_segments.rows.edit_opaque(Vec::clear);
    special_segments.declared_count = 2;
    let special_relation = &mut special_endpoint_dimension
        .relations
        .as_mut()
        .expect("relations")
        .rows[0];
    special_relation.relation_type = 0;
    special_relation.operand_vectors = Some([
        [Some(1), Some(5), None, Some(1)],
        [Some(1), Some(1), Some(0), Some(1)],
        [Some(15), Some(16), Some(15), Some(1)],
    ]);
    assert_eq!(*(crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx,
            &special_endpoint_dimension,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        )).expect("test section solve")[0]
        .0
        .definition).kind(),
        SketchConstraintDefinitionInput::HorizontalDistance {
            first: SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:101".to_string(),
            ).expect("valid test fixture")),
            second: SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:102".to_string(),
            ).expect("valid test fixture")),
            parameter: ParameterId::mint("creo:featdefs:parameter#917:42".to_string()).expect("identity grammar"),
        }
    );
    vector_native.relations.as_mut().expect("relations").rows[0].used = 34;
    let stored_state =
        crate::decode::with_test_decode_ctx(|ctx| section_dimension_constraints(ctx, &vector_native, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"))).expect("test section solve");
    assert!(matches!(
        stored_state[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Native {
            native_state: Some(34),
            native_flags: None,
            ..
        }
    ));
}

