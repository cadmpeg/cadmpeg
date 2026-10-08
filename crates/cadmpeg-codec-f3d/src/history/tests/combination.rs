// SPDX-License-Identifier: Apache-2.0
//! History row collection tests.
#![allow(clippy::unwrap_used)]

use crate::history::combine_historical_rows;

fn feature() -> cadmpeg_ir::features::FeatureId {
    cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#combine").unwrap()
}

#[test]
fn combine_historical_rows_preserve_exact_zip_output() {
    let feature = feature();
    let rows = crate::test_support::with_decode_context(|ctx| {
        combine_historical_rows(
            ctx,
            &feature,
            1,
            vec![2, 3],
            vec!["native".into()].into_iter().map(Ok),
        )
    })
    .unwrap();
    assert_eq!(
        rows,
        Some(vec![cadmpeg_ir::features::BodyMember::new(
            cadmpeg_ir::ids::HistoricalBodyId::mint("f3d:history-input:body#7:combine:1:2",)
                .unwrap(),
            cadmpeg_core::text::NonBlankString::try_from("native").unwrap(),
        )])
    );
}

#[test]
fn combine_historical_rows_keep_blank_member_short_circuit() {
    let feature = feature();
    let rows = crate::test_support::with_decode_context(|ctx| {
        combine_historical_rows(
            ctx,
            &feature,
            1,
            vec![2, 3],
            vec![String::new(), "native".into()].into_iter().map(Ok),
        )
    })
    .unwrap();
    assert_eq!(rows, None);
}

#[test]
fn combine_historical_rows_refuse_collection_work_limit() {
    let operation = "collect F3D Combine fallback rows";
    let feature = feature();
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            combine_historical_rows(
                ctx,
                &feature,
                1,
                vec![2],
                vec!["native".into()].into_iter().map(Ok),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn combine_historical_rows_propagate_member_work_refusal() {
    let operation = "validate body native member";
    let feature = feature();
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            combine_historical_rows(
                ctx,
                &feature,
                1,
                vec![2],
                vec!["native".into()].into_iter().map(Ok),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn rejected_combine_row_skips_native_identity_tail() {
    let feature = feature();
    let native = std::iter::once(Ok(String::new())).chain(std::iter::once_with(|| {
        panic!("rejected row must not encode the next native identity")
    }));
    let rows = crate::test_support::with_decode_context(|ctx| {
        combine_historical_rows(ctx, &feature, 1, vec![2, 3], native)
    })
    .unwrap();
    assert_eq!(rows, None);
}
