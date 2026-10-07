// SPDX-License-Identifier: Apache-2.0
//! Work admission of existing owned collector sources.

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_source_work_refusal<T>(
    operation: &'static str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, CodecError>,
    populated: impl Fn(&T) -> bool,
) {
    let service_output = crate::test_support::with_service_context(|ctx| run(ctx))
        .expect("service source fixture decodes");
    assert!(populated(&service_output), "service output is nonempty");
    let refused = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = run(ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(refused,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == operation));
}

#[test]
fn b5_framed_record_scan_preserves_work_refusal() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let frames = crate::test_support::with_service_context(|ctx| {
        super::super::collect_object_stream_frames(ctx, &bytes)
    })
    .expect("service frame scan budget");
    assert_source_work_refusal(
        "catia_b5_framed_record_dependency_scan",
        |ctx| {
            let mut scratch = ctx.reserve_scoped(0, "test_b5_record_closure_scratch")?;
            super::super::topology_records_and_dependency_candidates(
                ctx,
                &bytes,
                &frames,
                None,
                &mut scratch,
            )
            .map(|output| output.map(|(records, candidates)| (records.len(), candidates.len())))
        },
        |output| output.as_ref().is_some_and(|(records, _)| *records > 0),
    );
}

#[test]
fn b5_framed_record_budget_exhaustion_stops_before_the_next_record() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let budget = cadmpeg_core::decode::WorkBudget::new(1);
    let output = crate::test_support::with_service_context(|ctx| {
        let frames = super::super::collect_object_stream_frames(ctx, &bytes)?;
        let mut scratch = ctx.reserve_scoped(0, "test_b5_record_closure_scratch")?;
        super::super::topology_records_and_dependency_candidates(
            ctx,
            &bytes,
            &frames,
            Some(&budget),
            &mut scratch,
        )
        .map(|output| output.is_none())
    })
    .expect("service budget");
    assert!(output);
    assert!(budget.exhausted());
}
