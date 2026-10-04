// SPDX-License-Identifier: Apache-2.0
//! Work admission for endpoint adjacency rows.

#[test]
fn zero_endpoint_match_sort_rows_preserve_work_refusal() {
    const OPERATION: &str = "catia_zero_match_sort_rows";
    let first = cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0);
    let last = cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0);
    let occurrences = [
        super::occurrence(1, 1, [first, last], first),
        super::occurrence(2, 2, [first, last], first),
    ];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::endpoint_match_graph(ctx, &occurrences, &ctx.work_budget(100_000))
    };
    let rows = crate::test_support::with_service_context(run).expect("service budget").expect("complete graph");
    assert_eq!(rows, [vec![1], vec![0]]);
    let result = crate::test_support::with_work_refusal(OPERATION, |ctx| {
        let result = run(ctx);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == OPERATION));
}
