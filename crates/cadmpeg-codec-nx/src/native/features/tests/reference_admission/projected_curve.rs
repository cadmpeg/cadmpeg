// SPDX-License-Identifier: Apache-2.0

use super::reference_file;
use crate::native::features::construction_records::{
    feature_projected_curve_construction_payloads, feature_projected_curve_construction_strings,
    feature_projected_curve_references,
};
use crate::native::features::feature_operation_labels;
fn projected_curve_file() -> Vec<u8> {
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0"
            .to_vec();
    reference_file("CPROJ", payload)
}

fn projected_curve_payload_file() -> Vec<u8> {
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0"
            .to_vec();
    let mut store = (0..715).map(|_| b"A".as_slice()).collect::<Vec<_>>();
    store[712] = b"\x66\x32\x03\x05ABC\0";
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "CPROJ", payload)],
        &store,
    );

    crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)])
}

fn projected_curve_payload_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let file = projected_curve_payload_file();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .expect("synthetic container");
    let (labels, references) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_operation_labels(
                ctx,
                &crate::native::features::FeatureHistory::new(ctx, &container)?,
            )?,
            feature_projected_curve_references(
                ctx,
                &crate::native::features::FeatureHistory::new(ctx, &container)?,
            )?,
        ))
    })
    .expect("projected curve payload inputs");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_projected_curve_construction_payloads(ctx, &container, &labels, &references)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted projected curve payloads")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("projected curve payload resource limit"),
    )
}

fn projected_curve_string_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let file = projected_curve_payload_file();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .expect("synthetic container");
    let payloads = crate::test_support::with_decode_context(|ctx| {
        let labels = feature_operation_labels(
            ctx,
            &crate::native::features::FeatureHistory::new(ctx, &container)?,
        )?;
        let references = feature_projected_curve_references(
            ctx,
            &crate::native::features::FeatureHistory::new(ctx, &container)?,
        )?;
        feature_projected_curve_construction_payloads(ctx, &container, &labels, &references)
    })
    .expect("projected curve construction payloads");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_projected_curve_construction_strings(ctx, &container, &payloads)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted projected curve strings")
            .len(),
        1
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("projected curve string resource limit"),
    )
}

#[test]
fn projected_curve_string_refuses_collection_limit() {
    let error = projected_curve_string_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn projected_curve_string_refuses_retained_limit() {
    let error = projected_curve_string_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn projected_curve_string_refuses_scoped_limit() {
    let error = projected_curve_string_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn projected_curve_string_refuses_work_limit() {
    let error = projected_curve_string_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn projected_curve_payload_refuses_collection_limit() {
    let error = projected_curve_payload_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn projected_curve_payload_refuses_retained_limit() {
    let error = projected_curve_payload_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn projected_curve_payload_refuses_scoped_limit() {
    let error = projected_curve_payload_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn projected_curve_payload_refuses_work_limit() {
    let error = projected_curve_payload_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn projected_curve_route_refusal(
    dimension: cadmpeg_core::decode::ResourceDimension,
) -> cadmpeg_core::CodecError {
    let file = projected_curve_file();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .expect("synthetic container");
    crate::test_support::with_decode_context_over(
        container.data.as_ref(),
        |policy| {
            policy.limits.max_work_units = u64::MAX;
            policy.limits.max_collection_items = u64::MAX;
            policy.limits.max_retained_bytes = u64::MAX;
            policy.limits.max_materialized_bytes = u64::MAX;
        },
        |history_ctx| {
            let history = crate::native::features::FeatureHistory::new(history_ctx, &container)?;

            let records = crate::test_support::with_decode_context(|ctx| {
                feature_projected_curve_references(ctx, &history)
            })
            .expect("admitted projected curve references");
            assert_eq!(records.len(), 3);

            let operation = match dimension {
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    "build NX projected-curve references"
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    "NX resolved feature payload references"
                }
                _ => "NX projected curve references",
            };
            Ok::<_, cadmpeg_core::CodecError>(crate::test_support::resource_refusal_at(
                container.data.as_ref(),
                dimension,
                operation,
                |ctx| feature_projected_curve_references(ctx, &history),
            ))
        },
    )
    .expect("prepared feature history")
}

#[test]
fn projected_curve_reference_route_refuses_collection_limit() {
    let error =
        projected_curve_route_refusal(cadmpeg_core::decode::ResourceDimension::CollectionItems);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn projected_curve_reference_route_refuses_retained_limit() {
    let error =
        projected_curve_route_refusal(cadmpeg_core::decode::ResourceDimension::RetainedBytes);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn projected_curve_reference_route_refuses_scoped_limit() {
    let error =
        projected_curve_route_refusal(cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn projected_curve_reference_route_refuses_work_limit() {
    let error = projected_curve_route_refusal(cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
