// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn symbolic_thread_text_releases_parser_storage() {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(
            &[0xff; 4],
            "SYMBOLIC_THREAD",
            b"\x03\x05CUT\0\x04\x09IGNORED\0\x03\x04M3\0".to_vec(),
        )],
        &[],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .unwrap();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let history = super::FeatureHistory::new(ctx, &container)?;
        super::feature_symbolic_threads(ctx, &history)
    };
    let rows = crate::test_support::with_decode_context(|ctx| decode(ctx)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text_frames.len(), 2);
    assert_eq!(rows[0].text_frames[0].value.as_str(), "CUT");
    assert_eq!(rows[0].text_frames[1].value.as_str(), "M3");
    assert_eq!(rows[0].text_frames[1].ordinal, 1);
    let expected = rows.capacity() * std::mem::size_of::<super::FeatureSymbolicThread>()
        + rows
            .iter()
            .map(|row| {
                row.id.len()
                    + row.operation_label.len()
                    + row.operation_record.len()
                    + row.text_frames.capacity()
                        * std::mem::size_of::<super::FeatureSymbolicThreadTextFrame>()
                    + row
                        .text_frames
                        .iter()
                        .map(|frame| {
                            frame.id.len()
                                + frame.symbolic_thread.len()
                                + frame.value.as_str().len()
                        })
                        .sum::<usize>()
            })
            .sum::<usize>();
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "test symbolic_thread_text retained checkpoint",
        |ctx| {
            let rows = decode(ctx)?;
            ctx.charge_retained(1, "test symbolic_thread_text retained checkpoint")?;
            Ok(rows)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == u64_from_index(expected) && limit.additional == 1));
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_materialized_bytes = 1_048_576,
        |ctx| {
            let rows = decode(ctx)?;
            assert_eq!(rows.len(), 1);
            let _checkpoint =
                ctx.reserve_scoped(1_048_576, "test symbolic_thread_text scratch checkpoint")?;
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}

#[test]
fn symbolic_thread_rejection_releases_parser_storage() {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(
            &[0xff; 4],
            "SYMBOLIC_THREAD",
            b"\x03\x05CUT\0\x04\x09IGNORED\0".to_vec(),
        )],
        &[],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let history = super::FeatureHistory::new(ctx, &container)?;
        assert_eq!(history.sections().len(), 1);
        assert_eq!(history.sections()[0].records.len(), 1);
        let record = &history.sections()[0].records[0].1;
        let frames = crate::om::operation_payload_text_frames(ctx, record.payload_view())?;
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].marker, crate::om::OperationTextMarker::Text);
        assert_eq!(frames[1].marker, crate::om::OperationTextMarker::String);
        assert!(super::symbolic_thread_text_frames(ctx, record.payload_view())?.is_none());
        Ok::<_, CodecError>(())
    })
    .unwrap();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 1_048_576;
        },
        |ctx| {
            let rows = {
                let history = super::FeatureHistory::new(ctx, &container)?;
                super::feature_symbolic_threads(ctx, &history)?
            };
            assert!(rows.is_empty());
            let _checkpoint = ctx.reserve_scoped(
                1_048_576,
                "test rejected symbolic-thread scratch checkpoint",
            )?;
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}
