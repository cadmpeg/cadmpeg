// SPDX-License-Identifier: Apache-2.0
//! Actual first consumers, table lifetimes and unchanged thread selection.

use super::{HoleProfileKey, ThreadHistoryLookups, OPERATION, RECORDS};
use crate::records::{Feature, FeatureContent, FeatureHistory, FeatureInputLane,
    FeatureInputSurfaceSelection, FeatureInputSurfaceSelectionKind, FeatureSource};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn feature(id: &str, source: Option<u32>, class: &str) -> Feature {
    Feature {
        id: id.into(), parent: "history".into(), xml_tag: "Feature".into(),
        tree_parent: None, source_id: source.and_then(FeatureSource::from_value), ordinal: 0,
        name: id.into(), kind: "Custom".into(), input_class: Some(class.into()),
        suppressed: false, parameters: BTreeMap::new(), dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(), text: None, content: Vec::new(),
    }
}

fn history(features: Vec<Feature>) -> FeatureHistory {
    FeatureHistory { id: "history".into(), part_name: None, properties: BTreeMap::new(),
        content: Vec::new(), configurations: Vec::new(), features }
}

fn lane(producers: &[&str]) -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(), configuration: None, native_payload: Vec::new(),
        classes: Vec::new(), names: Vec::new(), scalars: Vec::new(),
        relation_bindings: Vec::new(), relation_instances: Vec::new(), body_selections: Vec::new(),
        edge_selections: Vec::new(), generated_surface_identities: Vec::new(),
        references: Vec::new(), sketch_entities: Vec::new(),
        surface_selections: vec![FeatureInputSurfaceSelection {
            id: "selection".into(), parent: "lane".into(), ordinal: 0, offset: 0, selector: 0,
            kind: FeatureInputSurfaceSelectionKind::Component, object_name_ref: "thread-name".into(),
            feature_ref: "thread".into(), producer_feature_refs: producers.iter().map(|id| (*id).into()).collect(),
            terminal_feature_ref: None, components: Vec::new(),
        }],
    }
}

fn direct_hole(id: &str, major: &str) -> Feature {
    let mut hole = feature(id, Some(10), "moHoleWzd_c");
    hole.parameters = [
        (cadmpeg_core::nonblank_literal!("Diameter"), "2.5mm".into()),
        (cadmpeg_core::nonblank_literal!("ThreadMajorDiameter"), major.into()),
        (cadmpeg_core::nonblank_literal!("ThreadDepth"), "6mm".into()),
        (cadmpeg_core::nonblank_literal!("DrillPointAngle"), "118°".into()),
    ].into();
    hole
}

fn profiled_history(child: &str) -> FeatureHistory {
    let mut hole = feature("hole", Some(10), "moHoleWzd_c");
    hole.properties.insert(cadmpeg_core::nonblank_literal!("DissectableChildren"), child.into());
    let mut profile = feature("profile", Some(11), "moProfileFeature_c");
    profile.kind = "Sketch".into();
    profile.parameters = [
        (cadmpeg_core::nonblank_literal!("bore"), "<MOD-DIAM>2.5".into()),
        (cadmpeg_core::nonblank_literal!("drill depth"), "7.5".into()),
        (cadmpeg_core::nonblank_literal!("major"), "<MOD-DIAM>3".into()),
        (cadmpeg_core::nonblank_literal!("thread depth"), "6".into()),
        (cadmpeg_core::nonblank_literal!("angle"), "118°".into()),
    ].into();
    profile.content = ["bore", "drill depth", "major", "thread depth", "angle"]
        .into_iter().map(|name| FeatureContent::Dimension(name.into())).collect();
    history(vec![hole, profile, feature("thread", Some(12), "moCosmeticThread_c")])
}

fn hash_bytes<T>() -> u64 {
    // The core first-growth bound has four typed buckets and control storage.
    u64::try_from(4 * std::mem::size_of::<T>()
        + std::mem::align_of::<T>().max(16) - 1 + 4 + 16).unwrap()
}

fn tree_bytes<K, V>() -> u64 {
    let alignment = std::mem::align_of::<K>().max(std::mem::align_of::<V>()).max(std::mem::align_of::<usize>());
    u64::try_from(11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment).unwrap()
}

fn enrich(ctx: &DecodeContext<'_>, history: &mut FeatureHistory, lanes: &[FeatureInputLane]) -> Result<(), CodecError> {
    super::super::enrich_history_cosmetic_thread_diameters(ctx, std::slice::from_mut(history), lanes)
}

