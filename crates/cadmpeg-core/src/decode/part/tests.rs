// SPDX-License-Identifier: Apache-2.0
//! Part-scoped limits and fatal errors at the part boundary.

use std::io;

use crate::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, LimitScope, ResourceDimension,
};
use crate::CodecError;

#[test]
fn malformed_truncated_and_unsupported_content_are_recoverable_inside_a_part() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"x", &arena, &DecodePolicy::default()).expect("root");
    let errors = [
        CodecError::malformed("damaged"),
        CodecError::truncated(root.location_at(1), "part scalar"),
        CodecError::NotImplemented("unknown type".into()),
        CodecError::WrongFormat("nested content".into()),
    ];
    for error in errors {
        let expected = error.to_string();
        let part = ctx
            .run_part(|| Err::<(), _>(error))
            .expect("bounded failure");
        assert_eq!(part.expect_err("given up").to_string(), expected);
    }
    ctx.finish_session()
        .expect("all failed parts still permit a result");
}

#[test]
fn part_expansion_refusal_recovers_without_refunding_cumulative_output() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_per_expand = 4;
    policy.limits.max_decompressed_bytes_total = 4;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"compressed source", &arena, &policy).expect("root");
    let first = ctx
        .run_part(|| {
            let mut writer = ctx.begin_expand(ExpandSpec::Unknown)?;
            writer.write(b"abc")?;
            writer.write(b"de")
        })
        .expect("part limit recovers");
    assert!(
        matches!(first, Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part && limit.dimension == ResourceDimension::DecompressedBytes && limit.limit == 4)
    );
    assert_eq!(ctx.resource_refusal(), None);
    let second = ctx
        .run_part(|| {
            let mut writer = ctx.begin_expand(ExpandSpec::Exact(1))?;
            writer.write(b"x")?;
            Ok(writer.finalize()?.window().to_vec())
        })
        .expect("remaining one byte");
    assert_eq!(second.expect("decoded"), b"x");
    let error = ctx
        .run_part(|| ctx.begin_expand(ExpandSpec::Exact(1)).map(drop))
        .expect_err("cumulative four bytes remain charged");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::DecompressedBytes && limit.used == 4)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Session)
    );
}

#[test]
fn swallowed_part_fuse_still_gives_up_the_part() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"ABC", &arena, &policy).expect("root");
    let part = ctx
        .run_part(|| {
            assert!(ctx.begin_expand(ExpandSpec::Exact(2)).is_err());
            Ok(99)
        })
        .expect("part recovery");
    assert!(
        matches!(part, Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part)
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn recursion_part_fuse_recovers_and_a_following_part_can_run() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("root");
    let first = ctx
        .run_part(|| {
            let _part = ctx.enter_nested("part parser")?;
            ctx.enter_nested("nested part parser").map(drop)
        })
        .expect("part recovery");
    assert!(
        matches!(first, Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part && limit.dimension == ResourceDimension::RecursionDepth)
    );
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(
        ctx.run_part(|| Ok(7)).expect("sibling").expect("decoded"),
        7
    );
    ctx.finish_session().expect("session");
}

#[test]
fn a_preexisting_part_fuse_is_not_owned_or_cleared_by_a_later_runner() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("root");
    assert!(ctx.begin_expand(ExpandSpec::Exact(2)).is_err());
    let before = ctx.resource_refusal().expect("existing fuse");
    let error = ctx
        .run_part(|| -> Result<(), CodecError> { panic!("preexisting fault prevents entry") })
        .expect_err("not this runner's fault");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit == before && limit.scope == LimitScope::Part)
    );
    assert_eq!(ctx.resource_refusal(), Some(before));
}

#[test]
fn session_limit_escapes_even_when_the_callback_swallows_it() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("root");
    let error = ctx
        .run_part(|| {
            assert!(ctx.charge_entities(1, "actual entity admission").is_err());
            Ok(())
        })
        .expect_err("session error must escape");
    let CodecError::ResourceLimit(expected) = error else {
        panic!("resource refusal")
    };
    assert_eq!(expected.scope, LimitScope::Session);
    assert!(
        matches!(ctx.run_part(|| Ok(())), Err(CodecError::ResourceLimit(limit)) if limit == expected)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == expected)
    );
}

#[test]
fn io_error_escapes_the_part() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default()).expect("root");
    let error = ctx
        .run_part(|| Err::<(), _>(CodecError::Io(io::Error::other("source unavailable"))))
        .expect_err("I/O must escape");
    assert!(matches!(error, CodecError::Io(error) if error.to_string() == "source unavailable"));
}

#[test]
fn a_fixed_part_limit_is_acknowledged_by_its_runner() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::service()).unwrap();
    let failed = ctx
        .run_part(|| {
            Err::<(), _>(ctx.refuse_part_limit(
                ResourceDimension::RecursionDepth,
                1,
                1,
                1,
                "literal part depth",
            ))
        })
        .unwrap();
    assert!(
        matches!(failed, Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part)
    );
    assert_eq!(ctx.resource_refusal(), None);
}
