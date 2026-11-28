// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use cadmpeg_asm::history::walk_entity_versions;
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{work_units, DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use std::rc::Rc;

use super::*;
use crate::feature::history_states::{
    frame_chain, index_chain, prepare, Closed, Framing, Prepared, Reading, StateIndex,
};
use crate::kernel::{parse_carrier, ActiveCarrier};
use crate::test_support::history_carrier::{
    carrier_bytes, carrier_bytes_with_name, framed_carrier, put_plain, put_sphere_solid,
    CURRENT_BODY_KEY, REF, SPHERE_BODY_KEY,
};

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
    with_edited_index(|_| {}, check)
}

/// Runs `check` on the two-state carrier after `edit` changed its state
/// index.
fn with_edited_index<R>(
    edit: impl FnOnce(&mut StateIndex),
    check: impl FnOnce(&DecodeContext<'_>, &ActiveCarrier<'_>, &Closed<'_>) -> R,
) -> R {
    with_carrier(&carrier_bytes(RefWidth::Eight, false), edit, check)
}

fn with_carrier<R>(
    bytes: &[u8],
    edit: impl FnOnce(&mut StateIndex),
    check: impl FnOnce(&DecodeContext<'_>, &ActiveCarrier<'_>, &Closed<'_>) -> R,
) -> R {
    let payload = framed_carrier(bytes);
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
    edit(&mut index);
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

fn decoded(outcome: Result<Rc<StateModel>, CodecError>) -> Topology {
    match &*model(outcome) {
        StateModel::Model(model) => model.geometry.clone(),
        StateModel::Unavailable => panic!("state unavailable"),
        StateModel::Failed => panic!("state failed"),
        StateModel::Bounded(limit) => panic!("state bounded: {limit}"),
    }
}

#[test]
fn two_live_state_views_share_the_retained_allowance() {
    let name = "n".repeat(1200);
    let bytes = carrier_bytes_with_name(RefWidth::Eight, false, Some(&name));
    with_carrier(
        &bytes,
        |_| {},
        |_, carrier, closed| {
            let views = StateViews::new(carrier, closed, 1 << 20);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 1800;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("document context");
            let older = model(views.after(&ctx, OLDER));
            let StateModel::Model(older_model) = &*older else {
                panic!("one named state fits");
            };
            assert_eq!(
                older_model.geometry.bodies[0].name.as_deref(),
                Some(name.as_str())
            );
            assert_eq!(ctx.retained_bytes_available(), 600);

            let newer = model(views.after(&ctx, HEAD));
            let StateModel::Bounded(StateLimit::Resource(limit)) = &*newer else {
                panic!("the second live state needs another name");
            };
            assert_eq!(
                limit.dimension,
                cadmpeg_core::decode::ResourceDimension::RetainedBytes
            );
            assert_eq!(limit.operation, "keep Inventor history state evidence");
            assert_eq!(limit.additional, 1200);
            assert!(ctx.resource_refusal().is_none());
            assert!(Rc::ptr_eq(&newer, &model(views.after(&ctx, HEAD))));
            assert_eq!(
                older_model.geometry.bodies[0].name.as_deref(),
                Some(name.as_str())
            );
        },
    );
}

#[test]
fn a_view_decodes_the_state_that_first_lists_the_source() {
    with_views(1 << 20, |ctx, views| {
        let older = decoded(views.after(ctx, OLDER));
        assert_eq!(older.bodies.len(), 1);
        assert_eq!(older.faces.len(), 1);
        assert!(matches!(
            &older.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
        ));
        let head = decoded(views.after(ctx, HEAD));
        assert_eq!(head.bodies.len(), 1);
        assert_eq!(head.faces.len(), 1);
    });
}

#[test]
fn a_passthrough_record_over_the_scratch_limit_leaves_room_for_the_document() {
    with_chain(|_, carrier, _| {
        let header = carrier.header.as_ref().expect("carrier header");
        let mut bytes = Vec::new();
        put_plain(&mut bytes, header.width, "asmheader", &[]);
        put_sphere_solid(&mut bytes, header.width, 1, CURRENT_BODY_KEY, -1);
        put_plain(&mut bytes, header.width, "face", &[(REF, -1); 1024]);
        let limit = 8192;
        let policy = DecodePolicy {
            limits: ResourceLimits {
                max_retained_bytes: limit,
                ..ResourceLimits::default()
            },
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input fits");
        let records = cadmpeg_asm::sab::frame(&ctx, &bytes, 0, bytes.len(), header.width, None)
            .expect("sphere and orphan face frame");
        let face = records.last().expect("orphan face record");
        assert_eq!(face.head(), "face");
        assert_eq!(face.ref_at(5), None);
        assert!(cadmpeg_core::decode::u64_from_index(face.len) > limit);
        let archive: HashMap<_, _> = records
            .into_iter()
            .map(|record| {
                (
                    i64::try_from(record.index).expect("record index fits i64"),
                    record,
                )
            })
            .collect();
        let versions = archive.keys().map(|&slot| (slot, slot)).collect();
        let Decoded::Scratch(scratch) =
            decode_scratch(&ctx, header, &archive, &versions, 0, |_, state| Ok(state))
                .expect("an optional state leaves the document decoding")
        else {
            panic!("the sphere fits the scratch allowance");
        };
        assert_eq!(scratch.geometry.bodies.len(), 1);
        assert_eq!(scratch.geometry.faces.len(), 1);
        assert!(matches!(
            &scratch.geometry.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
        ));
        ctx.retain_bytes(limit, "kept document bytes")
            .expect("kept output fits the full allowance");
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
            StateModel::Bounded(StateLimit::Work)
        ));
    });
    // The walk to the older state passes the head state and charges both.
    with_chain(|ctx, carrier, closed| {
        let costs = walk_costs(closed);
        let views = StateViews::new(carrier, closed, costs[1] - 1);
        assert!(matches!(
            &*model(views.after(ctx, OLDER)),
            StateModel::Bounded(StateLimit::Work)
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
            StateModel::Bounded(StateLimit::Work)
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

#[test]
fn the_cache_holds_at_most_its_limit_of_models() {
    let model = Rc::new(StateModel::Unavailable);
    let mut cache = HashMap::new();
    for ordinal in 0..3 {
        keep(&mut cache, 2, ordinal, &model);
        assert!(cache.len() <= 2);
    }
    // The third request found the cache full, dropped both, and kept itself.
    assert_eq!(cache.keys().copied().collect::<Vec<_>>(), [2]);
    // An evicted state decodes again and gives the same answer.
    with_views(1 << 20, |ctx, views| {
        let first = decoded(views.after(ctx, OLDER));
        views.decoded.borrow_mut().clear();
        let second = decoded(views.after(ctx, OLDER));
        assert_eq!(first.faces.len(), second.faces.len());
    });
}

#[test]
fn a_state_introduces_a_key_that_it_holds_and_the_state_before_does_not() {
    with_chain(|_, carrier, closed| {
        let views = StateViews::new(carrier, closed, 1 << 20);
        // The head state holds the current body, and the older state does
        // not.
        assert!(views.introduces(HEAD, CURRENT_BODY_KEY));
        // The older state is the oldest state and has no state before it.
        assert!(!views.introduces(OLDER, SPHERE_BODY_KEY));
        assert!(!views.introduces(ALIAS, SPHERE_BODY_KEY));
        // The head state no longer holds the archived body.
        assert!(!views.introduces(HEAD, SPHERE_BODY_KEY));
        assert!(!views.introduces(HEAD, 99));
    });
}

#[test]
fn a_key_that_both_states_hold_is_not_introduced() {
    with_edited_index(
        |index| index.states[0].body_keys = vec![CURRENT_BODY_KEY, SPHERE_BODY_KEY],
        |_, carrier, closed| {
            let views = StateViews::new(carrier, closed, 1 << 20);
            assert!(!views.introduces(HEAD, CURRENT_BODY_KEY));
        },
    );
}

#[test]
fn a_source_without_a_first_state_introduces_nothing() {
    with_chain(|_, carrier, closed| {
        let views = StateViews::new(carrier, closed, 1 << 20);
        assert!(!views.introduces(99, CURRENT_BODY_KEY));
    });
    with_edited_index(
        |index| {
            index.first.remove(&HEAD);
        },
        |_, carrier, closed| {
            let views = StateViews::new(carrier, closed, 1 << 20);
            assert!(!views.introduces(HEAD, CURRENT_BODY_KEY));
        },
    );
}

#[test]
fn the_state_before_is_the_next_older_state() {
    with_chain(|ctx, carrier, closed| {
        let views = StateViews::new(carrier, closed, 1 << 20);
        // The head state lists HEAD first, so the state before it is the
        // older state, which `after` of OLDER decodes.
        let before = decoded(views.before(ctx, HEAD));
        let older = decoded(views.after(ctx, OLDER));
        assert_eq!(before.faces.len(), older.faces.len());
        assert!(matches!(
            &before.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
        ));
    });
}

#[test]
fn no_state_precedes_the_oldest_state_or_an_unlisted_source() {
    with_chain(|ctx, carrier, closed| {
        let views = StateViews::new(carrier, closed, 1 << 20);
        // OLDER and ALIAS first appear in the state at history position 0.
        for source in [OLDER, ALIAS, 99] {
            assert!(matches!(
                &*model(views.before(ctx, source)),
                StateModel::Unavailable
            ));
        }
        assert!(!closed.archive_built());
    });
}
