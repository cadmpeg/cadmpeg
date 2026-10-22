// SPDX-License-Identifier: Apache-2.0
//! Part rollback, scoped limits, and source framing without invented reads.

use std::collections::BTreeMap;
use std::io;

use crate::decode::{
    ByteRange, DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, LimitScope, ResourceDimension,
};
use crate::CodecError;

#[test]
fn captured_extent_preserves_named_alias_and_enclosing_record_without_reads() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"A12345Z", &arena, &DecodePolicy::default()).unwrap();
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 1, end: 6 }, "archive/member")
        .unwrap();
    ctx.record("outer", root, || {
        let capture = ctx.capture_part_extent(alias.child(1, 4).unwrap())?;
        assert_eq!(
            capture.into_parts(),
            ("archive/member".into(), 1, 3, Some("outer".into()))
        );
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "outer");
    assert!(captures[0].reads().is_empty());
    let streams = ctx.source_streams().unwrap();
    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|stream| {
        stream.name() == "archive/member"
            && matches!(stream.origin(), crate::decode::SourceOrigin::Alias { parent, extent }
                if parent == "file" && *extent == ByteRange { start: 1, end: 6 })
    }));
}

#[test]
fn extent_capture_alone_creates_no_record_or_semantic_read() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::default()).unwrap();
    let capture = ctx.capture_part_extent(root.child(2, 5).unwrap()).unwrap();
    assert_eq!(capture.into_parts(), ("file".into(), 2, 3, None));
    assert!(ctx.take_record_captures().unwrap().is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn extent_capture_rejects_detached_and_foreign_session_views() {
    let arena = DecodeArena::new();
    let foreign_arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"same", &arena, &DecodePolicy::default()).unwrap();
    let (_, foreign) =
        DecodeContext::from_root_bytes(b"same", &foreign_arena, &DecodePolicy::default()).unwrap();
    assert!(ctx.capture_part_extent(foreign).is_err());
    assert!(ctx
        .capture_part_extent(crate::decode::View::over_retained(b"same"))
        .is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn failed_part_discards_its_reads_and_committed_children_without_losing_siblings() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"A12345Z", &arena, &DecodePolicy::default()).expect("root");
    ctx.record("outer", root, || {
        assert_eq!(root.read_at(0, 1), Some(&b"A"[..]));
        let part = ctx.part_transaction(root.child(1, 6).expect("part extent"), || {
            assert_eq!(root.read_at(1, 2), Some(&b"12"[..]));
            ctx.record("discarded-child", root.child(3, 6).expect("child"), || {
                assert_eq!(root.read_at(3, 2), Some(&b"34"[..]));
                Ok(())
            })?;
            Err::<(), _>(CodecError::malformed("invalid part value"))
        })?;
        assert!(matches!(part.result(), Err(CodecError::Malformed(message)) if message == "invalid part value"));
        assert_eq!((part.capture().stream(), part.capture().offset(), part.capture().byte_len(), part.capture().record_id()), ("file", 1, 5, Some("outer")));
        let good = ctx.part_transaction(root.child(5, 7).expect("sibling extent"), || {
            ctx.record("good-child", root.child(5, 7).expect("child extent"), || Ok(root.read_at(5, 2).expect("sibling bytes").to_vec()))
        })?;
        assert_eq!(good.result().expect("decoded sibling"), b"5Z");
        assert_eq!(root.read_at(6, 1), Some(&b"Z"[..]));
        Ok(())
    }).expect("outer survives");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(
        captures
            .iter()
            .map(|capture| capture.id())
            .collect::<Vec<_>>(),
        ["good-child", "outer"]
    );
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![5..7])])
    );
    assert_eq!(
        captures[1].reads(),
        &BTreeMap::from([("file".to_owned(), vec![0..1, 6..7])])
    );
    ctx.finish_session().expect("session remains valid");
}

