// SPDX-License-Identifier: Apache-2.0

use super::super::{entity_intersection_cached, named_trim_vertex_prototype_complete,
    trim_buckets, trim_carrier, trim_radius, trim_vertex_entry, trim_vertex_entry_bounds,
    TrimEntryKind, TrimTableClasses, TrimTableHeader};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;

fn zero_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

#[test]
fn counted_trim_bounds_admit_only_present_entities() {
    let payload = b"\xf8\x02\x09\x0a\x03\x00";
    for (end, visits) in [(2, 0), (3, 1), (payload.len(), 2)] {
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let mut policy = zero_policy();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || trim_vertex_entry_bounds(&ctx, payload, 0, end);
            let result = run();
            if cap == visits {
                assert_eq!(result.expect("exact source work"),
                    (end == payload.len()).then_some((2, 3, payload.len(), 2)));
                let original = ctx.charge_work_limit(1, "after trim bounds")
                    .expect_err("present entity work consumed");
                assert_eq!((original.used, original.additional), (visits, 1));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                let original = ctx.resource_refusal().expect("present entity refusal");
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, 1, "creo trim vertex bound entities"));
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}

#[test]
fn counted_trim_vertices_admit_only_present_entities() {
    let payload = b"\xf8\x02\x09\x0a\x03\x00";
    for (end, visits) in [(2, 0), (3, 1), (payload.len(), 2)] {
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || trim_vertex_entry(&ctx, payload, 0, end);
            let result = run();
            if cap == visits {
                assert_eq!(result.expect("exact source work"),
                    (end == payload.len()).then_some((vec![9, 10], 3, payload.len())));
                let original = ctx.charge_work_limit(1, "after trim vertices")
                    .expect_err("present entity work consumed");
                assert_eq!((original.used, original.additional), (visits, 1));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                let original = ctx.resource_refusal().expect("present entity refusal");
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, 1, "creo trim vertex entity traversal"));
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}

