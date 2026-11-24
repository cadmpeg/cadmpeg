// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use cadmpeg_asm::history::walk_entity_versions;
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{work_units, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use std::rc::Rc;

use super::*;
use crate::feature::history_states::{
    frame_chain, index_chain, prepare, Closed, Framing, Prepared, Reading,
};
use crate::kernel::{parse_carrier, ActiveCarrier};
use crate::test_support::history_carrier::{carrier_bytes, framed_carrier};

/// Source index that a naming tag first lists in the older state of the
/// carrier, and in its head state.
const OLDER: u32 = 5;
const HEAD: u32 = 6;
/// Second source index that the older state first lists.
const ALIAS: u32 = 7;

/// Runs `check` on the two-state carrier: the older state holds a sphere
/// solid, and the head state holds a different solid body.
fn with_chain<R>(
    check: impl FnOnce(&DecodeContext<'_>, &ActiveCarrier<'_>, &Closed<'_>) -> R,
) -> R {
    let payload = framed_carrier(&carrier_bytes(RefWidth::Eight, false));
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::default())
        .expect("carrier fits policy");
    let carrier =
        parse_carrier(view, &cadmpeg_ir::identity_key!("token"), 1, 0, 18).expect("carrier parses");
    let header = carrier.header.as_ref().expect("header parses");
    let window = carrier.bytes.window();
    let Framing::Framed(framed) = frame_chain(&ctx, window, header.width).expect("frame") else {
        panic!("the chain frames");
    };
    let Prepared::Ready(chain) = prepare(&ctx, &framed, window, header.width).expect("prepare")
    else {
        panic!("the chain closes");
    };
    // The index of a closed chain lists the oldest state of each source.
    let Reading::Read(mut index) = index_chain(&chain, carrier.schema, 1 << 20) else {
        panic!("the chain reads");
    };
    index.first = HashMap::from([(OLDER, 0), (ALIAS, 0), (HEAD, 1)]);
    let closed = Closed::new(*chain, index);
    check(&ctx, &carrier, &closed)
}

/// Runs `check` on views over the carrier with a work cap of `limit`.
fn with_views<R>(
    limit: usize,
    check: impl FnOnce(&DecodeContext<'_>, &StateViews<'_, '_>) -> R,
) -> R {
    with_chain(|ctx, carrier, closed| check(ctx, &StateViews::new(carrier, closed, limit)))
}

/// Work that the walk charges to reach each state, head state first: the
/// walk charges every state it passes and stops at the requested one.
fn walk_costs(closed: &Closed<'_>) -> Vec<usize> {
    let mut passed = Vec::new();
    walk_entity_versions(
        &closed.views,
        closed.active_count,
        &closed.archived_ids,
        |_, versions| passed.push(work_units(versions.len())),
    );
    passed
        .iter()
        .scan(0, |sum, cost| {
            *sum += cost;
            Some(*sum)
        })
        .collect()
}

fn model(outcome: Result<Rc<StateModel>, CodecError>) -> Rc<StateModel> {
    outcome.expect("view runs")
}

fn decoded(outcome: Result<Rc<StateModel>, CodecError>) -> CadIr {
    match &*model(outcome) {
        StateModel::Model(model) => (**model).clone(),
        StateModel::Unavailable => panic!("state unavailable"),
        StateModel::Failed => panic!("state failed"),
        StateModel::Bounded => panic!("state bounded"),
    }
}

#[test]
fn a_view_decodes_the_state_that_first_lists_the_source() {
    with_views(1 << 20, |ctx, views| {
        let older = decoded(views.after(ctx, OLDER));
        assert_eq!(older.geometry.bodies.len(), 1);
        assert_eq!(older.geometry.faces.len(), 1);
        assert!(matches!(
            &older.geometry.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
        ));
        let head = decoded(views.after(ctx, HEAD));
        assert_eq!(head.geometry.bodies.len(), 1);
        assert_eq!(head.geometry.faces.len(), 1);
    });
}

#[test]
fn a_source_that_no_state_lists_has_no_view_and_builds_no_archive() {
    with_chain(|ctx, carrier, closed| {
        let views = StateViews::new(carrier, closed, 1 << 20);
        assert!(matches!(
            &*model(views.after(ctx, 99)),
            StateModel::Unavailable
        ));
        assert!(!closed.archive_built());
        assert!(matches!(
            &*model(views.after(ctx, OLDER)),
            StateModel::Model(_)
        ));
        assert!(closed.archive_built());
    });
}

#[test]
fn a_view_over_its_work_cap_is_bounded() {
    with_views(0, |ctx, views| {
        assert!(matches!(
            &*model(views.after(ctx, OLDER)),
            StateModel::Bounded
        ));
    });
    // The walk to the older state passes the head state and charges both.
    with_chain(|ctx, carrier, closed| {
        let costs = walk_costs(closed);
        let views = StateViews::new(carrier, closed, costs[1] - 1);
        assert!(matches!(
            &*model(views.after(ctx, OLDER)),
            StateModel::Bounded
        ));
    });
}

#[test]
fn the_walk_stops_at_the_requested_state_and_views_share_one_budget() {
    with_chain(|ctx, carrier, closed| {
        let costs = walk_costs(closed);
        let views = StateViews::new(carrier, closed, costs[0]);
        // The head state is the first state of the walk.
        assert!(matches!(
            &*model(views.after(ctx, HEAD)),
            StateModel::Model(_)
        ));
        assert!(matches!(
            &*model(views.after(ctx, OLDER)),
            StateModel::Bounded
        ));
    });
}

#[test]
fn two_sources_of_one_state_decode_it_once() {
    with_chain(|ctx, carrier, closed| {
        let costs = walk_costs(closed);
        // The cap pays for one walk to the older state only.
        let views = StateViews::new(carrier, closed, costs[1]);
        let first = model(views.after(ctx, OLDER));
        assert!(matches!(&*first, StateModel::Model(_)));
        let second = model(views.after(ctx, ALIAS));
        assert!(Rc::ptr_eq(&first, &second));
    });
}