#[test]
fn framing_metadata_and_a_successful_value_do_not_invent_a_record_or_byte_read() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::default()).expect("root");
    let entry = ctx
        .register_named_slice(root, ByteRange { start: 1, end: 5 }, "archive/member")
        .expect("named source");
    let part = ctx
        .part_transaction(entry.child(1, 3).expect("part"), || Ok(17))
        .expect("part");
    assert_eq!(part.capture().record_id(), None);
    let (capture, result) = part.into_parts();
    assert_eq!(
        capture.into_parts(),
        ("archive/member".to_owned(), 1, 2, None)
    );
    assert_eq!(result.expect("value"), 17);
    assert!(ctx.take_record_captures().expect("captures").is_empty());
}

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
            .part_transaction(root, || Err::<(), _>(error))
            .expect("bounded failure");
        assert_eq!(part.result().expect_err("given up").to_string(), expected);
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
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"compressed source", &arena, &policy).expect("root");
    let first = ctx
        .part_transaction(root, || {
            let mut writer = ctx.begin_expand(ExpandSpec::Unknown)?;
            writer.write(b"abc")?;
            writer.write(b"de")
        })
        .expect("part limit recovers");
    assert!(
        matches!(first.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part && limit.dimension == ResourceDimension::DecompressedBytes && limit.limit == 4)
    );
    assert_eq!(ctx.resource_refusal(), None);
    let second = ctx
        .part_transaction(root, || {
            let mut writer = ctx.begin_expand(ExpandSpec::Exact(1))?;
            writer.write(b"x")?;
            Ok(writer
                .finalize_from(root, "successful sibling")?
                .window()
                .to_vec())
        })
        .expect("remaining one byte");
    assert_eq!(second.result().expect("decoded"), b"x");
    let error = ctx
        .part_transaction(root, || ctx.begin_expand(ExpandSpec::Exact(1)).map(drop))
        .expect_err("cumulative four bytes remain charged");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::DecompressedBytes && limit.used == 4)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Session)
    );
}

#[test]
fn swallowed_part_fuse_still_gives_up_and_clears_no_successful_ancestor_read() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABC", &arena, &policy).expect("root");
    ctx.record("outer", root, || {
        assert_eq!(root.read_at(0, 1), Some(&b"A"[..]));
        let part = ctx.part_transaction(root.child(1, 3).expect("part"), || {
            assert_eq!(root.read_at(1, 1), Some(&b"B"[..]));
            assert!(ctx.begin_expand(ExpandSpec::Exact(2)).is_err());
            Ok(99)
        })?;
        assert!(matches!(part.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part));
        assert_eq!(root.read_at(2, 1), Some(&b"C"[..]));
        Ok(())
    }).expect("surviving outer");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![0..1, 2..3])])
    );
}

#[test]
fn recursion_part_fuse_recovers_and_a_following_part_can_run() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("root");
    let first = ctx
        .part_transaction(root, || ctx.enter_nested("nested part parser").map(drop))
        .expect("part recovery");
    assert!(
        matches!(first.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part && limit.dimension == ResourceDimension::RecursionDepth)
    );
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(
        *ctx.part_transaction(root, || Ok(7))
            .expect("sibling")
            .result()
            .expect("decoded"),
        7
    );
    ctx.finish_session().expect("session");
}

#[test]
fn part_entry_depth_refusal_recovers_without_running_the_callback() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("root");
    for _ in 0..2 {
        let part = ctx
            .part_transaction(root, || -> Result<(), CodecError> {
                panic!("unadmitted callback")
            })
            .expect("empty result is still recoverable");
        assert!(
            matches!(part.result(), Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Part)
        );
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().expect("all parts failed");
}

#[test]
fn a_preexisting_part_fuse_is_not_owned_or_cleared_by_a_later_runner() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("root");
    assert!(ctx.begin_expand(ExpandSpec::Exact(2)).is_err());
    let before = ctx.resource_refusal().expect("existing fuse");
    let error = ctx
        .part_transaction(root, || -> Result<(), CodecError> {
            panic!("preexisting fault prevents entry")
        })
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
    let (ctx, root) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("root");
    let error = ctx
        .part_transaction(root, || {
            assert!(ctx.charge_entities(1, "actual entity admission").is_err());
            Ok(())
        })
        .expect_err("session error must escape");
    let CodecError::ResourceLimit(expected) = error else {
        panic!("resource refusal")
    };
    assert_eq!(expected.scope, LimitScope::Session);
    assert!(
        matches!(ctx.part_transaction(root, || Ok(())), Err(CodecError::ResourceLimit(limit)) if limit == expected)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == expected)
    );
}

#[test]
fn io_error_escapes_and_rolls_back_the_parts_record_captures() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default()).expect("root");
    let error = ctx
        .part_transaction(root, || {
            ctx.record("discarded", root, || {
                assert_eq!(root.read_at(0, 1), Some(&b"A"[..]));
                Ok(())
            })?;
            Err::<(), _>(CodecError::Io(io::Error::other("source unavailable")))
        })
        .expect_err("I/O must escape");
    assert!(matches!(error, CodecError::Io(error) if error.to_string() == "source unavailable"));
    assert!(ctx.take_record_captures().expect("captures").is_empty());
}

