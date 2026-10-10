//! Layout probes admit roster scans only after recognizing stored endpoints.

use super::super::{
    coordinate_centered_line_endpoints, extended_wide_selected_axis_endpoints,
    legacy_point_roster_line_endpoint_markers, one_based_point_roster_line_endpoint_markers,
};
use crate::records::{SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn rejected_endpoint_layouts_do_not_scan_the_marker_roster() {
    let entities = (0..1024)
        .map(|i| {
            SketchInputEntity::new(
                format!("synthetic:marker#{i}"),
                "lane",
                0,
                0,
                SketchInputKind::LineOrCircle,
            )
        })
        .collect::<Vec<_>>();
    let markers = entities.iter().collect::<Vec<_>>();
    let owner = &entities[0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2048;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        extended_wide_selected_axis_endpoints(&ctx, &[], owner, &markers)
            .unwrap()
            .is_none()
    );
    assert!(
        one_based_point_roster_line_endpoint_markers(&ctx, &[], owner, &markers)
            .unwrap()
            .is_none()
    );
    assert!(
        legacy_point_roster_line_endpoint_markers(&ctx, &[], owner, &markers)
            .unwrap()
            .is_none()
    );
    assert!(
        coordinate_centered_line_endpoints(&ctx, &[], owner, &markers)
            .unwrap()
            .is_none()
    );
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn unlinked_markers_do_not_bill_identity_and_feature_comparisons() {
    let entities = (0..1000)
        .map(|i| {
            SketchInputEntity::new(
                format!("synthetic:marker:unlinked#{i}"),
                "lane",
                0,
                0,
                SketchInputKind::LineOrCircle,
            )
        })
        .collect::<Vec<_>>();
    let markers = entities
        .iter()
        .map(|marker| (marker.id(), marker))
        .collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2048;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        super::super::line_endpoint_markers(&ctx, &entities[0], &markers)
            .unwrap()
            .is_empty()
    );
    assert!(ctx.resource_refusal().is_none());
}