#[test]
fn no_selected_lane_builds_no_history_tables_or_output() {
    let mut history = profiled_history("11");
    let expected = history.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| enrich(&ctx, &mut history, &[]));
    result.unwrap();
    assert_eq!(allocations, 0);
    assert_eq!(history, expected);
    ctx.finish_session().unwrap();
}

#[test]
fn empty_selection_lane_builds_no_history_tables() {
    let mut history = profiled_history("11");
    let expected = history.clone();
    let mut lane = lane(&["hole"]);
    lane.surface_selections.clear();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| enrich(&ctx, &mut history, &[lane]));
    result.unwrap();
    assert_eq!(allocations, 0);
    assert_eq!(history, expected);
    ctx.finish_session().unwrap();
}

#[test]
fn first_selected_id_refuses_before_table_allocation() {
    let mut history = profiled_history("11");
    let expected = history.clone();
    let lanes = [lane(&["hole"])];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| enrich(&ctx, &mut history, &lanes));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("first ID slot refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, OPERATION);
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert_eq!(allocations, 0);
    assert_eq!(history, expected);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn first_id_hash_growth_has_the_exact_typed_workspace_boundary() {
    let features = [feature("thread", Some(12), "moCosmeticThread_c")];
    let bytes = hash_bytes::<(&str, &Feature)>();
    for accepted in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes - u64::from(!accepted);
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut lookups = ThreadHistoryLookups::new(&ctx, &features).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| lookups.by_id("thread"));
        if accepted {
            assert!(std::ptr::eq(result.unwrap().unwrap(), &features[0]));
            assert_eq!(allocations, 1);
            drop(lookups);
            drop(ctx.reserve_scoped(bytes, "test ID workspace teardown").unwrap());
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("ID table bytes"); };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.operation, OPERATION);
            assert_eq!((limit.used, limit.additional, limit.limit), (0, bytes, bytes - 1));
            assert_eq!(allocations, 0);
            drop(lookups);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        }
    }
}

#[test]
fn first_source_tree_growth_has_the_exact_typed_workspace_boundary() {
    let features = [feature("profile", Some(11), "moProfileFeature_c")];
    let bytes = tree_bytes::<FeatureSource, &Feature>();
    for accepted in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes - u64::from(!accepted);
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut lookups = ThreadHistoryLookups::new(&ctx, &features).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| lookups.profile(HoleProfileKey::Source(FeatureSource::from_value(11).unwrap())));
        if accepted {
            assert!(std::ptr::eq(result.unwrap().unwrap(), &features[0]));
            assert_eq!(allocations, 1);
            assert!(lookups.by_id.is_none());
            assert!(lookups.records.is_none());
            drop(lookups);
            drop(ctx.reserve_scoped(bytes, "test source workspace teardown").unwrap());
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("source tree bytes"); };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.operation, OPERATION);
            assert_eq!((limit.used, limit.additional, limit.limit), (0, bytes, bytes - 1));
            assert_eq!(allocations, 0);
            drop(lookups);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        }
    }
}

#[test]
fn first_record_hash_growth_has_the_exact_typed_workspace_boundary() {
    let features = [feature("profile", None, "moProfileFeature_c")];
    let bytes = hash_bytes::<(&str, Option<&Feature>)>();
    for accepted in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes - u64::from(!accepted);
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut lookups = ThreadHistoryLookups::new(&ctx, &features).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| lookups.profile(HoleProfileKey::Record("profile")));
        if accepted {
            assert!(std::ptr::eq(result.unwrap().unwrap(), &features[0]));
            assert_eq!(allocations, 1);
            assert!(lookups.by_id.is_none());
            assert!(lookups.by_source.is_none());
            drop(lookups);
            drop(ctx.reserve_scoped(bytes, "test record workspace teardown").unwrap());
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("record hash bytes"); };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.operation, RECORDS);
            assert_eq!((limit.used, limit.additional, limit.limit), (0, bytes, bytes - 1));
            assert_eq!(allocations, 0);
            drop(lookups);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        }
    }
}

