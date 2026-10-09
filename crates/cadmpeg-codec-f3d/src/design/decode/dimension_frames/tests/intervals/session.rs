// SPDX-License-Identifier: Apache-2.0

use super::{owner_at, parameter_at, scope_at, STREAM};
use crate::design::decode::dimension_frames::CompanionIntervals;
use crate::records::parameters::DesignParameterCompanion;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn companion() -> DesignParameterCompanion {
    DesignParameterCompanion::unbound(
        "f3d:native:parameter-companion#11".into(),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        42,
    )
}

fn native_stream() -> String {
    crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scope(ctx, STREAM, "test companion stream identity").unwrap()
    })
}

#[test]
fn companion_session_preserves_native_interval_and_input_order_then_releases_storage() {
    let stream = native_stream();
    let parameters = [parameter_at(12, 100)];
    let owners = [owner_at(20, 13, 22, 21, 200), owner_at(10, 13, 12, 11, 120)];
    let mut scopes = [scope_at(13, 250, vec![13])];
    scopes[0].id = format!("{stream}:parameter-scope#13");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut intervals = CompanionIntervals::new(&ctx, &parameters, &owners, &scopes, &[]).unwrap();
    assert_eq!(intervals.interval(&stream, &companion(), 300).unwrap(), Some((58, 100)));
    assert_eq!(intervals.interval(&stream, &companion(), 300).unwrap(), Some((58, 100)));
    assert_eq!(intervals.interval("f3d:missing", &companion(), 300).unwrap(), Some((58, 300)));
    let mut actual_owners = intervals.owners_of(&stream, "test companion owner input order").unwrap();
    assert!(std::ptr::eq(actual_owners.next().unwrap(), &owners[0]));
    assert!(std::ptr::eq(actual_owners.next().unwrap(), &owners[1]));
    assert!(actual_owners.next().is_none());
    drop(actual_owners);
    let mut actual_scopes = intervals.scopes_of(&stream, "test companion scope identity").unwrap();
    assert!(std::ptr::eq(actual_scopes.next().unwrap(), &scopes[0]));
    assert!(actual_scopes.next().is_none());
    drop(actual_scopes);
    drop(intervals);
    drop(ctx.reserve_scoped(4096, "after companion interval owner destruction").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn companion_session_keeps_original_fuse_for_occupied_vacant_and_missing_queries() {
    let stream = native_stream();
    let records = [parameter_at(12, 100)];
    for parameters in [&records[..], &[][..]] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (other, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut intervals = CompanionIntervals::new(&ctx, parameters, &[], &[], &[]).unwrap();
        assert_eq!(intervals.interval(&stream, &companion(), 300).unwrap(),
            Some((58, if parameters.is_empty() { 300 } else { 100 })));
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
            policy.limits.max_work_units + 1, "original companion interval session refusal",
        ) else { panic!("original creating session must refuse excess work"); };
        for query in [stream.as_str(), "f3d:missing"] {
            let Err(CodecError::ResourceLimit(later)) = intervals.interval(query, &companion(), 300)
            else { panic!("cached and new streams must preserve the creating session refusal"); };
            assert_eq!(later, original);
            match intervals.owners_of(query, "test refused companion owner query") {
                Err(CodecError::ResourceLimit(later)) => assert_eq!(later, original),
                _ => panic!("owner lookup must preserve the creating session refusal"),
            }
            match intervals.scopes_of(query, "test refused companion scope query") {
                Err(CodecError::ResourceLimit(later)) => assert_eq!(later, original),
                _ => panic!("scope lookup must preserve the creating session refusal"),
            }
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert_eq!(other.resource_refusal(), None);
            other.charge_work(0, "unrelated companion session remains healthy").unwrap();
        }
        drop(intervals);
        other.finish_session().unwrap();
    }
}

#[test]
fn companion_session_empty_queries_need_no_storage_and_keep_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut intervals = CompanionIntervals::new(&ctx, &[], &[], &[], &[]).unwrap();
    assert!(intervals.owners_of("f3d:missing", "test empty companion owners").unwrap().next().is_none());
    assert!(intervals.scopes_of("f3d:missing", "test empty companion scopes").unwrap().next().is_none());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty companion refusal")
    else { panic!("zero-work creating session must refuse excess work"); };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    let Err(CodecError::ResourceLimit(later)) = intervals.interval("f3d:missing", &companion(), 100)
    else { panic!("empty lazy interval query must preserve the original refusal"); };
    assert_eq!(later, original);
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(intervals);
}