#[test]
fn counted_trim_prototype_stops_at_absent_entity_source() {
    let payload = b"ent_ids\0\xf8\x02\x09\x0a";
    let classes = TrimTableClasses { table: 66, bucket: 67, entry: 68 };
    for (end, visits) in [(10, 0_u64), (11, 1)] {
        // The first search admits its bounded window and eight-byte label.
        // Only the following present entity byte needs a parser admission.
        let search_work = u64::try_from(end + b"ent_ids\0".len()).expect("small fixture");
        let need = search_work + visits;
        for cap in 0..=need {
            let arena = DecodeArena::new();
            let mut policy = zero_policy();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || named_trim_vertex_prototype_complete(&ctx, payload, 0, end, classes);
            let result = run();
            if cap == need {
                assert!(!result.expect("exact present-source work keeps incomplete prototype"));
                let original = ctx.charge_work_limit(1, "after trim prototype")
                    .expect_err("all source work consumed");
                assert_eq!((original.used, original.additional), (need, 1));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                let original = ctx.resource_refusal().expect("present source refusal");
                let (used, additional, operation) = if cap < search_work {
                    (0, search_work, "find Creo feature definition field")
                } else { (search_work, 1, "creo trim prototype entities") };
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, used, additional, operation));
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}

#[test]
fn empty_trim_buckets_keep_zero_work_and_original_refusal() {
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let header = TrimTableHeader { declared_count: 0,
        classes: TrimTableClasses { table: 66, bucket: 67, entry: 68 } };
    let run = || trim_buckets(&ctx, &[], 0, 0, header, TrimEntryKind::Vertex);
    assert!(run().expect("empty table is free").is_empty());
    let original = ctx.charge_work_limit(1, "before empty trim buckets").expect_err("zero work");
    assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn fixed_trim_bounds_keep_zero_work_and_original_refusal() {
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let payload = b"\x09\x0a\x03\x00";
    assert_eq!(trim_vertex_entry_bounds(&ctx, payload, 0, payload.len()).expect("fixed grammar"),
        Some((2, 3, payload.len(), 0)));
    let original = ctx.charge_work_limit(1, "before fixed trim bounds").expect_err("zero work");
    assert!(matches!(trim_vertex_entry_bounds(&ctx, payload, 0, payload.len()),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}

fn empty_geometry<'ctx>(ctx: &'ctx DecodeContext<'_>)
    -> super::super::super::points::CheckedTrimGeometry<'ctx> {
    let variables = super::FeatureVariableTable { declared_count: 0, entity_ref: None,
        rows: Vec::new(), offset: 0 };
    variables.reconciled_trim_geometry(ctx)
        .expect("empty fixture geometry")
}

fn isolated_segment() -> super::FeatureSegment {
    super::FeatureSegment { kind: super::FeatureSegmentKind::Point(7), directions: [None; 3],
        center_id: None, arc_orientation: None, vertical_horizontal: None, radius_ref: None,
        radius2_ref: None, external_id: 9, body: Vec::new(), offset: 0 }
}

#[test]
fn absent_trim_radius_keeps_zero_work_and_original_refusal() {
    let fixture_arena = DecodeArena::new();
    let fixture_policy = DecodePolicy::service();
    let (fixture_ctx, _) = DecodeContext::from_root_bytes(&[], &fixture_arena, &fixture_policy)
        .expect("fixture root");
    let geometry = empty_geometry(&fixture_ctx);
    let segment = isolated_segment();
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let run = || trim_radius(&ctx, &segment, [0.0; 2], &geometry);
    assert!(run().expect("absent radius is free").is_none());
    let original = ctx.charge_work_limit(1, "before absent trim radius").expect_err("zero work");
    assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn isolated_trim_carrier_keeps_zero_work_and_original_refusal() {
    let fixture_arena = DecodeArena::new();
    let fixture_policy = DecodePolicy::service();
    let (fixture_ctx, _) = DecodeContext::from_root_bytes(&[], &fixture_arena, &fixture_policy)
        .expect("fixture root");
    let geometry = empty_geometry(&fixture_ctx);
    let segment = isolated_segment();
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let run = || trim_carrier(&ctx, &segment, &geometry);
    assert!(run().expect("isolated point has no carrier").is_none());
    let original = ctx.charge_work_limit(1, "before isolated trim carrier").expect_err("zero work");
    assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn missing_trim_intersection_inputs_keep_zero_work_and_original_refusal() {
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut geometry = None;
    assert!(entity_intersection_cached(&ctx, &[], None, None, &mut geometry)
        .expect("absent inputs are free").is_none());
    assert!(geometry.is_none());
    let original = ctx.charge_work_limit(1, "before missing trim intersection").expect_err("zero work");
    assert!(matches!(entity_intersection_cached(&ctx, &[], None, None, &mut geometry),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(geometry.is_none());
}

#[test]
fn trim_vertex_vector_transfer_keeps_exact_backing_and_ambient_lifetime() {
    // Both grammars insert at most three u32 slots into one amortized vector.
    // The core growth rule admits four slots; popping the vertex keeps capacity.
    let backing = u64::try_from(4 * std::mem::size_of::<u32>()).expect("small vector");
    for payload in [b"\xf8\x02\x09\x0a\x03\x00".as_slice(), b"\x09\x0a\x03\x00"] {
        for cap in 0..=backing {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || trim_vertex_entry(&ctx, payload, 0, payload.len());
            if cap == backing {
                assert_eq!(run().expect("exact retained backing"), Some((vec![9, 10], 3, payload.len())));
                let original = ctx.charge_retained_limit(1, "after retained trim vector")
                    .expect_err("backing remains retained");
                assert_eq!((original.used, original.additional), (backing, 1));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                let original = ctx.resource_refusal();
                assert!(original.is_none());
                let result = run();
                let original = ctx.resource_refusal().expect("retained transfer refusal");
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::RetainedBytes, 0, backing, "creo trim vertex entities"));
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
        for overflow in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
            let allowance = EMPTY_ROOT_MATERIALIZED_BYTES;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut parent = ctx.reserve_scoped(37, "trim ambient parent").expect("parent");
            let entry = parent.with_storage(|| trim_vertex_entry(&ctx, payload, 0, payload.len()))
                .expect("child backing transfers once").expect("complete vertex");
            assert_eq!(entry.0, [9, 10]);
            let live = 37 + backing;
            let probe = ctx.reserve_scoped(allowance - live + u64::from(overflow), "trim live counter");
            if overflow {
                assert!(matches!(probe, Err(CodecError::ResourceLimit(actual))
                    if actual.dimension == ResourceDimension::MaterializedBytes
                        && actual.used == live && actual.additional == allowance - live + 1));
            } else {
                drop(probe.expect("exact remaining materialized allowance"));
                drop(entry);
                drop(parent);
                drop(ctx.reserve_scoped(allowance, "trim cleanup").expect("all scratch released"));
                let original = ctx.charge_retained_limit(1, "trim retained cleanup")
                    .expect_err("no child backing retained under parent");
                assert_eq!((original.used, original.additional), (0, 1));
            }
        }
    }
}

#[test]
fn trim_intersection_traversals_visit_present_rows_and_unordered_pairs_once() {
    let segments = super::FeatureSegmentTable { declared_count: 3, has_elided_prototype: false,
        entity_ref: None, offset: 0, rows: vec![
            ([1, 2], 9), ([3, 4], 10), ([5, 6], 11),
        ].into_iter().map(|(points, id)| {
            let mut segment = isolated_segment();
            segment.kind = super::FeatureSegmentKind::Line(points);
            segment.external_id = id;
            crate::feature::segment_rows::SegmentRow::Ordinary(segment)
        }).collect() };
    let variables = crate::feature::definitions::test_support::with_points(
        super::FeatureVariableTable { declared_count: 0, entity_ref: None,
            rows: Vec::new(), offset: 0 },
        vec![(-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0),
            (0.0, -2.0), (0.0, 2.0)].into_iter().enumerate().map(|(index, (u, v))|
                super::FeatureSectionPoint { point_id: u32::try_from(index + 1).expect("six points"),
                    u: Some(u), v: Some(v) }).collect());
    let mut duplicates = vec![9, 9];
    duplicates.extend(std::iter::repeat_n(10, 512));
    for (ids, expected, counts) in [(vec![9, 10, 11], Some([0.0, 0.0]), [3, 3, 3, 3]),
        (duplicates, None, [2, 0, 0, 0])] {
        let mut observed = [const { std::collections::BTreeSet::new() }; 4];
        let operations = ["creo trim intersection entities", "creo trim carrier traversal",
            "creo trim carrier pairs", "creo trim intersections"];
        let mut cap = 0;
        loop {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            assert!(cap <= policy.limits.max_work_units);
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let fixture_arena = DecodeArena::new();
            let fixture_policy = DecodePolicy::service();
            let (fixture_ctx, _) = DecodeContext::from_root_bytes(&[], &fixture_arena, &fixture_policy)
                .expect("fixture root");
            let mut geometry = Some(variables.reconciled_trim_geometry(&fixture_ctx)
                .expect("fixed fixture cache"));
            match entity_intersection_cached(&ctx, &ids, Some(&segments), Some(&variables), &mut geometry) {
                Ok(actual) => { assert_eq!(actual, expected); break; }
                Err(CodecError::ResourceLimit(original)) => {
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    if let Some(index) = operations.iter().position(|operation| *operation == original.operation) {
                        assert_eq!(original.additional, 1);
                        observed[index].insert(original.used);
                    }
                    assert!(matches!(entity_intersection_cached(&ctx, &ids, Some(&segments),
                        Some(&variables), &mut geometry),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                    cap = original.used.checked_add(original.additional).expect("bounded fixture work");
                }
                Err(error) => panic!("unexpected trim route error: {error:?}"),
            }
        }
        // Three present entities and carriers make three unordered pairs.
        // Each outer carrier is visited once, including the final empty suffix.
        assert_eq!(observed.map(|boundaries| boundaries.len()), counts);
    }
}
