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
    let geometry = MarkerGeometryIndex::new(&ctx, &[&center, &other, &curve], crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(&ctx, &[]).unwrap()).unwrap();
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
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
    let geometry = MarkerGeometryIndex::new(&ctx, &[&point, &other, &unlocated, &arc, &unowned, &relation], crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(&ctx, &[]).unwrap()).unwrap();
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

#[test]
fn embedded_rosters_extend_once_and_keep_shorter_prefixes_after_failure() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let marker = |id: &str, offset, coordinates: Option<[f64; 2]>| {
        let mut marker = SketchInputEntity::new(id, "lane", 0, offset, SketchInputKind::Point);
        marker.feature_ref = Some("owner".into());
        marker.coordinates_m = coordinates.and_then(cadmpeg_ir::units::FiniteVector::new);
        marker
    };
    let first = marker("first", 0, Some([0.0, 0.0]));
    let second = marker("second", 10, Some([1.0, 2.0]));
    let short = marker("short", 110, None);
    let long = marker("long", 210, None);
    let prefixes = MarkerPrefixIndex::new(&ctx, &[]).unwrap();
    assert!(prefixes.coordinates.set(vec![
        LegacyCoordinateRecord { offset: 0, coordinates: [0.0, 0.0], code_two_count: 1, embedded_count: 0 },
        LegacyCoordinateRecord { offset: 100, coordinates: [1.0, 2.0], code_two_count: 1, embedded_count: 1 },
        LegacyCoordinateRecord { offset: 200, coordinates: [99.0, 99.0], code_two_count: 1, embedded_count: 2 },
    ]).is_ok());
    let geometry = MarkerGeometryIndex::new(&ctx, &[&second, &first], prefixes).unwrap();
    assert_eq!(geometry.embedded_roster(&short).unwrap().unwrap().iter().map(|marker| marker.id()).collect::<Vec<_>>(), ["first", "second"]);
    assert!(geometry.embedded_roster(&long).unwrap().is_none());
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "resolve SLDPRT embedded coordinate roster", None,
    );
    assert!(geometry.embedded_roster(&long).unwrap().is_none());
    assert_eq!(geometry.embedded_roster(&short).unwrap().unwrap().len(), 2);
}

#[test]
fn embedded_coordinate_index_keeps_relative_tolerance_ambiguity() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let marker = |id: &str, offset, coordinates: Option<[f64; 2]>| {
        let mut marker = SketchInputEntity::new(id, "lane", 0, offset, SketchInputKind::Point);
        marker.feature_ref = Some("owner".into());
        marker.coordinates_m = coordinates.and_then(cadmpeg_ir::units::FiniteVector::new);
        marker
    };
    let first = marker("first", 0, Some([1.0e9, 2.0]));
    let close = marker("close", 10, Some([1.0e9 + 0.5, 2.0]));
    let curve = marker("curve", 110, None);
    let prefixes = MarkerPrefixIndex::new(&ctx, &[]).unwrap();
    assert!(prefixes.coordinates.set(vec![
        LegacyCoordinateRecord { offset: 0, coordinates: [1.0e9, 2.0], code_two_count: 1, embedded_count: 0 },
        LegacyCoordinateRecord { offset: 100, coordinates: [1.0e9, 2.0], code_two_count: 1, embedded_count: 1 },
    ]).is_ok());
    let geometry = MarkerGeometryIndex::new(&ctx, &[&close, &first], prefixes).unwrap();
    assert!(geometry.embedded_roster(&curve).unwrap().is_none());
}


#[test]
fn object_sources_include_unlocated_records_and_all_owners_once() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let marker = |id, owner: Option<&str>, index, coordinates: Option<[f64; 2]>| {
        let mut marker = SketchInputEntity::new(id, "lane", 0, 0, SketchInputKind::Arc)
            .with_test_identity(Some(index), None);
        marker.feature_ref = owner.map(str::to_owned);
        marker.coordinates_m = coordinates.and_then(cadmpeg_ir::units::FiniteVector::new);
        marker
    };
    let unlocated = marker("unlocated", Some("owner"), 7, None);
    let located = marker("located", Some("owner"), 7, Some([1.0, 2.0]));
    let other_owner = marker("other", Some("other-owner"), 7, None);
    let unowned = marker("unowned", None, 7, None);
    let other_index = marker("other-index", Some("owner"), 8, None);
    let geometry = MarkerGeometryIndex::new(&ctx,
        &[&unlocated, &other_owner, &located, &unowned, &other_index],
        MarkerPrefixIndex::new(&ctx, &[]).unwrap(),
    ).unwrap();
    assert!(geometry.object_sources.get().is_none());
    assert_eq!(geometry.object_markers(&ctx, &unlocated, Some(7)).unwrap().iter()
        .map(|marker| marker.id()).collect::<Vec<_>>(), ["located"]);
    let sources = geometry.object_sources(&ctx, Some(7)).unwrap();
    assert_eq!(sources.iter().map(|marker| marker.id()).collect::<Vec<_>>(),
        ["unlocated", "located", "other", "unowned"]);
    let cached = sources.as_ptr();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "index SLDPRT object marker sources", None,
    );
    assert_eq!(geometry.object_sources(&ctx, Some(7)).unwrap().as_ptr(), cached);
    assert!(geometry.object_sources(&ctx, Some(9)).unwrap().is_empty());
}

#[test]
fn point_position_index_keeps_owner_and_point_kind_and_reuses_storage() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let marker = |id, owner: &str, kind, coordinates| {
        let mut marker = SketchInputEntity::new(id, "lane", 0, 0, kind);
        marker.feature_ref = Some(owner.into());
        marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new(coordinates);
        marker
    };
    let point = marker("point", "owner", SketchInputKind::Point, [0.0, 0.0]);
    let constrained = marker("constrained", "owner", SketchInputKind::ConstrainedPoint, [0.0, 2.0]);
    let arc = marker("arc", "owner", SketchInputKind::Arc, [0.0, 3.0]);
    let other = marker("other", "other-owner", SketchInputKind::Point, [0.0, 4.0]);
    let geometry = MarkerGeometryIndex::new(&ctx, &[&arc, &other, &constrained, &point], MarkerPrefixIndex::new(&ctx, &[]).unwrap()).unwrap();
    let index = geometry.point_positions(&arc).unwrap().unwrap();
    let (mut coordinates, _storage) = index.point_candidates(&ctx,
        super::super::arc_centers::PointPositionQuery::EqualRadii { start: Point2::new(-1.0, 0.0), end: Point2::new(1.0, 0.0) },
        [None, None],
    ).unwrap();
    coordinates.sort_unstable_by(|left, right| left[1].total_cmp(&right[1]));
    assert_eq!(coordinates, [[0.0, 0.0], [0.0, 2.0]]);
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "index SLDPRT arc center positions", None,
    );
    assert!(std::ptr::eq(index, geometry.point_positions(&arc).unwrap().unwrap()));
}
