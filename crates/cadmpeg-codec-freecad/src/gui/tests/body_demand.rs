//! Demand-driven body payload prefix groups.

use super::super::{select_shape_bodies, TopologyIndex};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};
use cadmpeg_ir::CadIr;

fn body(key: &str) -> Body {
    Body {
        id: BodyId::mint(format!("fcstd:model:body#{key}")).expect("body identity"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    }
}

fn ir_with_bodies(keys: &[&str]) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.bodies = keys.iter().map(|key| body(key)).collect();
    ir
}

#[test]
fn body_groups_include_only_requested_exact_prefixes_in_arena_order() {
    let ir = ir_with_bodies(&[
        "a:b:c:0",
        "a:bc:1",
        "a:b:d:2",
        "unrelated:z:0",
        "a",
        "a:x:0",
    ]);
    let payload_ids = ["fcstd:payload#a:b", "fcstd:payload#a"];

    crate::test_support::with_service_context(&[], |ctx| {
        let mut topology = TopologyIndex::new(&ir);
        topology
            .ensure_bodies(ctx, payload_ids)
            .expect("requested body groups");

        assert_eq!(
            topology.bodies.keys().copied().collect::<Vec<_>>(),
            ["a", "a:b"]
        );
        assert_eq!(
            topology.bodies["a:b"]
                .iter()
                .map(|(_, body)| body.as_str())
                .collect::<Vec<_>>(),
            ["fcstd:model:body#a:b:c:0", "fcstd:model:body#a:b:d:2"]
        );
        assert_eq!(
            topology.bodies["a"]
                .iter()
                .map(|(_, body)| body.as_str())
                .collect::<Vec<_>>(),
            [
                "fcstd:model:body#a:b:c:0",
                "fcstd:model:body#a:bc:1",
                "fcstd:model:body#a:b:d:2",
                "fcstd:model:body#a:x:0"
            ]
        );

        let selected = select_shape_bodies(
            ctx,
            &topology.bodies,
            ["fcstd:payload#a:b", "fcstd:payload#a", "fcstd:payload#a:b"],
        )
        .expect("source-order body selections");
        assert_eq!(
            selected.iter().map(BodyId::as_str).collect::<Vec<_>>(),
            [
                "fcstd:model:body#a:b:c:0",
                "fcstd:model:body#a:b:d:2",
                "fcstd:model:body#a:b:c:0",
                "fcstd:model:body#a:bc:1",
                "fcstd:model:body#a:b:d:2",
                "fcstd:model:body#a:x:0",
                "fcstd:model:body#a:b:c:0",
                "fcstd:model:body#a:b:d:2"
            ]
        );
    });
}

#[test]
fn body_candidates_are_reused_for_incremental_provider_prefixes() {
    let ir = ir_with_bodies(&["a:b:0", "c:d:0", "a:b:1", "without-separator"]);
    crate::test_support::with_service_context(&[], |ctx| {
        let mut topology = TopologyIndex::new(&ir);
        topology
            .ensure_bodies(ctx, std::iter::once("fcstd:payload#a:b"))
            .expect("first provider body group");
        let candidate_count = topology.body_candidates.len();
        assert_eq!(candidate_count, 3);
        assert_eq!(topology.bodies.keys().copied().collect::<Vec<_>>(), ["a:b"]);

        topology
            .ensure_bodies(
                ctx,
                [
                    "fcstd:payload#missing",
                    "fcstd:payload#c",
                    "fcstd:payload#a:b",
                ],
            )
            .expect("later provider body groups");
        assert_eq!(topology.body_candidates.len(), candidate_count);
        assert!(topology.body_candidate_storage.is_some());
        assert!(topology.body_storage.is_some());
        assert_eq!(
            topology.bodies.keys().copied().collect::<Vec<_>>(),
            ["a:b", "c"]
        );
        assert_eq!(
            topology.bodies["a:b"].len(),
            2,
            "a repeated request must not duplicate the cached group"
        );
        assert_eq!(
            topology.bodies["c"]
                .iter()
                .map(|(_, body)| body.as_str())
                .collect::<Vec<_>>(),
            ["fcstd:model:body#c:d:0"]
        );
    });
}

