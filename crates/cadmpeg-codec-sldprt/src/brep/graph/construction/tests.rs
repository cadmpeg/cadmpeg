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
        edges
            .by_carrier
            .get(&100)
            .unwrap()
            .iter()
            .map(|&index| &edges.faces[index])
            .collect::<Vec<_>>(),
        vec![
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new(),
            &std::collections::BTreeSet::new()
        ]
    );
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index Parasolid surface face edges",
        None,
    );
    assert_eq!(
        indexes
            .face_edges(&ctx)
            .unwrap()
            .by_carrier
            .get(&100)
            .unwrap()
            .len(),
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

#[test]
fn face_edge_groups_borrow_one_cached_set_per_source_face() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::ResourceDimension;
    use std::collections::{BTreeMap, BTreeSet};
    let faces = [
        (10, 100, vec![30, 31, 30]),
        (11, 100, vec![31]),
        (12, 200, vec![30]),
    ]
    .map(
        |(bridge_attr, surface_attr, ring)| crate::brep::graph::WalkedFace {
            bridge_attr,
            surface_attr,
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: vec![(20, ring)],
        },
    );
    let setup = cadmpeg_test_support::service_decode_context();
    let mut tables = crate::brep::topology::Tables::default();
    for (attr, edge) in [(30, 70), (31, 71)] {
        tables
            .insert_coedge(
                &setup,
                crate::brep::topology::Coedge {
                    attr,
                    refs: [0, 20, 0, 0, 0, 0, edge, 0, 0],
                    sense: cadmpeg_ir::topology::Sense::Forward,
                    offset: 0,
                },
            )
            .unwrap();
    }
    let loops = BTreeSet::from([20]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "collect Parasolid face edge sets",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut indexes =
                super::ProceduralSupportIndexes::new(&ctx, &faces, &loops, &tables).unwrap();
            let result = indexes.face_edges(&ctx).map(|_| ());
            drop(indexes);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*limit));
                assert!(
                    matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == *limit)
                );
            }
            result
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("face set backing refusal");
    };
    assert_eq!(
        limit.additional,
        u64::try_from(4 * std::mem::size_of::<BTreeSet<u16>>()).unwrap()
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut indexes = super::ProceduralSupportIndexes::new(&ctx, &faces, &loops, &tables).unwrap();
    let indexed = indexes.face_edges(&ctx).unwrap();
    assert_eq!(
        indexed.faces,
        [
            BTreeSet::from([70, 71]),
            BTreeSet::from([71]),
            BTreeSet::from([70])
        ]
    );
    assert_eq!(
        indexed.by_carrier,
        BTreeMap::from([(100, vec![0, 1]), (200, vec![2])])
    );
    assert!(std::ptr::eq(
        &raw const indexed.faces[0],
        &raw const indexed.faces[indexed.by_carrier[&100][0]]
    ));
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index Parasolid face edges",
        None,
    );
    let reused = indexes.face_edges(&ctx).unwrap();
    assert_eq!(reused.faces.len(), faces.len());
    drop(probe);
    drop(indexes);
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "reuse face edge index storage")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn selected_published_blend_supports_skip_the_support_walk() {
    use crate::test_support::parasolid::{offset_surface_carrier, plane_carrier};
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let setup = cadmpeg_test_support::service_decode_context();
    let mut bytes = plane_carrier(181, [0.; 3], [0., 0., 1.], [1., 0., 0.]);
    bytes.extend(offset_surface_carrier(182, 181, 0.001));
    let carriers = crate::brep::index::scan_carriers(&setup, &bytes).unwrap();
    let faces = [crate::brep::graph::WalkedFace {
        bridge_attr: 10,
        surface_attr: 181,
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: vec![(20, vec![30])],
    }];
    let loops = std::collections::BTreeSet::from([20]);
    let hidden = std::collections::BTreeSet::from([182]);
    let tables = crate::brep::topology::Tables::default();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut indexes = super::ProceduralSupportIndexes::new(&ctx, &faces, &loops, &tables).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "check Parasolid surface support",
        None,
    );
    assert_eq!(
        indexes
            .blend_supports_need_emission(
                &ctx,
                [182, 181],
                crate::brep::index::view::CarrierView::Retained(&carriers),
                &hidden
            )
            .unwrap(),
        Some(false)
    );
    drop(probe);
    drop(indexes);
    ctx.finish_session().unwrap();
}
