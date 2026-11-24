// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

use super::*;
use crate::feature::history_states::{
    frame_chain, index_chain, prepare, Closed, Framing, Prepared, Reading,
};
use crate::kernel::parse_carrier;
use crate::test_support::history_carrier::{carrier_bytes, framed_carrier};

/// Source index that a naming tag first lists in the older state of the
/// carrier, and in its head state.
const OLDER: u32 = 5;
const HEAD: u32 = 6;

/// Runs `check` on views over the two-state carrier: the older state holds a
/// sphere solid, and the head state holds a different solid body.
fn with_views<R>(
    limit: usize,
    check: impl FnOnce(&DecodeContext<'_>, &StateViews<'_, '_>) -> R,
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
    index.first = HashMap::from([(OLDER, 0), (HEAD, 1)]);
    let closed = Closed::new(*chain, index);
    check(&ctx, &StateViews::new(&carrier, &closed, limit))
}

fn model(outcome: Result<StateModel, CodecError>) -> Box<CadIr> {
    match outcome.expect("view runs") {
        StateModel::Model(model) => model,
        StateModel::Unavailable => panic!("state unavailable"),
        StateModel::Bounded => panic!("state bounded"),
    }
}

#[test]
fn a_view_decodes_the_state_that_first_lists_the_source() {
    with_views(1 << 20, |ctx, views| {
        let older = model(views.after(ctx, OLDER));
        assert_eq!(older.geometry.bodies.len(), 1);
        assert_eq!(older.geometry.faces.len(), 1);
        assert!(matches!(
            &older.geometry.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
        ));
        let head = model(views.after(ctx, HEAD));
        assert_eq!(head.geometry.bodies.len(), 1);
        assert_eq!(head.geometry.faces.len(), 1);
    });
}

#[test]
fn a_source_that_no_state_lists_has_no_view() {
    with_views(1 << 20, |ctx, views| {
        assert!(matches!(
            views.after(ctx, 99).expect("view runs"),
            StateModel::Unavailable
        ));
    });
}

#[test]
fn a_view_over_its_work_cap_is_bounded() {
    with_views(0, |ctx, views| {
        assert!(matches!(
            views.after(ctx, OLDER).expect("view runs"),
            StateModel::Bounded
        ));
    });
}

#[test]
fn views_share_one_budget_across_requests() {
    // The older state holds six records: the body, lump, shell, face, surface
    // and the asm header. A cap of seven admits one request and not two.
    with_views(7, |ctx, views| {
        assert!(matches!(
            views.after(ctx, OLDER).expect("view runs"),
            StateModel::Model(_)
        ));
        assert!(matches!(
            views.after(ctx, OLDER).expect("view runs"),
            StateModel::Bounded
        ));
    });
}
