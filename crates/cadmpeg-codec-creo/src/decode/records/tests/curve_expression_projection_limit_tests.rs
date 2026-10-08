// SPDX-License-Identifier: Apache-2.0
use crate::curve::{
    CurveExpressionActivation, CurveExpressionAssignment, CurveExpressionEquation,
    CurveExpressionLine, CurveExpressionLocalSystem, CurveExpressionRecord,
    CurveExpressionSolveBlock, CurveExpressionTarget, CurveExpressionValue, SolveUnknown,
};
use crate::decode::records::curve_expression_records;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let assignment = CurveExpressionAssignment {
        target: CurveExpressionTarget::Parameter {
            name: "p".into(),
            declared_unit: None,
        },
        expression: "2".into(),
        dependencies: vec!["q".into()],
        value: Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"),
        )),
        activation: CurveExpressionActivation::Active,
        offset: 6,
    };
    scan.curves.expressions.push(CurveExpressionRecord {
        entity_id: 7,
        backup: false,
        local_system: Some(CurveExpressionLocalSystem {
            dimensions: 4,
            count: 3,
            body: vec![0xf9],
            explicit_slots: None,
            offset: 4,
        }),
        lines: vec![CurveExpressionLine {
            text: "p=2".into(),
            offset: 5,
        }],
        assignments: vec![assignment.clone()],
        solve_blocks: vec![CurveExpressionSolveBlock {
            equations: vec![CurveExpressionEquation {
                left: "p".into(),
                right: "q".into(),
                dependencies: vec!["q".into()],
                offset: 7,
            }],
            assignments: vec![assignment],
            unknowns: vec![SolveUnknown {
                name: "q".into(),
                solution: Some(CurveExpressionValue::Number(
                    cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"),
                )),
            }],
            offset: 8,
            for_offset: 9,
        }],
        unresolved_solve_control: false,
        prohibited_constructs: vec!["bad".into()],
        offset: 3,
        expression_offset: 5,
    });
    scan
}

#[test]
fn curve_expression_id_refuses_retained_limit() {
    let scan = scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo curve expression record id"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_materialized_bytes = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            curve_expression_records(&trial_ctx, &scan).map(|_| ())
        },
    );

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(error) = curve_expression_records(&ctx, &scan) else {
        panic!("native ID exceeds retained limit")
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo curve expression record id"),
        "{error:?}"
    );
}

#[test]
fn curve_expression_nested_rows_refuse_collection_limit() {
    let scan = scan();
    for operation in [
        "creo native curve expression lines",
        "creo native curve expression assignments",
        "creo native curve expression equations",
        "creo native curve expression block assignments",
        "creo native curve expression solve blocks",
        "creo native curve expression records",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some(operation),
            |cap| {
                let trial_arena = DecodeArena::new();
                let mut trial_policy = DecodePolicy::service();
                trial_policy.limits.max_collection_items = cap;
                let (trial_ctx, _) =
                    DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
                curve_expression_records(&trial_ctx, &scan).map(|_| ())
            },
        );

        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let Err(error) = curve_expression_records(&ctx, &scan) else {
            panic!("one more projection row exceeds the collection limit")
        };
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation),
            "{error:?}"
        );
    }
}

#[test]
fn borrowed_curve_expression_preserves_nested_json() {
    let scan = scan();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let (records, _records_storage) =
        curve_expression_records(&ctx, &scan).expect("expression is admitted");
    let value = serde_json::to_value(&records[0]).expect("record serializes");
    assert_eq!(value["local_system"]["body"], serde_json::json!([249]));
    assert_eq!(value["lines"][0]["text"], "p=2");
    assert_eq!(value["assignments"][0]["target"]["kind"], "parameter");
    assert_eq!(
        value["solve_blocks"][0]["variables"],
        serde_json::json!(["q"])
    );
    assert_eq!(
        value["solve_blocks"][0]["solutions"],
        serde_json::json!([2.0])
    );
    assert_eq!(value["prohibited_constructs"], serde_json::json!(["bad"]));
}
