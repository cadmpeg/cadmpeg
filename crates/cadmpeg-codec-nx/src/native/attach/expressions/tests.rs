// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::native::om::{ExpressionUnit, ParameterFormula};

#[test]
fn expression_dependency_index_preserves_ambiguous_names_and_cycles() {
    let formula = |id: &str, name: &str, expression: &str| ParameterFormula {
        id: id.into(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.into()),
        expression: expression.into(),
        unit: ExpressionUnit::Millimeter,
        value: None,
        source_table: cadmpeg_core::text::NonBlankString::try_from("nx:test:expression-table#1")
            .unwrap(),
        source_entry: "synthetic".into(),
        source_offset: 0,
    };
    let records = [
        formula("first", "p1", "p2 + 1"),
        formula("second", "p2", "1"),
        formula("third", "p2", "2"),
        formula("cycle-a", "p3", "p4"),
        formula("cycle-b", "p4", "p3"),
    ];
    crate::test_support::with_decode_context(|ctx| {
        let mut expressions = records.iter().collect::<Vec<_>>();
        let mut storage = ctx.reserve_scoped(0, "test expression ordering").unwrap();
        assert_eq!(
            order_expression_dependencies(ctx, &mut storage, &mut expressions).unwrap(),
            3
        );
        assert_eq!(
            expressions
                .iter()
                .map(|value| value.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second", "third", "cycle-a", "cycle-b"]
        );
    });
}

#[test]
fn expression_dependency_refusal_reaches_keyed_lookup() {
    let formula = ParameterFormula {
        id: "nx:test:expression#1".into(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new("p1".into()),
        expression: "p1 + 1".into(),
        unit: ExpressionUnit::Millimeter,
        value: None,
        source_table: cadmpeg_core::text::NonBlankString::try_from("nx:test:expression-table#1")
            .unwrap(),
        source_entry: "synthetic".into(),
        source_offset: 0,
    };
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX expression dependency lookup",
        |ctx| {
            let mut expressions = vec![&formula];
            let mut storage = ctx.reserve_scoped(0, "test dependency scratch")?;
            order_expression_dependencies(ctx, &mut storage, &mut expressions).map(|_| ())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "NX expression dependency lookup")
    );
}

#[test]
fn expression_parameter_identity_retains_exact_text_before_return() {
    let expected = "nx:test:parameter#μ";
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(expected.len());
        },
        |ctx| {
            let id = expression_parameter_id(ctx, "nx:test:expression#μ")
                .unwrap()
                .unwrap();
            assert_eq!(id.as_str(), expected);
            let error = ctx
                .charge_retained(1, "test complete parameter identity storage")
                .unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.used == cadmpeg_core::decode::u64_from_index(expected.len()) && limit.additional == 1));
        },
    );
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "NX expression parameter identity",
        |ctx| expression_parameter_id(ctx, "nx:test:expression#μ"),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "NX expression parameter identity")
    );
}

#[test]
fn expression_dependency_order_releases_index_and_flags() {
    const LIMIT: u64 = 1_000_000;
    let formula = ParameterFormula {
        id: "nx:test:expression#1".into(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new("p1".into()),
        expression: "1".into(),
        unit: ExpressionUnit::Millimeter,
        value: None,
        source_table: cadmpeg_core::text::NonBlankString::try_from("nx:test:expression-table#1")
            .unwrap(),
        source_entry: "synthetic".into(),
        source_offset: 0,
    };
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_materialized_bytes = LIMIT,
        |ctx| {
            let mut expressions = vec![&formula];
            let mut storage = ctx
                .reserve_scoped(0, "test expression order result storage")
                .unwrap();
            assert_eq!(
                order_expression_dependencies(ctx, &mut storage, &mut expressions).unwrap(),
                1
            );
            assert_eq!(expressions[0].id, "nx:test:expression#1");
            let live_bytes = expressions.capacity() * std::mem::size_of::<&ParameterFormula>();
            let _remaining = ctx
                .reserve_scoped(
                    LIMIT - cadmpeg_core::decode::u64_from_index(live_bytes),
                    "test released expression index storage",
                )
                .unwrap();
        },
    );
}
