// SPDX-License-Identifier: Apache-2.0

use super::super::prototype_mixed_derivatives;
use crate::surface::{
    SurfaceNamedParameter, SurfaceNamedValue, SurfacePrototypeFamily, SurfacePrototypeRecord,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const FIELD: &str = "end_uv_deriv";

fn record(value: SurfaceNamedValue) -> SurfacePrototypeRecord {
    SurfacePrototypeRecord {
        family: SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Splsrf),
        parameters: vec![SurfaceNamedParameter {
            name: FIELD.to_owned(),
            value,
            body: Vec::new(),
            offset: 0,
            value_offset: 0,
        }],
        offset: 0,
    }
}

fn array(dimensions: u32, count: u32, values: Vec<Option<f64>>) -> SurfaceNamedValue {
    let mut array = crate::surface::arrays::DimensionedScalars::empty(dimensions, count)
        .expect("fixture extent");
    array.fill_values(values).expect("finite slots match shape");
    SurfaceNamedValue::ScalarArray(array)
}

fn assert_projection(record: &SurfacePrototypeRecord, expected: Option<[[f64; 3]; 4]>) {
    // Unique lookup visits each present row once and admits both name operands.
    // All fixtures use the same field name. The checked 12-slot projection has
    // fixed size and no allocation, collection, entity or nesting admission.
    let events: Vec<_> = record
        .parameters
        .iter()
        .flat_map(|_| {
            [
                (1, "creo prototype field search"),
                (
                    u64::try_from(FIELD.len()).expect("fixture name fits u64"),
                    "creo prototype field name comparison",
                ),
                (
                    u64::try_from(FIELD.len()).expect("fixture name fits u64"),
                    "creo prototype field name comparison",
                ),
            ]
        })
        .collect();
    let work: u64 = events.iter().map(|(units, _)| units).sum();
    let run = |ctx: &DecodeContext<'_>| prototype_mixed_derivatives(ctx, record);
    let policy_at = |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        policy
    };
    let observed = std::cell::RefCell::new(Vec::new());
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &events
            .iter()
            .map(|(_, operation)| *operation)
            .collect::<Vec<_>>(),
        |cap| {
            let arena = DecodeArena::new();
            let policy = policy_at(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = run(&ctx);
            if let Err(CodecError::ResourceLimit(original)) = &result {
                assert_eq!((original.limit, original.used), (cap, cap));
                observed
                    .borrow_mut()
                    .push((original.additional, original.operation));
                for _ in 0..2 {
                    assert!(
                        matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == *original)
                    );
                }
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == *original)
                );
            }
            result.map(|actual| assert_eq!(actual, expected))
        },
    );
    assert_eq!(observed.into_inner(), events);
    for cap in [work, work + 1] {
        let arena = DecodeArena::new();
        let policy = policy_at(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(run(&ctx).expect("fixed projection admitted"), expected);
        let original = ctx
            .charge_work_limit(cap - work + 1, "after mixed derivative projection")
            .expect_err("source-derived work consumed");
        assert_eq!(original.used, work);
        for _ in 0..2 {
            assert!(
                matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        }
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
        );
    }
}

#[test]
fn mixed_derivatives_project_four_triples_without_temporary_storage() {
    let present = record(array(
        4,
        3,
        (0..12).map(|value| Some(f64::from(value))).collect(),
    ));
    assert_projection(
        &present,
        Some([
            [0.0, 1.0, 2.0],
            [3.0, 4.0, 5.0],
            [6.0, 7.0, 8.0],
            [9.0, 10.0, 11.0],
        ]),
    );
}

#[test]
fn mixed_derivatives_preserve_absent_partial_shape_and_ambiguity_routes() {
    for dimensions in [0, 1, 3, 5] {
        assert_projection(
            &record(array(
                dimensions,
                3,
                vec![
                    Some(1.0);
                    usize::try_from(dimensions).expect("fixture dimension fits usize") * 3
                ],
            )),
            None,
        );
    }
    assert_projection(&record(array(6, 2, vec![Some(1.0); 12])), None);
    for absent in [0, 11] {
        let mut values = vec![Some(1.0); 12];
        values[absent] = None;
        assert_projection(&record(array(4, 3, values)), None);
    }
    assert_projection(&record(SurfaceNamedValue::ScalarSequence(vec![1.0])), None);
    let mut duplicate = record(array(4, 3, vec![Some(1.0); 12]));
    duplicate.parameters.push(duplicate.parameters[0].clone());
    assert_projection(&duplicate, None);
    duplicate.parameters.clear();
    assert_projection(&duplicate, None);
}
