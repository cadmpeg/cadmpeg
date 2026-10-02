// SPDX-License-Identifier: Apache-2.0
use super::super::NurbsError;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn knot_edit_preserves_original_curve_storage_on_caller_refusal() {
    let original = super::curve();
    let count = original.knots().len();
    let visits = u64::try_from(count * 4).expect("four passes");
    for cap in 0..visits {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut curve = original.clone();
        let called = std::cell::Cell::new(false);
        let Err(CodecError::ResourceLimit(limit)) = curve.edit_knots(&ctx, |knots| {
            called.set(true);
            for value in knots { *value += 2.; }
        }) else { panic!("every copy/edit/invariant pass needs admission"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(called.get(), cap >= u64::try_from(count * 2).expect("copy and edit"));
        assert_eq!(curve, original);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::RetainedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = u64::try_from(count * 8 - 1).expect("bytes"),
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = u64::try_from(count - 1).expect("slots"),
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = u64::try_from(count * 8 - 1).expect("bytes"),
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut curve = original.clone();
        let Err(CodecError::ResourceLimit(limit)) = curve.edit_knots(&ctx, |knots| knots.fill(2.)) else { panic!("candidate storage and final retention need admission"); };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(curve, original);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}

#[test]
fn knot_edit_commits_scoped_candidate_once_and_keeps_geometry_error_order() {
    let mut curve = super::curve();
    let original = curve.clone();
    let count = curve.knots().len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(count * 8).expect("bytes");
    policy.limits.max_retained_bytes = policy.limits.max_materialized_bytes;
    policy.limits.max_collection_items = u64::try_from(count).expect("slots");
    policy.limits.max_work_units = u64::try_from(count * 4).expect("four passes");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    curve.edit_knots(&ctx, |knots| { for knot in knots { *knot += 2.; } }).expect("admission").expect("valid edit");
    assert_eq!(curve.knots().as_slice(), original.knots().iter().map(|knot| knot + 2.).collect::<Vec<_>>());
    assert_eq!(curve.pole_rows(), original.pole_rows());
    ctx.reserve_scoped(policy.limits.max_materialized_bytes, "released knot candidate").expect("candidate scope released");
    ctx.finish_session().expect("exact limits");

    let ctx = cadmpeg_test_support::service_decode_context();
    let before = curve.clone();
    let Err(NurbsError::Structure(message)) = curve.edit_knots(&ctx, |knots| {
        knots.reverse();
        knots[0] = f64::INFINITY;
    }).expect("semantic failure") else { panic!("nonfinite knots precede order refusal"); };
    assert_eq!(message, "knots contains a non-finite value");
    assert_eq!(curve, before);
}

#[test]
fn knot_replacement_moves_admitted_output_without_copying_poles() {
    let original = super::curve();
    let knots = original.knots().iter().map(|knot| knot + 2.).collect::<Vec<_>>();
    let address = knots.as_ptr();
    let count = knots.len();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = u64::try_from(count * 2).expect("two invariant passes");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let curve = original.clone().with_knots(&ctx, knots).expect("admission").expect("valid replacement");
    assert_eq!(curve.knots().as_slice().as_ptr(), address);
    assert_eq!(curve.pole_rows(), original.pole_rows());
    ctx.finish_session().expect("moved storage");
    for cap in [0, u64::try_from(count).expect("count")] {
        let arena = DecodeArena::new();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) = original.clone().with_knots(&ctx, original.knots().to_vec()) else { panic!("replacement scans use caller account"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}
