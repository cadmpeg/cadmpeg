// SPDX-License-Identifier: Apache-2.0
//! Empty B-rep sources execute no admitted source step.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::super::{component_is_closed, merge_body_components, split_neutral_component_shells};

#[test]
fn empty_brep_traversals_need_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(merge_body_components(&ctx, Vec::new())
        .expect("empty components")
        .is_empty());
    assert!(component_is_closed(
        &ctx,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &[],
    )
    .expect("empty closed component"));
    assert!(split_neutral_component_shells(
        &ctx,
        &[],
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .expect("empty shell partition")
    .is_empty());
}

#[test]
fn brep_circle_loop_early_shapes_are_free() {
    let edge = crate::topology::HalfEdgeId {
        curve_id: 10,
        side: crate::topology::Side::Zero,
    };
    let short = crate::test_support::closed_loop(std::num::NonZeroU32::new(5), vec![edge]);
    let repeated = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5), vec![edge, crate::topology::HalfEdgeId {
            curve_id: edge.curve_id, side: crate::topology::Side::One,
        }],
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut index = crate::decode::surfaces::model_ids::ModelIdentityIndex::new(&ctx)
        .expect("empty identity index");
    let source = crate::decode::source_carriers::SourceUnitCarriers::default();
    for lp in [&short, &repeated] {
        assert!(super::super::native_circle_loop_geometry(
            &ctx, &mut index, lp, &[], &source,
        ).expect("fixed shape rejection").is_none());
    }
}

#[test]
fn brep_circle_order_early_gates_are_free() {
    let lp = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5), vec![crate::topology::HalfEdgeId {
            curve_id: 10, side: crate::topology::Side::Zero,
        }],
    );
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
    );
    let source = crate::decode::source_carriers::SourceUnitCarriers::default();
    let empty: [&crate::topology::Loop; 0] = [];
    let one = [&lp];
    let two = [&lp, &lp];
    let polygons = [Vec::new(), Vec::new()];
    let inputs = [
        (empty.as_slice(), &polygons[..0]),
        (one.as_slice(), &polygons[..1]),
        (two.as_slice(), &polygons[..1]),
        (two.as_slice(), &polygons[..2]),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for (loops, polygons) in inputs {
        assert!(super::super::ordered_two_edge_circle_loops(
            &ctx, loops, polygons, &surface, &[], &source,
        ).expect("fixed ordering gate").is_none());
    }
}

#[test]
fn brep_missing_coedge_pcurve_is_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(super::super::one_coedge_pcurve_use(&ctx, None)
        .expect("missing pcurve").is_empty());
}
