// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

fn link_order_refusal(configure: impl FnOnce(&mut DecodePolicy)) -> cadmpeg_core::CodecError {
    // One more link than the stable sort sorts without scratch.
    let links = (0..21)
        .map(|ordinal| crate::native::segments::SegmentOmLink {
            id: format!("link-{ordinal}"),
            row: "row".to_string(),
            slot: crate::native::segments::SegmentIndexSlot::TypeCode,
            schema_role: crate::native::om::OmSchemaRole::FeatureHistory,
            location: crate::native::segments::om_location::OmLocation::new(1, 0).unwrap(),
        })
        .collect::<Vec<_>>();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| crate::native::features::canonical_feature_history_links(ctx, links).unwrap_err(),
    )
}

#[test]
fn feature_history_links_refuse_work_limit() {
    let error = link_order_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits)
    );
}

#[test]
fn feature_history_links_refuse_scoped_limit() {
    let error = link_order_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes)
    );
}
