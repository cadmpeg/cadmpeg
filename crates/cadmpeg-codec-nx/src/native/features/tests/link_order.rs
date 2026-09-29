// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn link_order_refusal(configure: impl FnOnce(&mut DecodePolicy)) -> cadmpeg_core::CodecError {
    let link = crate::native::segments::SegmentOmLink {
        id: "link".to_string(),
        row: "row".to_string(),
        slot: crate::native::segments::SegmentIndexSlot::TypeCode,
        schema_role: crate::native::om::OmSchemaRole::FeatureHistory,
        location: crate::native::segments::om_location::OmLocation::new(1, 0).unwrap(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    crate::native::features::canonical_feature_history_links(&ctx, vec![link]).unwrap_err()
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
