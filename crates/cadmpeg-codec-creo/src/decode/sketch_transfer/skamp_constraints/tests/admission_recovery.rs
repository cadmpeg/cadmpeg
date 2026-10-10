// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchId;

fn assert_empty_constraint_route(
    relations: Option<crate::feature::definitions::FeatureRelationTable>,
) {
    let mut definition = super::make_definition(false);
    definition.relations = relations;
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch");
    let empty_geometry = std::collections::BTreeMap::default();
    for geometry in [None, Some(&empty_geometry)] {
        for refused in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let original = refused.then(|| {
                ctx.charge_work_limit(1, "before empty SKAMP recovery")
                    .expect_err("zero work")
            });
            for _ in 0..2 {
                let result = super::super::section_skamp_constraints_for_geometry(
                    &ctx,
                    &definition,
                    &sketch,
                    geometry,
                );
                if let Some(original) = original {
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                } else {
                    assert!(result.expect("no SKAMP constraint rows").is_empty());
                }
            }
            if let Some(original) = original {
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                ctx.finish_session().expect("active free route");
            }
        }
    }
}

#[test]
fn absent_relations_are_free_and_keep_original_skamp_refusal() {
    assert_empty_constraint_route(None);
}

fn legacy_relation_table() -> crate::feature::definitions::FeatureRelationTable {
    crate::feature::definitions::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: vec![crate::feature::definitions::FeatureRelation {
            relation_id: 1,
            used: 0,
            operands: Vec::new(),
            operand_vectors: None,
            sign: 1,
            dimension_id: 0,
            relation_type: 15,
            body: Vec::new(),
            offset: 0,
        }],
        skamps: None,
        triples: None,
        offset: 0,
    }
}

#[test]
fn absent_skamp_rows_skip_unused_relations_and_keep_original_refusal() {
    assert_empty_constraint_route(Some(legacy_relation_table()));
}

#[test]
fn empty_declared_skamp_rows_are_free_and_keep_original_refusal() {
    let mut relations = legacy_relation_table();
    relations.skamps = Some(crate::feature::definitions::SolverSubtable::Declared {
        header: crate::feature::definitions::FeatureSolverTableHeader {
            declared_count: 0,
            entity_ref: 1,
            offset: 0,
        },
        rows: Vec::new(),
    });
    assert_empty_constraint_route(Some(relations));
}