#[test]
fn part_recovery_removes_derived_parent_reads_and_preserves_a_successful_overlap() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_per_expand = 3;
    let (ctx, root) = DecodeContext::from_root_bytes(b"0123456789", &arena, &policy).expect("root");
    let mut expanded = ctx.begin_expand(ExpandSpec::Exact(3)).expect("expand");
    expanded.write(b"abc").expect("write");
    let expanded = expanded
        .finalize_from(root.child(2, 8).expect("source"), "expanded")
        .expect("expanded");
    ctx.record("outer", root, || {
        assert_eq!(root.read_at(2, 1), Some(&b"2"[..]));
        let part = ctx.part_transaction(expanded, || {
            assert_eq!(expanded.read_at(1, 1), Some(&b"b"[..]));
            Err::<(), _>(ctx.refuse_part_limit(
                ResourceDimension::Entities,
                0,
                0,
                1,
                "fixed per-part entity limit",
            ))
        })?;
        assert!(part.result().is_err());
        assert_eq!(root.read_at(8, 1), Some(&b"8"[..]));
        Ok(())
    })
    .expect("outer survives");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![2..3, 8..9])])
    );
}

#[test]
fn unrelated_inner_record_does_not_hide_the_actual_enclosing_record() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789", &arena, &DecodePolicy::default())
            .expect("root");
    ctx.record("outer", root, || {
        ctx.record(
            "unrelated-inner",
            root.child(0, 2).expect("unrelated"),
            || {
                let part = ctx.part_transaction(root.child(6, 9).expect("part"), || Ok(()))?;
                assert_eq!(part.capture().record_id(), Some("outer"));
                Ok(())
            },
        )
    })
    .expect("records");
    let captures = ctx.take_record_captures().expect("captures");
    assert!(captures.iter().all(|capture| capture.reads().is_empty()));
}

#[test]
fn a_part_outside_every_active_record_has_no_invented_owner() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789", &arena, &DecodePolicy::default())
            .expect("root");
    ctx.record("unrelated", root.child(0, 2).expect("record"), || {
        let part = ctx.part_transaction(root.child(6, 9).expect("part"), || Ok(()))?;
        assert_eq!(part.capture().record_id(), None);
        Ok(())
    })
    .expect("record");
}

#[test]
fn same_stream_part_links_the_innermost_enclosing_record() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789", &arena, &DecodePolicy::default())
            .expect("root");
    ctx.record("outer", root, || {
        ctx.record("inner", root.child(3, 8).expect("inner"), || {
            let part = ctx.part_transaction(root.child(4, 7).expect("part"), || Ok(()))?;
            assert_eq!(part.capture().record_id(), Some("inner"));
            Ok(())
        })
    })
    .expect("records");
}

#[test]
fn alias_and_expanded_parts_link_only_a_proven_containing_source_extent() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789abcdef", &arena, &DecodePolicy::default())
            .expect("root");
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 4, end: 12 }, "entry")
        .expect("entry");
    let mut expanded = ctx.begin_expand(ExpandSpec::Exact(3)).expect("expand");
    expanded.write(b"ABC").expect("write");
    let expanded = expanded
        .finalize_from(alias.child(1, 6).expect("compressed extent"), "decoded")
        .expect("decoded");
    ctx.record("parent", root.child(5, 10).expect("parent extent"), || {
        let stored_part =
            ctx.part_transaction(alias.child(1, 3).expect("stored part"), || Ok(()))?;
        assert_eq!(stored_part.capture().record_id(), Some("parent"));
        let decoded_part = ctx.part_transaction(expanded, || Ok(()))?;
        assert_eq!(decoded_part.capture().record_id(), Some("parent"));
        let outside_part = ctx.part_transaction(
            alias.child(0, 3).expect("part crossing parent boundary"),
            || Ok(()),
        )?;
        assert_eq!(outside_part.capture().record_id(), None);
        Ok(())
    })
    .expect("parent");
    assert!(ctx.take_record_captures().expect("captures")[0]
        .reads()
        .is_empty());
}

#[test]
fn every_composite_segment_must_be_inside_the_claimed_parent_record() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789", &arena, &DecodePolicy::default())
            .expect("root");
    let inside = ctx
        .concat_named_views(
            &[
                root.child(1, 3).expect("first"),
                root.child(5, 7).expect("second"),
            ],
            "inside",
        )
        .expect("inside composite");
    let crossing = ctx
        .concat_named_views(
            &[
                root.child(1, 3).expect("first"),
                root.child(7, 9).expect("outside"),
            ],
            "crossing",
        )
        .expect("crossing composite");
    ctx.record("parent", root.child(1, 8).expect("parent"), || {
        assert_eq!(
            ctx.part_transaction(inside, || Ok(()))?
                .capture()
                .record_id(),
            Some("parent")
        );
        assert_eq!(
            ctx.part_transaction(crossing, || Ok(()))?
                .capture()
                .record_id(),
            None
        );
        Ok(())
    })
    .expect("parent");
}
