// SPDX-License-Identifier: Apache-2.0
//! Selection reuse, refusal propagation and scoped cache storage.

use std::cell::Cell;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::PcurveId;

use super::{PcurveSelections, Selection};
use crate::reader::topology::{PcurveSelectionFailure, SelectedPcurve};

fn candidate() -> PcurveId {
    PcurveId::mint("step:data:pcurve#33").expect("valid pcurve identity")
}

#[test]
fn completed_pcurve_ranges_are_reused_with_each_use_identity() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = PcurveSelections::new(&ctx).expect("empty cache");
    let candidates = [candidate()];
    for (edge, range) in [(42, Some([1.0, -2.0])), (45, None)] {
        let first = cache
            .select(edge, 26, &candidates, &ctx, || {
                Ok(SelectedPcurve {
                    id: candidates[0].try_clone_for_decode(&ctx, "test first pcurve")?,
                    parameter_range: range,
                })
            })
            .expect("first selection");
        let second = cache
            .select(edge, 26, &candidates, &ctx, || {
                panic!("completed selection must not run its witness twice")
            })
            .expect("reused selection");
        assert_eq!(first.id, candidates[0]);
        assert_eq!(second.id, candidates[0]);
        assert_eq!(first.parameter_range, range);
        assert_eq!(second.parameter_range, range);
    }
}

fn reused_miss(miss: Selection) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = PcurveSelections::new(&ctx).expect("empty cache");
    let candidates = [candidate()];
    let result = cache.select(42, 26, &candidates, &ctx, || match miss {
        Selection::Carrier => Err(PcurveSelectionFailure::Carrier),
        Selection::Endpoint => Err(PcurveSelectionFailure::Endpoint),
        Selection::Locus => Err(PcurveSelectionFailure::Locus),
        Selection::Accepted(_) => panic!("miss fixture"),
    });
    let cached = cache.select(42, 26, &candidates, &ctx, || {
        panic!("completed geometric miss must not be evaluated twice")
    });
    for error in [
        result.err().expect("first miss"),
        cached.err().expect("cached miss"),
    ] {
        assert!(matches!(
            (miss, error),
            (Selection::Carrier, PcurveSelectionFailure::Carrier)
                | (Selection::Endpoint, PcurveSelectionFailure::Endpoint)
                | (Selection::Locus, PcurveSelectionFailure::Locus)
        ));
    }
}

#[test]
fn completed_pcurve_carrier_misses_are_reused() {
    reused_miss(Selection::Carrier);
}

#[test]
fn completed_pcurve_endpoint_misses_are_reused() {
    reused_miss(Selection::Endpoint);
}

#[test]
fn completed_pcurve_locus_misses_are_reused() {
    reused_miss(Selection::Locus);
}

#[test]
fn completed_pcurve_keys_distinguish_source_edges_and_surfaces() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = PcurveSelections::new(&ctx).expect("empty cache");
    let candidates = [candidate()];
    let calls = Cell::new(0);
    for (edge, surface) in [(42, 26), (45, 26), (42, 28)] {
        assert!(matches!(
            cache.select(edge, surface, &candidates, &ctx, || {
                calls.set(calls.get() + 1);
                Err(PcurveSelectionFailure::Endpoint)
            }),
            Err(PcurveSelectionFailure::Endpoint)
        ));
    }
    assert_eq!(calls.get(), 3);
}

#[test]
fn pcurve_resource_refusals_do_not_complete_cache_entries() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let refusal_ctx = cadmpeg_test_support::service_decode_context();
    let CodecError::ResourceLimit(limit) = refusal_ctx
        .charge_work(u64::MAX, "test witness refusal")
        .expect_err("resource refusal")
    else {
        panic!("work refusal");
    };
    let mut cache = PcurveSelections::new(&ctx).expect("empty cache");
    let candidates = [candidate()];
    for resource_variant in [false, true] {
        let error = cache
            .select(42, 26, &candidates, &ctx, || {
                Err(if resource_variant {
                    PcurveSelectionFailure::Resource(CodecError::ResourceLimit(limit))
                } else {
                    PcurveSelectionFailure::ResourceLimit(limit)
                })
            })
            .err()
            .expect("witness refusal propagates");
        assert!(matches!(error,
            PcurveSelectionFailure::ResourceLimit(found)
            | PcurveSelectionFailure::Resource(CodecError::ResourceLimit(found)) if found == limit));
        assert!(cache.completed.is_empty());
    }
    assert!(matches!(
        cache.select(42, 26, &candidates, &ctx, || {
            Err(PcurveSelectionFailure::Locus)
        }),
        Err(PcurveSelectionFailure::Locus)
    ));
}

#[test]
fn ambiguous_pcurve_candidates_do_not_populate_selection_cache() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = PcurveSelections::new(&ctx).expect("empty cache");
    let candidates = [candidate(), candidate()];
    let calls = Cell::new(0);
    for _ in 0..2 {
        assert!(matches!(
            cache.select(42, 26, &candidates, &ctx, || {
                calls.set(calls.get() + 1);
                Err(PcurveSelectionFailure::NotUnique { count: 2 })
            }),
            Err(PcurveSelectionFailure::NotUnique { count: 2 })
        ));
    }
    assert_eq!(calls.get(), 2);
    assert!(cache.completed.is_empty());
}

#[test]
fn completed_pcurve_cache_refuses_unadmitted_storage() {
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            _ => panic!("cache storage dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty input");
        let mut cache = PcurveSelections::new(&ctx).expect("empty cache needs no storage");
        let error = cache
            .select(42, 26, &[candidate()], &ctx, || {
                Err(PcurveSelectionFailure::Carrier)
            })
            .err()
            .expect("cache storage refuses");
        assert!(matches!(error,
            PcurveSelectionFailure::Resource(CodecError::ResourceLimit(limit))
                if limit.dimension == dimension
                    && limit.operation == "step_completed_pcurve_selections"));
        assert!(cache.completed.is_empty());
    }
}

#[test]
fn completed_pcurve_hit_refuses_unadmitted_output_identity() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty input");
    let mut cache = PcurveSelections::new(&ctx).expect("empty cache");
    let candidates = [candidate()];
    // Supply a completed witness without allocating an output identity in the context.
    cache
        .select(42, 26, &candidates, &ctx, || {
            Ok(SelectedPcurve {
                id: candidate(),
                parameter_range: None,
            })
        })
        .expect("completed witness");
    let error = cache
        .select(42, 26, &candidates, &ctx, || {
            panic!("cache hit must not repeat the witness")
        })
        .err()
        .expect("output identity refuses");
    assert!(matches!(error,
        PcurveSelectionFailure::Resource(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "step_selected_pcurve_id"
                && Some(limit) == ctx.resource_refusal()));
}

#[test]
fn completed_pcurve_cache_releases_scoped_storage() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = PcurveSelections::new(&ctx).expect("empty cache");
    assert!(matches!(
        cache.select(42, 26, &[candidate()], &ctx, || {
            Err(PcurveSelectionFailure::Carrier)
        }),
        Err(PcurveSelectionFailure::Carrier)
    ));
    drop(cache);
    let CodecError::ResourceLimit(limit) = ctx
        .reserve_scoped(u64::MAX, "test cache release")
        .expect_err("storage probe")
    else {
        panic!("materialized refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.used, 0);
}
