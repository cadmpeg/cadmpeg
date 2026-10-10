// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn fixed_short_loop_prototype_and_frame_routes_are_free() {
    let data = [0xff; 3];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut storage = ctx.reserve_scoped(0, "loop test scratch").expect("scratch");
    assert_eq!(
        super::super::find_named_field(&ctx, &data, 0, data.len(), b"lo_id")
            .expect("no complete field window"),
        None
    );
    assert_eq!(
        super::super::prototype_close(&ctx, &data, 0, data.len(), 42)
            .expect("no complete close window"),
        None
    );
    assert_eq!(
        super::super::named_prototype_end(&ctx, &data, 0, data.len(), 42)
            .expect("no complete prototype"),
        None
    );
    assert_eq!(
        super::super::parse_frame(&ctx, &mut storage, &data, 0, data.len())
            .expect("no frame header"),
        None
    );
    assert_eq!(
        super::super::row_end(&ctx, &data, data.len(), data.len()).expect("no row token"),
        None
    );
}

#[test]
fn missing_loop_prototype_field_stops_before_unexecuted_field_searches() {
    let data = b"padding\xf1\xf7\x2a\xe3";
    assert_eq!(
        super::work_output(|ctx| super::super::named_prototype_end(ctx, data, 0, data.len(), 42)),
        None
    );
}

#[test]
fn complete_loop_prototype_admits_present_windows_and_leaves_following_bytes_free() {
    let prototype = prototype();
    let end = prototype.len();
    let mut trailing = prototype.clone();
    trailing.extend([0xff; 4096]);
    for data in [prototype.as_slice(), trailing.as_slice()] {
        assert_eq!(
            super::work_output(|ctx| super::super::named_prototype_end(
                ctx,
                data,
                0,
                data.len(),
                42
            )),
            Some(end)
        );
    }
}

#[test]
fn empty_loop_array_output_is_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::super::scan(&ctx, &[]).expect("no frame discovery or ordering"),
        super::super::LoopArrayScan::default()
    );
}
