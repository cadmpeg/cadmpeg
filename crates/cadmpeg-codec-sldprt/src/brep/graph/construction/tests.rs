// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const SCRATCH_BYTES: u64 = 64 * 1024;

#[test]
fn support_walk_releases_membership_without_retained_output() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let carriers = crate::brep::index::CarrierIndex::default();
    for _ in 0..64 {
        assert!(super::support_is_acyclic(
            &ctx,
            42,
            crate::brep::index::view::CarrierView::Retained(&carriers)
        )
        .unwrap());
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test released support walk",
            )
            .unwrap(),
        );
    }
    ctx.finish_session().unwrap();
}

#[test]
fn procedural_indexes_need_no_work_or_storage_without_a_consumer() {
    let faces = vec![
        crate::brep::graph::WalkedFace {
            bridge_attr: 10,
            surface_attr: 100,
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: vec![(20, vec![30])],
        };
        100
    ];
    let loops = std::collections::BTreeSet::from([20]);
    let tables = crate::brep::topology::Tables::default();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    drop(super::ProceduralSupportIndexes::new(&ctx, &faces, &loops, &tables).unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn lazy_support_indexes_preserve_minimum_bridge_and_grouped_faces() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::ResourceDimension;
    let faces = [41, 12, 23].map(|bridge_attr| crate::brep::graph::WalkedFace {
        bridge_attr,
        surface_attr: 100,
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: vec![(20, vec![30])],
    });
    let loops = std::collections::BTreeSet::from([20]);
    let tables = crate::brep::topology::Tables::default();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut indexes = super::ProceduralSupportIndexes::new(&ctx, &faces, &loops, &tables).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index Parasolid surface face edges",
        None,
    );
    let emitted = indexes.emitted(&ctx).unwrap();
    assert_eq!(emitted.get(&100), Some(&12));
    drop(probe);
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "select Parasolid face surface carriers",
        None,
    );
    assert_eq!(indexes.emitted(&ctx).unwrap().get(&100), Some(&12));
    drop(probe);
    let edges = indexes.face_edges(&ctx).unwrap();
    assert_eq!(
        edges.get(&100).unwrap(),
        &[
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new()
        ]
    );
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index Parasolid surface face edges",
        None,
    );
    assert_eq!(
        indexes.face_edges(&ctx).unwrap().get(&100).unwrap().len(),
        3
    );
    drop(probe);
    drop(indexes);
    drop(
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test released procedural indexes",
        )
        .unwrap(),
    );
    ctx.finish_session().unwrap();
}
