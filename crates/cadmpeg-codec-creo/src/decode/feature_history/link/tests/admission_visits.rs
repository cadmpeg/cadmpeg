// SPDX-License-Identifier: Apache-2.0
use super::super::{
    generated_profile_entry_is_admissible, generated_profile_table_shape,
    generated_surface_id_for_feature, insert_ordered_family_surface_binding,
    ordered_analytic_surface_id_for_feature, section_entity_is_generated_profile,
    surface_kind_for_geometry, unique_model_feature_index, SurfaceBindingSource,
};
use crate::feature::entity::{FeatureEntityTable, FeatureEntityTableEntry};
use crate::surface::SurfaceKind;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use std::collections::{BTreeMap, BTreeSet};

fn work_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn entry(id: u32, class: u32, source: Option<u32>) -> FeatureEntityTableEntry {
    FeatureEntityTableEntry {
        payload: crate::feature::entity::entry_payload(class, source, None, None),
        entity_id: id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    }
}

#[test]
fn fixed_history_queries_are_free_and_preserve_original_refusal() {
    let ir = CadIr::empty();
    let target = super::feature("creo:model:feature#7").id;
    let table = FeatureEntityTable::new(7, 29, Vec::new(), &BTreeSet::new(), 0);
    let operand = entry(1, 200, None);
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let order = crate::feature::definitions::FeatureOrderTable {
        declared_count: 0,
        has_prototype: false,
        entity_ref: None,
        rows: Vec::new().into(),
        offset: 0,
    };
    let unknown = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let procedural = SurfaceGeometry::Procedural {
        construction: cadmpeg_ir::ids::ProceduralSurfaceId::mint("test:model:surface#free-kind")
            .expect("identity"),
        cache: None,
    };
    let queries = |ctx: &DecodeContext<'_>| {
        [
            unique_model_feature_index(ctx, &ir, &target).map(|value| value.is_none()),
            generated_surface_id_for_feature(ctx, &[], 7, 1).map(|value| value.is_none()),
            generated_profile_entry_is_admissible(ctx, 7, &table, &operand, &[], &rows)
                .map(|value| !value),
            section_entity_is_generated_profile(ctx, false, Some(7), 1, &[], &[], &rows)
                .map(|value| !value),
            section_entity_is_generated_profile(ctx, true, None, 1, &[], &[], &rows)
                .map(|value| !value),
            generated_profile_table_shape(ctx, &table).map(|value| !value),
            surface_kind_for_geometry(ctx, &unknown).map(|value| value.is_none()),
            surface_kind_for_geometry(ctx, &procedural).map(|value| value.is_none()),
            ordered_analytic_surface_id_for_feature(ctx, &rows, &[], 7, &order, 1, &unknown)
                .map(|value| value.is_none()),
            insert_ordered_family_surface_binding(
                ctx,
                &SurfaceBindingSource {
                    surface_rows: &rows,
                    feature_id: 7,
                    tables: &[],
                    order: &order,
                    expected_kind: SurfaceKind::Plane,
                },
                1,
                &mut BTreeMap::new(),
                &mut BTreeSet::new(),
            )
            .map(|value| !value),
        ]
    };
    let arena = DecodeArena::new();
    let policy = work_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    for result in queries(&ctx) {
        assert!(result.expect("fixed or absent query"));
    }
    let original = ctx
        .charge_work_limit(1, "seed history query refusal")
        .expect_err("zero cap");
    assert_eq!((original.used, original.additional), (0, 1));
    for result in queries(&ctx) {
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn generated_surface_query_counts_present_tables_and_entries_and_stops_on_ambiguity() {
    let unrelated = FeatureEntityTable::new(99, 29, Vec::new(), &BTreeSet::new(), 0);
    let unique = FeatureEntityTable::new(
        7,
        29,
        vec![
            entry(11, 200, Some(3)),
            entry(12, 200, Some(5)),
            entry(14, 200, Some(3)),
        ],
        &BTreeSet::new(),
        0,
    )
    .with_surface_ids([12, 14]);
    let mut ambiguous_entries = vec![
        entry(11, 200, Some(3)),
        entry(12, 200, Some(5)),
        entry(13, 200, Some(5)),
    ];
    ambiguous_entries.extend((0..128).map(|id| entry(100 + id, 200, Some(5))));
    let ambiguous = FeatureEntityTable::new(7, 29, ambiguous_entries, &BTreeSet::new(), 0)
        .with_surface_ids([12, 13]);
    for (table, source, expected) in [
        (&unique, 5, Some(12)),
        (&unique, 8, None),
        (&ambiguous, 5, None),
    ] {
        let tables = [unrelated.clone(), table.clone()];
        // One unrelated table, one matching table, three actual entries.
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &[
                "creo generated surface feature tables",
                "creo generated surface feature entries",
            ],
            |cap| {
                let arena = DecodeArena::new();
                let policy = work_policy(cap);
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = generated_surface_id_for_feature(&ctx, &tables, 7, source);
                if cap == 5 {
                    assert_eq!(result.expect("five source visits"), expected);
                    let exhausted = ctx
                        .charge_work_limit(1, "generated surface exact visit boundary")
                        .expect_err("all source visits used");
                    assert_eq!((exhausted.used, exhausted.additional), (5, 1));
                } else {
                    let Err(CodecError::ResourceLimit(original)) = result else {
                        panic!("a present source visit must refuse")
                    };
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(
                        (original.limit, original.used, original.additional),
                        (cap, cap, 1)
                    );
                    assert_eq!(
                        original.operation,
                        if cap < 2 {
                            "creo generated surface feature tables"
                        } else {
                            "creo generated surface feature entries"
                        }
                    );
                    assert!(
                        matches!(generated_surface_id_for_feature(&ctx, &tables, 7, source),
                    Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                    assert_eq!(ctx.resource_refusal(), Some(original));
                }

                if cap < 5 {
                    Err(ctx
                        .resource_refusal()
                        .expect("present visit refusal")
                        .into())
                } else {
                    Ok::<_, CodecError>(())
                }
            },
        );
    }
}

#[test]
fn generated_profile_shape_stops_at_first_missing_source_without_building_index() {
    let mut entries = vec![
        entry(1, 204, None),
        entry(2, 203, None),
        entry(3, 200, None),
    ];
    entries.extend((0..128).map(|id| entry(100 + id, 200, Some(id))));
    let table = FeatureEntityTable::new(7, 29, entries, &BTreeSet::new(), 0);
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &["creo generated profile remaining entries"],
        |cap| {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = generated_profile_table_shape(&ctx, &table);
            if cap == 1 {
                assert!(!result.expect("one remaining entry"));
                let exhausted = ctx
                    .charge_work_limit(1, "generated profile exact visit boundary")
                    .expect_err("one source visit used");
                assert_eq!((exhausted.used, exhausted.additional), (1, 1));
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("the first remaining entry must refuse")
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(
                    original.operation,
                    "creo generated profile remaining entries"
                );
                assert_eq!((original.used, original.additional), (0, 1));
                assert!(matches!(generated_profile_table_shape(&ctx, &table),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!(ctx.resource_refusal(), Some(original));
            }

            if cap < 1 {
                Err(ctx
                    .resource_refusal()
                    .expect("present visit refusal")
                    .into())
            } else {
                Ok::<_, CodecError>(())
            }
        },
    );
}