#[test]
fn empty_and_unmatched_body_requests_create_no_prefix_groups() {
    let ir = ir_with_bodies(&["a:b:0", "c:d:0"]);
    crate::test_support::with_service_context(&[], |ctx| {
        let mut topology = TopologyIndex::new(&ir);
        topology
            .ensure_bodies(ctx, std::iter::empty::<&str>())
            .expect("empty demand");
        assert!(!topology.body_candidates_built);
        assert!(topology.body_candidates.is_empty());
        assert!(topology.body_candidate_storage.is_none());
        assert!(topology.bodies.is_empty());
        assert!(topology.body_storage.is_none());

        topology
            .ensure_bodies(ctx, std::iter::once("fcstd:payload#missing"))
            .expect("unmatched demand");
        assert!(topology.body_candidates_built);
        assert!(topology.bodies.is_empty());
        assert!(topology.body_storage.is_none());
    });
}

#[test]
fn body_candidate_source_refusal_stops_before_a_long_suffix() {
    let target = body("payload:0");
    let work_cap = crate::test_support::with_service_context(&[], |ctx| {
        let mut ir = CadIr::empty();
        ir.model.bodies.push(target.clone());
        let mut topology = TopologyIndex::new(&ir);
        topology
            .ensure_bodies(ctx, std::iter::once("fcstd:payload#payload"))
            .expect("one body candidate and exact prefix group");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx
            .charge_work(u64::MAX, "test body candidate work oracle")
            .expect_err("the work oracle must exceed the service cap")
        else {
            panic!("expected a work-unit refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        limit.used
    });
    let work_cap_usize = usize::try_from(work_cap).expect("work prefix fits usize");
    let suffix_len = work_cap_usize.saturating_mul(4).saturating_add(32);
    assert!(u64::try_from(suffix_len).expect("suffix length fits u64") > work_cap);

    let mut ir = CadIr::empty();
    ir.model.bodies.push(target);
    ir.model.bodies.extend((0..suffix_len).map(|_| body("x")));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work_cap;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    let mut topology = TopologyIndex::new(&ir);
    let error = topology
        .ensure_bodies(&ctx, std::iter::once("fcstd:payload#payload"))
        .expect_err("candidate source work must stop at the finite cap");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected a work-unit refusal, got {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.limit, work_cap);
    assert!(matches!(
        limit.operation,
        "FCStd GUI body candidate sources"
            | "FCStd GUI body identity key"
            | "FCStd GUI body payload key separator"
    ));
    assert!(limit.used + limit.additional > work_cap);
}

#[test]
fn body_group_refusal_uses_a_cached_source_index() {
    let ir = ir_with_bodies(&["a:b:0", "c:d:0"]);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (index_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service)
        .expect("empty index input is within policy");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty selection input is within policy");
    let mut topology = TopologyIndex::new(&ir);
    topology
        .ensure_bodies(&index_ctx, std::iter::once("fcstd:payload#missing"))
        .expect("cache source candidates before group construction");
    assert!(topology.body_candidates_built);
    assert!(topology.bodies.is_empty());

    let error = topology
        .ensure_bodies(&ctx, std::iter::once("fcstd:payload#a"))
        .expect_err("the first requested body group needs one admitted slot");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "FCStd GUI body payload groups"
                && limit.used == 0
                && limit.additional == 1
    ));
}

#[test]
fn cached_body_prefixes_keep_their_guards_live_and_fuse_empty_selection() {
    let ir = ir_with_bodies(&["a:b:0"]);
    crate::test_support::with_service_context(&[], |ctx| {
        let mut topology = TopologyIndex::new(&ir);
        topology
            .ensure_bodies(ctx, std::iter::once("fcstd:payload#a"))
            .expect("cached body prefix");
        assert!(topology.body_candidate_storage.is_some());
        assert!(topology.body_storage.is_some());
        assert_eq!(topology.bodies["a"].len(), 1);

        ctx.charge_work(u64::MAX, "test fused body refusal")
            .expect_err("test must fuse the decode budget");
        assert!(matches!(
            topology.ensure_bodies(ctx, std::iter::empty::<&str>()),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert!(matches!(
            select_shape_bodies(ctx, &topology.bodies, std::iter::empty::<&str>()),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
    });
}
