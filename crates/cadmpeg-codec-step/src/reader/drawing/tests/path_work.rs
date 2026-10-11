// SPDX-License-Identifier: Apache-2.0
//! Drawing diamond discovery and requested ambiguity use charged visits.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{TargetResolution, WrapperCache};

fn diamond() -> (crate::parse::Exchange, BTreeMap<u64, BTreeSet<String>>) {
    let mut records = String::new();
    for id in 1..=8 {
        let next = ((id - 1) / 2 + 1) * 2 + 1;
        write!(records, "#{id}=MAPPED_ITEM('',#{},$);#{}=REPRESENTATION_MAP($,#{});#{}=REPRESENTATION('',(#{next},#{}),$);", id + 100, id + 100, id + 200, id + 200, next + 1).expect("diamond wrappers");
    }
    records.push_str("#9=ITEM();#10=ITEM();");
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let exchange = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("diamond exchange").0;
    (exchange, BTreeMap::from([(9, BTreeSet::from([String::from("first")])), (10, BTreeSet::from([String::from("second")]))]))
}

enum Walk {
    Resolve,
    Probe,
    Invalidate,
}

fn refuses_step(operation: &str, walk: Walk) {
    let (exchange, identities) = diamond();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, operation, |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
            let cache = WrapperCache::new(&ctx).expect("cache");
            let result = match walk {
                Walk::Invalidate => cache.resolve(1, &identities, &exchange, true).and_then(|_| cache.invalidate(9)),
                Walk::Resolve => cache.resolve(1, &identities, &exchange, true).map(|_| ()),
                Walk::Probe => cache.probe(1, &identities, &exchange, &mut BTreeSet::new(), &mut ctx.reserve_scoped(0, "diamond probe proofs").expect("scope")).map(|_| ()),
            };
            let CodecError::ResourceLimit(original) = result.expect_err("diamond walk refuses") else { panic!("work refusal") };
            assert_eq!(ctx.resource_refusal(), Some(original));
            drop(cache);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(found)) if found == original));
            Err::<(), _>(CodecError::ResourceLimit(original))
        },
    );
    let CodecError::ResourceLimit(limit) = error else { panic!("work refusal") };
    assert!(limit.limit < 1_000_000, "modest walk budget: {limit:?}");
}

#[test]
fn diamond_wrapper_discovery_refuses_its_worklist_visit() {
    refuses_step("STEP drawing worklist step", Walk::Resolve);
}

#[test]
fn diamond_wrapper_identity_walk_refuses_its_visited_node() {
    refuses_step("STEP wrapper identity step", Walk::Resolve);
}

#[test]
fn diamond_wrapper_invalidation_refuses_its_visited_node() {
    refuses_step("STEP drawing invalidation step", Walk::Invalidate);
}

#[test]
fn opaque_wrapper_fallback_does_not_materialize_discarded_ambiguity() {
    let (exchange, identities) = diamond();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let cache = WrapperCache::new(&ctx).expect("cache");
    let documents = BTreeMap::new();
    let _probe = RefusalProbe::arm(ResourceDimension::MaterializedBytes, "step_drawing_wrapper_identity_text", None);
    {
    let fallback = super::super::target_resolution(1, &identities, &HashSet::new(), &exchange, &documents, Some(&cache), &ctx).expect("opaque fallback");
    let TargetResolution::Resolved(fallback) = fallback else { panic!("opaque selection") };
    assert_eq!(fallback.local_target(), Some("step:data:mapped_item#1"));
    assert_eq!(ctx.resource_refusal(), None);
    }
    // A source-typed target needs the same graph's ambiguity. Its first
    // identity copy reaches the armed materialization boundary.
    let original = {
    let requested = super::super::target_resolution(1, &identities, &HashSet::from([1]), &exchange, &documents, Some(&cache), &ctx);
    match requested {
        Err(CodecError::ResourceLimit(limit)) => limit,
        Err(error) => panic!("unexpected ambiguity error: {error}"),
        Ok(_) => panic!("requested ambiguity must copy its identities"),
    }
    };
    assert_eq!(original.operation, "step_drawing_wrapper_identity_text");
    drop(cache);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(found)) if found == original));
}


#[test]
fn diamond_wrapper_probe_refuses_its_child_visit() {
    refuses_step("STEP drawing wrapper probe child traversal", Walk::Probe);
}

#[test]
fn diamond_wrapper_target_index_refuses_its_completed_child_visit() {
    refuses_step("STEP wrapper target edge traversal", Walk::Resolve);
}
