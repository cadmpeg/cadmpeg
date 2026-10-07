//! Lane curve joins preserve source occurrences and scoped storage.

use super::super::{marker_curve_endpoint_markers, marker_curve_endpoint_markers_in, CurveMarkers};
use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::HashMap;

fn point(id: &str, offset: u64, feature: Option<&str>) -> SketchInputEntity {
    let mut marker = SketchInputEntity::new(id, "lane", 0, offset, SketchInputKind::Point);
    marker.feature_ref = feature.map(str::to_owned);
    marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([f64::from(u32::try_from(offset).unwrap()), 0.0]);
    marker
}

fn links(marker: &mut SketchInputEntity, targets: &[&str]) {
    marker.links = SketchInputLinks::new(0, targets.iter().enumerate().map(|(index, target)| SketchInputLink {
        local_id: u16::try_from(index + 1).unwrap(),
        entity_ref: (*target).into(),
    }).collect());
}

#[test]
fn curve_marker_index_preserves_reverse_links_and_feature_separation() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut curve = SketchInputEntity::new("curve", "lane", 0, 0, SketchInputKind::LineOrCircle);
    curve.feature_ref = Some("feature".into());
    let mut first = point("first", 1, Some("feature"));
    let mut second = point("second", 2, Some("feature"));
    let mut foreign = point("foreign", 3, Some("other"));
    links(&mut first, &["curve", "curve"]);
    links(&mut second, &["curve"]);
    links(&mut foreign, &["curve"]);
    let roster = [&foreign, &second, &curve, &first];
    let by_id = roster.iter().map(|marker| (marker.id(), *marker)).collect::<HashMap<_, _>>();
    let (index, _storage) = CurveMarkers::new(&ctx, &roster).unwrap();
    let indexed = marker_curve_endpoint_markers_in(&ctx, &[], &curve, &by_id, &index).unwrap();
    let plain = marker_curve_endpoint_markers(&ctx, &[], &curve, &by_id, &roster).unwrap();
    assert_eq!(indexed.iter().map(|marker| marker.id()).collect::<Vec<_>>(), ["first", "second"]);
    assert_eq!(indexed, plain);
}

#[test]
fn curve_marker_offset_index_keeps_first_occurrence_at_repeated_offsets() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let curve = point("curve", 2, None);
    let first = point("first", 5, None);
    let duplicate = point("duplicate", 5, None);
    let earlier = point("earlier", 1, None);
    let foreign = point("foreign", 3, Some("other"));
    let roster = [&duplicate, &earlier, &foreign, &first];
    let (index, _storage) = CurveMarkers::new(&ctx, &roster).unwrap();
    assert!(std::ptr::eq(index.next_after(&ctx, &curve).unwrap().unwrap(), &duplicate));
    let after = point("after", 5, None);
    assert!(index.next_after(&ctx, &after).unwrap().is_none());
}

#[test]
fn curve_marker_index_storage_is_scoped_and_refuses_before_growth() {
    let marker = point("point", 1, Some("feature"));
    let roster = [&marker];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let built = CurveMarkers::new(&ctx, &roster).unwrap();
    drop(built);
    ctx.reserve_scoped(policy.limits.max_materialized_bytes, "released curve index").unwrap();
    crate::test_support::work_refusal_at("index SLDPRT curve markers", |ctx| {
        CurveMarkers::new(ctx, &roster).map(|_| ())
    });
}
