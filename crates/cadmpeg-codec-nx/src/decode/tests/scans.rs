// SPDX-License-Identifier: Apache-2.0
//! Work admission at scan-summary boundaries.

#[test]
fn stream_summary_counts_refuse_exhausted_work() {
    let file = crate::test_support::test_prt::single_part_prt();
    let container = crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file)).unwrap();
    let scan = crate::decode::Scan {
        container,
        streams: vec![crate::parasolid::Stream {
            file_offset: 0, consumed: 0, inflated: Vec::new(), body: crate::parasolid::StreamBody::Preview,
        }],
    };
    for count in [false, true] {
        crate::test_support::with_decode_context_over(&[], |policy| policy.limits.max_work_units = 0, |ctx| {
            let error = if count {
                scan.count(ctx, crate::parasolid::StreamKind::Preview).map(|_| ())
            } else { scan.has_parasolid(ctx).map(|_| ()) }.unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && matches!(limit.operation, "count NX streams" | "scan NX Parasolid streams")));
        });
    }
}

#[test]
fn summary_refuses_directory_scan_before_building_notes() {
    let file = crate::test_support::test_prt::single_part_prt();
    let container = crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file)).unwrap();
    let scan = crate::decode::Scan { container, streams: Vec::new() };
    crate::test_support::with_decode_context_over(&[], |policy| policy.limits.max_work_units = 0, |ctx| {
        let error = crate::scan_notes::summarize(ctx, &scan).err().expect("summary must refuse");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "count NX directory entries"));
    });
}
