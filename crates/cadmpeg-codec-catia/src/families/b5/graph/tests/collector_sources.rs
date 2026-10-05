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
fn b5_ordered_framed_records_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let frames = crate::test_support::with_service_context(|ctx| {
        super::super::collect_object_stream_frames(ctx, &bytes)
    })
    .expect("service frame scan budget");
    assert_source_work_refusal(
        "catia_b5_ordered_framed_records",
        |ctx| {
            super::super::framed_records_and_dependency_candidates(ctx, &bytes, &frames, None)
                .map(|(records, candidates)| (records.len(), candidates.len()))
        },
        |(records, _)| *records > 0,
    );
}

#[test]
fn b5_ordered_indexed_records_collector_preserves_work_refusal() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let frames = crate::test_support::with_service_context(|ctx| {
        super::super::collect_object_stream_frames(ctx, &bytes)
    })
    .expect("service frame scan budget");
    assert_source_work_refusal(
        "catia_b5_ordered_indexed_records",
        |ctx| {
            super::super::indexed_topology_records_and_dependency_candidates(
                ctx, &bytes, &frames, None,
            )
            .map(|output| output.map(|(records, candidates)| (records.len(), candidates.len())))
        },
        |output| output.as_ref().is_some_and(|(records, _)| *records > 0),
    );
}
