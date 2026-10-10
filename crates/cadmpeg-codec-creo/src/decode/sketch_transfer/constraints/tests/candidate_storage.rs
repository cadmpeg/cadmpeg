// SPDX-License-Identifier: Apache-2.0

use super::super::{section_dimension_constraints_with_links, RelationIncidences};
use crate::feature::definitions::{DefinitionIdentity, DimensionValue, FeatureDefinition, FeatureDimension, FeatureDimensionTable, FeatureRelation, FeatureRelationTable};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchId};

fn definition() -> FeatureDefinition {
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed { schema_id: std::num::NonZeroU32::new(1), owner_feature_id: None },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(),
        variables: None, segments: None, trim_entities: None, trim_vertices: None,
        order_table: None, section_3d: None,
        dimensions: Some(FeatureDimensionTable { declared_count: 1, entity_ref: None,
            rows: vec![FeatureDimension { dimension_type: 3, value: DimensionValue::Resolved(2.0), value_body: Vec::new(), direction_byte: 0, auxiliary_value: None, auxiliary_body: Vec::new(), external_id: 7, references: None, offset: 0 }], offset: 0 }),
        relations: Some(FeatureRelationTable { declared_count: 3, entity_ref: None,
            rows: vec![FeatureRelation { relation_id: 11, used: 1, operands: Vec::new(), operand_vectors: None, sign: 1, dimension_id: 0, relation_type: 17, body: Vec::new(), offset: 0 }], skamps: None, triples: None, offset: 0 }),
        saved_section: None, offset: 0,
    }
}

#[test]
fn unsupported_typed_dimension_does_not_copy_a_typed_parameter() {
    let definition = definition();
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || {
            let solver = RelationIncidences::new(&ctx, &definition)?;
            let (rows, _slots) = section_dimension_constraints_with_links(&ctx, &sketch, &solver)?;
            assert_eq!(rows.len(), 1);
            assert!(matches!(rows[0].0.definition.kind(), SketchConstraintDefinitionInput::Native { parameter: Some(_), .. }));
            Ok::<_, CodecError>(())
        };
        let result = run();
        if let Err(CodecError::ResourceLimit(resource)) = &result {
            assert_ne!(resource.operation, "creo typed dimension parameter copy");
        }
        result
    });
}

#[test]
fn discarded_dimension_candidates_do_not_consume_retained_storage() {
    let definition = definition();
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for _ in 0..16 {
        let solver = RelationIncidences::new(&ctx, &definition).expect("solver");
        let (rows, _slots) = section_dimension_constraints_with_links(&ctx, &sketch, &solver).expect("candidate storage stays provisional");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, 0);
        assert_eq!(rows[0].2, 0);
        assert!(matches!(rows[0].0.definition.kind(), SketchConstraintDefinitionInput::Native { .. }));
    }
    assert_eq!(ctx.copy_retained_text("x", "test surviving output").expect("discarded candidates retained no bytes"), "x");
    ctx.finish_session().expect("active session");
}

#[test]
fn discarded_equation_candidates_do_not_consume_retained_storage() {
        let row = |variable_type, key, value: Option<f64>| {
            crate::feature::definitions::FeatureVariableRow {
                variable_type: crate::feature::definitions::VariableType::from(variable_type),
                key,
                value: value.map_or(
                    crate::feature::definitions::ScalarLane::DimensionDriven,
                    crate::feature::definitions::ScalarLane::Value,
                ),
                value_body: Vec::new(),
                guess: value.map_or(
                    crate::feature::definitions::ScalarLane::DimensionDriven,
                    crate::feature::definitions::ScalarLane::Value,
                ),
                guess_body: Vec::new(),

                known: Some(0),
                homogeneity: Some(1),
                uvar_id: None,

                offset: 0,
            }
        };
        let definition = crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(40),
                owner_feature_id: None,
            },
            body: b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
                    \xe0\x01id\0\0\xf1\xf7\x80\x9f\xe2\
                    \x01\x05\xf8\x03\x00\x01\x02\xf6\xe2"
                .to_vec(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::definitions::FeatureVariableTable {
                declared_count: 3,
                entity_ref: None,
                rows: vec![
                    row(6, 10, Some(2.5)),
                    row(6, 11, Some(2.5)),
                    row(5, 20, Some(0.0)),
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for _ in 0..16 {
        let (rows, _slots) = super::super::section_equation_function_five_scalar_equality_candidates(&ctx, &definition, &sketch).expect("equation storage stays provisional");
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0].0.definition.kind(), SketchConstraintDefinitionInput::ScalarEquality { first: 10, second: 11 }));
    }
    assert_eq!(ctx.copy_retained_text("x", "test surviving output").expect("discarded candidates retained no bytes"), "x");
    ctx.finish_session().expect("active session");
}

