// SPDX-License-Identifier: Apache-2.0

use super::{EquationIncidences, RelationIncidences};
use crate::feature::definitions::{DefinitionIdentity, FeatureDefinition, FeatureRelation, FeatureRelationTable, FeatureRelationTriple, FeatureSkamp, FeatureSolverTableHeader, SolverSubtable};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchId};

fn definition(triples: Vec<FeatureRelationTriple>, rows: Vec<FeatureRelation>, declared_count: u32) -> FeatureDefinition {
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed { schema_id: std::num::NonZeroU32::new(1), owner_feature_id: None },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(), variables: None,
        segments: None, trim_entities: None, trim_vertices: None, order_table: None, section_3d: None,
        dimensions: None, saved_section: None, offset: 0,
        relations: Some(FeatureRelationTable {
            declared_count, entity_ref: None, rows,
            skamps: Some(SolverSubtable::Declared {
                header: FeatureSolverTableHeader { declared_count: 1, entity_ref: 0, offset: 0 },
                rows: vec![FeatureSkamp { id: 3, kind: 99, flags: 0, status: 0, items: Vec::new(), offset: 0 }],
            }),
            triples: Some(SolverSubtable::Declared {
                header: FeatureSolverTableHeader { declared_count: u32::try_from(triples.len()).expect("fixture count"), entity_ref: 0, offset: 0 },
                rows: triples,
            }), offset: 0,
        }),
    }
}

fn triple(key: Option<u32>, offset: usize) -> FeatureRelationTriple {
    FeatureRelationTriple { relation_id: key, equation_id: key, skamp_id: Some(3), offset }
}

#[derive(Clone, Copy)]
enum JoinKind { Relation, Equation }

fn assert_unused_incidence_index(triples: Vec<FeatureRelationTriple>, kind: JoinKind) {
    let definition = definition(triples, Vec::new(), 1);
    let source_operation = match kind { JoinKind::Relation => "creo solver relation join rows", JoinKind::Equation => "creo solver equation join rows" };
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[source_operation], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || {
            match kind {
                JoinKind::Relation => {
                    let solver = RelationIncidences::new(&ctx, &definition)?;
                    assert!(solver.joined(9).is_none());
                }
                JoinKind::Equation => {
                    let solver = EquationIncidences::new(&ctx, &definition)?;
                    assert!(!solver.is_disabled(9));
                }
            }
            Ok::<_, CodecError>(())
        };
        let result = run();
        if let Err(CodecError::ResourceLimit(resource)) = &result {
            assert_ne!(resource.operation, "creo solver incidence identity rows");
        }
        result
    });
}

#[test]
fn relation_join_without_keys_does_not_index_incidences() {
    assert_unused_incidence_index(vec![triple(None, 0)], JoinKind::Relation);
}

#[test]
fn equation_join_without_keys_does_not_index_incidences() {
    assert_unused_incidence_index(vec![triple(None, 0)], JoinKind::Equation);
}

#[test]
fn relation_join_with_ambiguous_triples_does_not_index_incidences() {
    assert_unused_incidence_index(vec![triple(Some(9), 0), triple(Some(9), 1)], JoinKind::Relation);
}

#[test]
fn equation_join_with_ambiguous_triples_does_not_index_incidences() {
    assert_unused_incidence_index(vec![triple(Some(9), 0), triple(Some(9), 1)], JoinKind::Equation);
}

fn row(offset: usize) -> FeatureRelation {
    FeatureRelation { relation_id: 9, used: 1, operands: Vec::new(), operand_vectors: Some([[Some(1), None, None, None], [None; 4], [None; 4]]), sign: 1, dimension_id: 0, relation_type: 17, body: Vec::new(), offset }
}

fn assert_dimension_rows_without_join_consumers(rows: Vec<FeatureRelation>, declared_count: u32) {
    let definition = definition(vec![triple(Some(9), 0)], rows, declared_count);
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    crate::decode::with_test_decode_ctx(|ctx| {
        let general = RelationIncidences::new(ctx, &definition).expect("general joined-incidence consumer");
        assert!(general.joined(9).is_some());
        assert!(!general.is_unique(9));
    });
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = RelationIncidences::for_dimension_rows(&ctx, &definition);
        if let Err(CodecError::ResourceLimit(resource)) = &result {
            assert_ne!(resource.operation, "creo solver relation join rows");
            assert_ne!(resource.operation, "creo solver incidence identity rows");
        }
        let solver = result?.expect("native dimension rows remain available");
        assert!(std::ptr::eq(solver.definition, &definition));
        assert!(!solver.is_unique(9));
        assert!(solver.joined(9).is_none());
        Ok(())
    });
    crate::decode::with_test_decode_ctx(|ctx| {
        let solver = RelationIncidences::for_dimension_rows(ctx, &definition).expect("dimension solver").expect("source rows");
        let (constraints, _slots) = super::super::constraints::section_dimension_constraints_with_links(ctx, &sketch, &solver).expect("native dimensions");
        assert_eq!(constraints.len(), definition.relations.as_ref().expect("relations").rows.len());
        for (constraint, _, _, _) in &constraints {
            let SketchConstraintDefinitionInput::Native { entities, operands, .. } = constraint.definition.kind() else { panic!("incomplete or ambiguous rows stay native"); };
            assert!(entities.is_empty());
            assert_eq!(operands.len(), 1);
        }
    });
}

#[test]
fn incomplete_dimension_rows_do_not_build_unused_join_indexes() {
    assert_dimension_rows_without_join_consumers(vec![row(0)], 4);
}

#[test]
fn ambiguous_dimension_rows_do_not_build_unused_join_indexes() {
    assert_dimension_rows_without_join_consumers(vec![row(0), row(1)], 4);
}