#[test]
fn repeated_queries_reuse_last_id_source_and_permanent_record_tombstones() {
    let features = [feature("profile", Some(11), "moProfileFeature_c"),
        feature("profile", Some(11), "moProfileFeature_c"), feature("profile", Some(11), "moProfileFeature_c")];
    let peak = hash_bytes::<(&str, &Feature)>() + tree_bytes::<FeatureSource, &Feature>()
        + hash_bytes::<(&str, Option<&Feature>)>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3; // One distinct slot in each actual table.
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut lookups = ThreadHistoryLookups::new(&ctx, &features).unwrap();
    for _ in 0..2 {
        assert!(std::ptr::eq(lookups.by_id("profile").unwrap().unwrap(), &features[2]));
        assert!(std::ptr::eq(lookups.profile(HoleProfileKey::Source(FeatureSource::from_value(11).unwrap())).unwrap().unwrap(), &features[2]));
        assert!(lookups.profile(HoleProfileKey::Record("profile")).unwrap().is_none());
    }
    drop(lookups);
    drop(ctx.reserve_scoped(peak, "test all thread lookups teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn direct_thread_parameters_do_not_consume_profile_indexes() {
    let mut history = history(vec![direct_hole("hole", "3mm"), feature("thread", Some(12), "moCosmeticThread_c")]);
    let lanes = [lane(&["hole"])];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, RECORDS, None);
    enrich(&ctx, &mut history, &lanes).unwrap();
    assert_eq!(history.features[1].parameters["D2"], "<MOD-DIAM>3mm");
    // The still-armed positive sentinel proves the record index was not used.
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, RECORDS) else { panic!("record index remained unused"); };
    assert_eq!(limit.operation, RECORDS);
    drop(probe);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn numeric_profile_source_hit_does_not_build_unique_records() {
    let mut history = profiled_history("11");
    let lanes = [lane(&["hole"])];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, RECORDS, None);
    enrich(&ctx, &mut history, &lanes).unwrap();
    assert_eq!(history.features[2].parameters["D2"], "<MOD-DIAM>3mm");
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, RECORDS) else { panic!("source hit skipped records"); };
    assert_eq!(limit.operation, RECORDS);
    drop(probe);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn textual_profile_fallback_accepts_the_unique_record_and_skips_source_lookup() {
    let mut history = profiled_history("profile");
    let lanes = [lane(&["hole"])];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "look up SLDPRT feature source", None);
    enrich(&ctx, &mut history, &lanes).unwrap();
    assert_eq!(history.features[2].parameters["D2"], "<MOD-DIAM>3mm");
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "look up SLDPRT feature source") else { panic!("text child skipped source lookup"); };
    drop(probe);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn lookup_tables_expire_before_the_actual_scoped_diameter_output() {
    let input = history(vec![direct_hole("hole", "3mm"), feature("thread", Some(12), "moCosmeticThread_c")]);
    let lanes = [lane(&["hole"])];
    let candidates = hash_bytes::<(String, Option<f64>)>() + u64::try_from("thread".len()).unwrap();
    let output = tree_bytes::<NonBlankString, String>() + u64::try_from("<MOD-DIAM>3mm".len()).unwrap();
    let peak = candidates + output;
    assert!(output > hash_bytes::<(&str, &Feature)>());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test diameter output owner").unwrap();
    let mut history = input;
    storage.with_storage(|| enrich(&ctx, &mut history, &lanes)).unwrap();
    assert_eq!(history.features[1].parameters["D2"], "<MOD-DIAM>3mm");
    drop(ctx.reserve_scoped(candidates, "test candidates and lookup expiry").unwrap());
    drop(history);
    drop(storage);
    drop(ctx.reserve_scoped(peak, "test actual output teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn conflicting_producers_and_existing_diameter_preserve_native_parameters() {
    let mut history = history(vec![direct_hole("hole", "3mm"), direct_hole("other-hole", "4mm"),
        feature("thread", Some(12), "moCosmeticThread_c")]);
    let ctx = cadmpeg_test_support::service_decode_context();
    enrich(&ctx, &mut history, &[lane(&["hole", "other-hole"])]).unwrap();
    assert!(history.features[2].parameters.is_empty());
    history.features[2].parameters.insert(cadmpeg_core::nonblank_literal!("D2"), "native-value".into());
    let expected = history.clone();
    enrich(&ctx, &mut history, &[lane(&["hole"])]).unwrap();
    assert_eq!(history, expected);
    ctx.finish_session().unwrap();
}

#[test]
fn thread_selection_preserves_an_original_refusal_without_allocation() {
    let mut history = profiled_history("11");
    let expected = history.clone();
    let lanes = [lane(&["hole"])];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original thread refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| enrich(&ctx, &mut history, &lanes));
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == first));
    assert_eq!(allocations, 0);
    assert_eq!(history, expected);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == first));
}