fn line_definition() -> FeatureDefinition {
    use crate::feature::definitions::{FeatureSegment, FeatureSegmentKind, FeatureSegmentTable};
    let mut definition = definition();
    definition.segments = Some(FeatureSegmentTable {
        declared_count: 1, has_elided_prototype: false, entity_ref: None,
        rows: [crate::feature::segment_rows::SegmentRow::Ordinary(FeatureSegment {
            kind: FeatureSegmentKind::Line([1, 2]), directions: [None; 3], center_id: None,
            arc_orientation: None, vertical_horizontal: None, radius_ref: None, radius2_ref: None,
            external_id: 7, body: Vec::new(), offset: 0,
        })].into_iter().collect(), offset: 0,
    });
    definition
}

#[test]
fn unknown_dimension_coordinate_does_not_build_unused_loci() {
    use crate::feature::definitions::{FeatureRelationTriple, FeatureSkamp, FeatureSkampItem, FeatureSolverTableHeader, SolverSubtable};
    let mut definition = line_definition();
    definition.dimensions.as_mut().expect("dimensions").rows[0].dimension_type = 1;
    let relation = definition.relations.as_mut().expect("relation table");
    relation.rows[0].relation_type = 0;
    relation.rows[0].operand_vectors = Some([[Some(1), Some(2), None, Some(1)], [Some(0); 4], [Some(15), Some(16), Some(15), Some(1)]]);
    relation.skamps = Some(SolverSubtable::Declared {
        header: FeatureSolverTableHeader { declared_count: 1, entity_ref: 0, offset: 0 },
        rows: vec![FeatureSkamp { id: 3, kind: 99, flags: 0, status: 1, items: vec![FeatureSkampItem { entity_id: 7, sense: 0 }], offset: 0 }],
    });
    relation.triples = Some(SolverSubtable::Declared {
        header: FeatureSolverTableHeader { declared_count: 1, entity_ref: 0, offset: 0 },
        rows: vec![FeatureRelationTriple { relation_id: Some(11), equation_id: None, skamp_id: Some(3), offset: 0 }],
    });
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || {
            let solver = RelationIncidences::new(&ctx, &definition)?;
            let (rows, _slots) = section_dimension_constraints_with_links(&ctx, &sketch, &solver)?;
            assert_eq!(rows.len(), 1);
            let SketchConstraintDefinitionInput::Distance { entities, .. } = rows[0].0.definition.kind() else { panic!("incidence distance remains typed"); };
            assert_eq!(entities.len(), 1);
            assert_eq!(entities[0].as_str(), "creo:featdefs:sketch_entity#1:7");
            Ok::<_, CodecError>(())
        };
        let result = run();
        if let Err(CodecError::ResourceLimit(resource)) = &result { assert_ne!(resource.operation, "creo dimension locus entity copy"); }
        result
    });
}

#[test]
fn rejected_incidence_locus_pair_releases_its_first_identity() {
    use crate::feature::definitions::{FeatureSkamp, FeatureSkampItem};
    let definition = line_definition();
    let incidence = FeatureSkamp { id: 3, kind: 99, flags: 0, status: 1, items: vec![FeatureSkampItem { entity_id: 7, sense: 0 }, FeatureSkampItem { entity_id: 7, sense: 99 }], offset: 0 };
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let refusal = std::cell::Cell::new(None);
    for _ in 0..16 {
        assert!(super::super::relation_incidence_loci(&ctx, &refusal, &definition, &sketch, Some(&incidence)).expect("failed pair releases storage").is_none());
        assert!(refusal.take().is_none());
    }
    assert_eq!(ctx.copy_retained_text("x", "test surviving output").expect("discarded first locus retained no bytes"), "x");
    ctx.finish_session().expect("active session");
}
