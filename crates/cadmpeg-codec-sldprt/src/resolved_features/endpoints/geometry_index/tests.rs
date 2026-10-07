use super::*;
use crate::resolved_features::endpoints::arc_centers::unique_arc_center_marker;
use crate::resolved_features::LEGACY_SKETCH_MARKER;
use cadmpeg_ir::math::Point2;

const EPS_CENTER_POSITION: f64 = 1.0e-8;

#[test]
fn marker_prefix_lookup_keeps_incomplete_records_and_reuses_the_scan() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut payload = vec![0; 128];
    for offset in [4, 12, 64] {
        payload[offset..offset + LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    }
    let prefixes = MarkerPrefixIndex::new(&ctx, &payload).unwrap();
    assert!(prefixes.offsets.get().is_none());
    assert_eq!(prefixes.nth(&ctx, 4, 64, 1).unwrap(), Some(12));
    let original = prefixes.offsets.get().unwrap().as_ptr();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "index SLDPRT sketch marker prefixes", None,
    );
    assert_eq!(prefixes.nth(&ctx, 5, 64, 1).unwrap(), Some(64));
    assert_eq!(prefixes.nth(&ctx, 5, 63, 1).unwrap(), None);
    assert_eq!(prefixes.offsets.get().unwrap().as_ptr(), original);
}

#[test]
fn marker_center_indexes_keep_owner_scope_units_and_cached_storage() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let marker = |id: &str, owner: &str, coordinates: Option<[f64; 2]>| {
        let mut marker = SketchInputEntity::new(id, "lane", 0, 0, SketchInputKind::Arc);
        marker.feature_ref = Some(owner.into());
        marker.coordinates_m = coordinates.and_then(cadmpeg_ir::units::FiniteVector::new);
        marker
    };
    let center = marker("center", "sketch", Some([0.005, 0.006]));
    let other = marker("other", "another-sketch", Some([0.005, 0.007]));
    let curve = marker("curve", "sketch", None);
    let geometry = MarkerGeometryIndex::new(&ctx, &[&center, &other, &curve]).unwrap();
    let native = geometry.arc_centers(&curve, false, EPS_CENTER_POSITION).unwrap().unwrap();
    let repeated = geometry.arc_centers(&curve, false, EPS_CENTER_POSITION).unwrap().unwrap();
    assert!(std::ptr::eq(native, repeated));
    assert_eq!(unique_arc_center_marker(&ctx, Point2::new(0.004, 0.006), Point2::new(0.006, 0.006),
        native, EPS_CENTER_POSITION, [None, None]).unwrap(), Some(Point2::new(0.005, 0.006)));
    let profile = geometry.arc_centers(&curve, true, EPS_CENTER_POSITION).unwrap().unwrap();
    assert!(!std::ptr::eq(native, profile));
    assert_eq!(unique_arc_center_marker(&ctx, Point2::new(4.0, 6.0), Point2::new(6.0, 6.0),
        profile, EPS_CENTER_POSITION, [None, None]).unwrap(), Some(Point2::new(5.0, 6.0)));
    assert_eq!(unique_arc_center_marker(&ctx, Point2::new(4.0, 6.0), Point2::new(6.0, 6.0),
        profile, EPS_CENTER_POSITION, [Some("center"), None]).unwrap(), None);
}

#[test]
fn owner_rosters_keep_unlocated_ordinals_and_cache_each_subset() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let marker = |id: &str, owner: Option<&str>, offset: u64, kind, coordinates: Option<[f64; 2]>| {
        let mut marker = SketchInputEntity::new(id, "lane", 0, offset, kind);
        marker.feature_ref = owner.map(str::to_owned);
        marker.coordinates_m = coordinates.and_then(cadmpeg_ir::units::FiniteVector::new);
        marker
    };
    let unlocated = marker("unlocated", Some("owner"), 8, SketchInputKind::Arc, None);
    let point = marker("point", Some("owner"), 24, SketchInputKind::Point, Some([1.0, 2.0]));
    let arc = marker("arc", Some("owner"), 16, SketchInputKind::Arc, Some([3.0, 4.0]));
    let relation = marker("relation", Some("owner"), 20, SketchInputKind::Relation(crate::records::SketchRelationKind::Coincident), Some([1.0, 2.0]));
    let other = marker("other", Some("other-owner"), 0, SketchInputKind::Point, Some([1.0, 2.0]));
    let unowned = marker("unowned", None, 4, SketchInputKind::Point, Some([5.0, 6.0]));
    let geometry = MarkerGeometryIndex::new(&ctx, &[&point, &other, &unlocated, &arc, &unowned, &relation]).unwrap();
    let all = geometry.roster(&arc, MarkerRoster::All).unwrap();
    assert_eq!(all.iter().map(|marker| marker.id()).collect::<Vec<_>>(), ["unlocated", "arc", "relation", "point"]);
    let located = geometry.roster(&arc, MarkerRoster::Located).unwrap();
    assert_eq!(located.iter().map(|marker| marker.id()).collect::<Vec<_>>(), ["arc", "relation", "point"]);
    assert_eq!(geometry.roster(&arc, MarkerRoster::Points).unwrap()[0].id(), "point");
    assert_eq!(geometry.roster(&unowned, MarkerRoster::All).unwrap()[0].id(), "unowned");
    let subset = geometry.roster(&arc, MarkerRoster::Geometry).unwrap();
    assert_eq!(subset.iter().map(|marker| marker.id()).collect::<Vec<_>>(), ["arc", "point"]);
    let cached = subset.as_ptr();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "index SLDPRT owner marker rosters", None,
    );
    assert_eq!(geometry.roster(&arc, MarkerRoster::Geometry).unwrap().as_ptr(), cached);
}
