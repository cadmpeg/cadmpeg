// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;
use std::ops::Range;

use crate::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use crate::CodecError;

#[test]
fn disjoint_producers_return_actual_values_in_one_primary_capture() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"xxABC---DEF!", &arena, &DecodePolicy::default()).unwrap();
    let primary = root.child(2, 5).unwrap();
    let second = root.child(8, 11).unwrap();
    let (first, owner) = ctx
        .record_owner("joined", primary, || {
            let mut value = primary;
            Ok(value.u16_le().unwrap())
        })
        .unwrap();
    let last = ctx
        .record_fragment(&owner, second, || {
            let mut value = second;
            Ok(value.u8().unwrap())
        })
        .unwrap();
    assert_eq!(first, u16::from_le_bytes(*b"AB"));
    assert_eq!(last, b'D');
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "joined");
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (2, 3));
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".into(), vec![2..4, 8..9])])
    );
}

#[test]
fn failed_fragments_and_ancestor_transactions_keep_only_committed_contributions() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCDEFGH", &arena, &DecodePolicy::default()).unwrap();
    let (_, owner) = ctx
        .record_owner("joined", root.child(0, 2).unwrap(), || {
            assert_eq!(root.read_at(0, 2), Some(b"AB".as_slice()));
            Ok(())
        })
        .unwrap();
    ctx.record_fragment(&owner, root.child(1, 4).unwrap(), || {
        assert_eq!(root.read_at(1, 3), Some(b"BCD".as_slice()));
        Ok(())
    })
    .unwrap();
    let before = ctx.retained_bytes_available();
    let failed: Result<(), CodecError> =
        ctx.record_fragment(&owner, root.child(3, 7).unwrap(), || {
            assert_eq!(root.read_at(3, 4), Some(b"DEFG".as_slice()));
            ctx.record("discarded-child", root.child(6, 7).unwrap(), || Ok(()))?;
            Err(CodecError::malformed("bad fragment"))
        });
    assert!(failed.is_err());
    assert!(ctx.retained_bytes_available() < before);
    let outer = ctx.read_transaction().unwrap();
    ctx.record_fragment(&owner, root.child(5, 8).unwrap(), || {
        assert_eq!(root.read_at(5, 3), Some(b"FGH".as_slice()));
        Ok(())
    })
    .unwrap();
    let spent = ctx.retained_bytes_available();
    drop(outer);
    assert_eq!(ctx.retained_bytes_available(), spent);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".into(), vec![0..4])])
    );
}

#[test]
fn rolled_back_owner_cannot_resume_or_authorize_a_reused_record_identity() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default()).unwrap();
    let outer = ctx.read_transaction().unwrap();
    let (_, stale) = ctx.record_owner("same", root, || Ok(())).unwrap();
    ctx.record_fragment(&stale, root, || {
        assert_eq!(root.read_at(1, 1), Some(b"B".as_slice()));
        Ok(())
    })
    .unwrap();
    drop(outer);
    let (_, live) = ctx.record_owner("same", root, || Ok(())).unwrap();
    let mut ran = false;
    assert!(ctx
        .record_fragment(&stale, root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    ctx.record_fragment(&live, root, || {
        assert_eq!(root.read_at(2, 1), Some(b"C".as_slice()));
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".into(), vec![2..3])])
    );
}

#[test]
fn part_ownership_uses_each_active_physical_fragment_and_failed_reads_rollback() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABC---DEF", &arena, &DecodePolicy::default()).unwrap();
    let primary = root.child(0, 3).unwrap();
    let second = root.child(6, 9).unwrap();
    let (_, owner) = ctx
        .record_owner("joined", primary, || {
            assert_eq!(
                ctx.capture_part_extent(root.child(0, 1).unwrap())?
                    .record_id(),
                Some("joined")
            );
            Ok(())
        })
        .unwrap();
    ctx.record_fragment(&owner, second, || {
        assert_eq!(ctx.capture_part_extent(primary)?.record_id(), None);
        let failed: crate::decode::PartOutcome<()> =
            ctx.part_transaction(root.child(7, 8).unwrap(), || {
                assert_eq!(root.read_at(7, 1), Some(b"E".as_slice()));
                Err(CodecError::malformed("bad field"))
            })?;
        assert!(failed.result().is_err());
        assert_eq!(failed.capture().record_id(), Some("joined"));
        assert_eq!(failed.capture().offset(), 7);
        assert_eq!(root.read_at(8, 1), Some(b"F".as_slice()));
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".into(), vec![8..9])])
    );
}

#[test]
fn ordinary_duplicates_independent_owners_nested_fragments_and_sealed_tokens_refuse() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default()).unwrap();
    let (_, owner) = ctx.record_owner("same", root, || Ok(())).unwrap();
    let mut ran = false;
    assert!(ctx
        .record("same", root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(ctx
        .record_owner("same", root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    ctx.record_fragment(&owner, root, || {
        assert!(ctx.take_record_captures().is_err());
        assert!(ctx
            .record_fragment(&owner, root, || {
                ran = true;
                Ok(())
            })
            .is_err());
        assert!(!ran);
        Ok(())
    })
    .unwrap();
    assert_eq!(ctx.take_record_captures().unwrap().len(), 1);
    assert!(ctx
        .record_fragment(&owner, root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
}

#[test]
fn foreign_detached_views_foreign_generations_and_cross_batch_tokens_refuse() {
    let arena = DecodeArena::new();
    let (first, old_view) =
        DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default()).unwrap();
    let (_, owner) = first.record_owner("same", old_view, || Ok(())).unwrap();
    let (second, root) =
        DecodeContext::from_root_bytes(b"DEF", &arena, &DecodePolicy::default()).unwrap();
    let mut ran = false;
    assert!(second
        .record_fragment(&owner, root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(first
        .record_fragment(&owner, root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(first
        .record_fragment(&owner, View::over_retained(b"ABC"), || {
            ran = true;
            Ok(())
        })
        .is_err());
    first
        .record_batch(|| {
            assert!(first
                .record_fragment(&owner, old_view, || {
                    ran = true;
                    Ok(())
                })
                .is_err());
            Ok(())
        })
        .unwrap();
    let (batched, _) = first
        .record_batch(|| {
            let (_, batched) = first.record_owner("batch", old_view, || Ok(()))?;
            first.record_fragment(&batched, old_view, || Ok(()))?;
            Ok(batched)
        })
        .unwrap();
    assert!(first
        .record_fragment(&batched, old_view, || {
            ran = true;
            Ok(())
        })
        .is_err());
    drop(first);
    let (replacement, new_view) =
        DecodeContext::from_root_bytes(b"GHI", &arena, &DecodePolicy::default()).unwrap();
    assert!(replacement
        .record_fragment(&owner, new_view, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
}

#[test]
fn context_move_and_unwind_preserve_only_successful_fragment_reads() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default()).unwrap();
    let (_, owner) = ctx.record_owner("joined", root, || Ok(())).unwrap();
    let moved = ctx;
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), CodecError> = moved.record_fragment(&owner, root, || {
            assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
            panic!("fragment unwind")
        });
    }));
    assert!(unwound.is_err());
    moved
        .record_fragment(&owner, root, || {
            assert_eq!(root.read_at(2, 1), Some(b"C".as_slice()));
            Ok(())
        })
        .unwrap();
    let captures = moved.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".into(), vec![2..3])])
    );
}

#[test]
fn a_fragment_session_refusal_stays_fused_without_reopening_or_refunding() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABC", &arena, &policy).unwrap();
    let (_, owner) = ctx.record_owner("joined", root, || Ok(())).unwrap();
    let before = ctx.retained_bytes_available();
    let refusal: Result<(), CodecError> = ctx.record_fragment(&owner, root, || {
        assert_eq!(root.read_at(1, 1), Some(b"B".as_slice()));
        ctx.charge_entities(1, "owner session fuse")?;
        Ok(())
    });
    assert!(
        matches!(refusal, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::Entities)
    );
    let failure = ctx.resource_refusal().unwrap();
    let spent = ctx.retained_bytes_available();
    assert!(spent < before);
    let mut ran = false;
    assert!(ctx
        .record_fragment(&owner, root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    assert_eq!(ctx.resource_refusal(), Some(failure));
    assert_eq!(ctx.retained_bytes_available(), spent);
    assert!(ctx.take_record_captures().is_err());
}

#[test]
fn separate_member_batches_keep_same_named_owners_and_primary_extents_distinct() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCDEFGH", &arena, &DecodePolicy::default()).unwrap();
    let (_, first) = ctx
        .record_batch(|| {
            let (value, owner) = ctx.record_owner("same", root.child(0, 2).unwrap(), || {
                Ok(root.read_at(0, 1).unwrap()[0])
            })?;
            assert_eq!(value, b'A');
            ctx.record_fragment(&owner, root.child(2, 4).unwrap(), || {
                assert_eq!(root.read_at(2, 1), Some(b"C".as_slice()));
                Ok(())
            })
        })
        .unwrap();
    let (_, second) = ctx
        .record_batch(|| {
            let (_, owner) = ctx.record_owner("same", root.child(4, 6).unwrap(), || Ok(()))?;
            ctx.record_fragment(&owner, root.child(6, 8).unwrap(), || {
                assert_eq!(root.read_at(6, 1), Some(b"G".as_slice()));
                Ok(())
            })
        })
        .unwrap();
    assert_ne!(first, second);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    for (batch, offset, reads) in [(first, 0, vec![0..1, 2..3]), (second, 4, vec![6..7])] {
        let capture = captures
            .iter()
            .find(|capture| capture.batch_id() == Some(batch))
            .unwrap();
        assert_eq!(capture.id(), "same");
        assert_eq!((capture.offset(), capture.byte_len()), (offset, 2));
        assert_eq!(capture.reads(), &BTreeMap::from([("file".into(), reads)]));
    }
}

#[test]
fn ordinary_capture_order_and_values_stay_unchanged_with_or_without_fragments() {
    for fragments in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(b"ABCD", &arena, &DecodePolicy::default()).unwrap();
        ctx.record("z", root.child(3, 4).unwrap(), || {
            assert_eq!(root.read_at(3, 1), Some(b"D".as_slice()));
            Ok(())
        })
        .unwrap();
        let owner = if fragments {
            Some(
                ctx.record_owner("m", root.child(1, 2).unwrap(), || {
                    assert_eq!(root.read_at(1, 1), Some(b"B".as_slice()));
                    Ok(())
                })
                .unwrap()
                .1,
            )
        } else {
            None
        };
        ctx.record("a", root.child(0, 1).unwrap(), || {
            assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
            Ok(())
        })
        .unwrap();
        if let Some(owner) = owner {
            ctx.record_fragment(&owner, root.child(2, 3).unwrap(), || {
                assert_eq!(root.read_at(2, 1), Some(b"C".as_slice()));
                Ok(())
            })
            .unwrap();
        }
        let captures = ctx.take_record_captures().unwrap();
        let ids: Vec<_> = captures.iter().map(|capture| capture.id()).collect();
        assert_eq!(
            ids,
            if fragments {
                vec!["a", "m", "z"]
            } else {
                vec!["a", "z"]
            }
        );
        let ordinary: Vec<_> = captures
            .into_iter()
            .filter(|capture| capture.id() != "m")
            .map(|capture| capture.into_parts())
            .collect();
        assert_eq!(
            ordinary,
            vec![
                (
                    "a".into(),
                    "file".into(),
                    0,
                    1,
                    BTreeMap::from([("file".into(), vec![0..1])])
                ),
                (
                    "z".into(),
                    "file".into(),
                    3,
                    1,
                    BTreeMap::from([("file".into(), vec![3..4])])
                ),
            ]
        );
    }
}

#[test]
fn failed_continuation_inside_a_part_keeps_prior_owner_and_containing_link() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCDEFGH", &arena, &DecodePolicy::default()).unwrap();
    let (header, owner) = ctx
        .record_owner("table", root.child(0, 2).unwrap(), || {
            let mut source = root.child(0, 2).unwrap();
            Ok(source.u8().unwrap())
        })
        .unwrap();
    assert_eq!(header, b'A');
    let failed = ctx
        .record("property", root.child(2, 8).unwrap(), || {
            assert_eq!(root.read_at(2, 1), Some(b"C".as_slice()));
            ctx.part_transaction(root.child(4, 6).unwrap(), || {
                ctx.record_fragment(&owner, root.child(4, 6).unwrap(), || {
                    assert_eq!(root.read_at(4, 1), Some(b"E".as_slice()));
                    Err::<(), _>(CodecError::malformed("bad table value"))
                })
            })
        })
        .unwrap();
    assert!(failed.result().is_err());
    assert_eq!(failed.capture().record_id(), Some("property"));
    assert_eq!(
        (failed.capture().offset(), failed.capture().byte_len()),
        (4, 2)
    );
    assert!(failed.capture().attempted_record_ids().is_empty());
    ctx.record_fragment(&owner, root.child(6, 8).unwrap(), || {
        assert_eq!(root.read_at(6, 1), Some(b"G".as_slice()));
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    let table = captures
        .iter()
        .find(|capture| capture.id() == "table")
        .unwrap();
    assert_eq!((table.offset(), table.byte_len()), (0, 2));
    assert_eq!(
        table.reads(),
        &BTreeMap::from([("file".into(), vec![0..1, 6..7])])
    );
    let property = captures
        .iter()
        .find(|capture| capture.id() == "property")
        .unwrap();
    assert_eq!(
        property.reads(),
        &BTreeMap::from([("file".into(), vec![2..3])])
    );
}

#[test]
fn failed_part_still_seeds_ordinary_and_new_primary_records_but_not_continuations() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABCDEFGH", &arena, &DecodePolicy::default()).unwrap();
    let (_, prior) = ctx
        .record_owner("prior", root.child(0, 2).unwrap(), || {
            assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
            Ok(())
        })
        .unwrap();
    let failed = ctx
        .part_transaction(root.child(2, 8).unwrap(), || {
            ctx.record_fragment(&prior, root.child(2, 3).unwrap(), || {
                assert_eq!(root.read_at(2, 1), Some(b"C".as_slice()));
                Ok(())
            })?;
            ctx.record("ordinary", root.child(3, 4).unwrap(), || {
                assert_eq!(root.read_at(3, 1), Some(b"D".as_slice()));
                Ok(())
            })?;
            let (_, fresh) = ctx.record_owner("fresh", root.child(4, 5).unwrap(), || {
                assert_eq!(root.read_at(4, 1), Some(b"E".as_slice()));
                Ok(())
            })?;
            ctx.record_fragment(&fresh, root.child(5, 6).unwrap(), || {
                assert_eq!(root.read_at(5, 1), Some(b"F".as_slice()));
                Ok(())
            })?;
            Err::<(), _>(CodecError::malformed("bad later field"))
        })
        .unwrap();
    assert!(failed.result().is_err());
    assert_eq!(
        failed.capture().attempted_record_ids(),
        &["fresh".to_owned(), "ordinary".to_owned()]
    );
    ctx.record_fragment(&prior, root.child(6, 8).unwrap(), || {
        assert_eq!(root.read_at(7, 1), Some(b"H".as_slice()));
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "prior");
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".into(), vec![0..1, 7..8])])
    );
}

/// Retained bytes that `run` charges, through capture extraction, in a fresh
/// session, and the reads of its `joined` capture.
fn charged_reads(
    run: impl FnOnce(&DecodeContext<'_>, View<'_>, View<'_>) -> Result<(), CodecError>,
) -> (u64, BTreeMap<String, Vec<Range<u64>>>) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"AB--CD--EF--GH", &arena, &DecodePolicy::default())
            .unwrap();
    let member = ctx
        .register_named_slice(root, ByteRange { start: 12, end: 14 }, "member")
        .unwrap();
    ctx.record("warm", root.child(0, 1).unwrap(), || Ok(()))
        .unwrap();
    let before = ctx.retained_bytes_available();
    run(&ctx, root, member).unwrap();
    let captures = ctx.take_record_captures().unwrap();
    let spent = before - ctx.retained_bytes_available();
    let joined = captures
        .iter()
        .find(|capture| capture.id() == "joined")
        .unwrap()
        .reads()
        .clone();
    (spent, joined)
}

#[test]
fn coalescing_fragments_charges_what_one_primary_record_with_the_same_reads_charges() {
    let spans = [(4, 6), (8, 10)];
    let fragments = charged_reads(|ctx, root, member| {
        let (_, owner) = ctx.record_owner("joined", root.child(0, 2).unwrap(), || {
            assert_eq!(root.read_at(0, 2), Some(b"AB".as_slice()));
            Ok(())
        })?;
        for (start, end) in spans {
            ctx.record_fragment(&owner, root.child(start, end).unwrap(), || {
                assert!(root.read_at(start, end - start).is_some());
                Ok(())
            })?;
        }
        // A stream new to the primary record.
        ctx.record_fragment(&owner, member, || {
            assert_eq!(member.read_at(0, 2), Some(b"GH".as_slice()));
            Ok(())
        })
    });
    let primary = charged_reads(|ctx, root, member| {
        ctx.record("joined", root.child(0, 2).unwrap(), || {
            assert_eq!(root.read_at(0, 2), Some(b"AB".as_slice()));
            for (start, end) in spans {
                assert!(root.read_at(start, end - start).is_some());
            }
            assert_eq!(member.read_at(0, 2), Some(b"GH".as_slice()));
            Ok(())
        })
    });
    // The member is bytes 12..14 of the root, so its read is also a root read.
    assert_eq!(
        primary.1,
        BTreeMap::from([
            ("file".into(), vec![0..2, 4..6, 8..10, 12..14]),
            ("member".into(), vec![0..2]),
        ])
    );
    assert_eq!(fragments, primary);
}

#[test]
fn repeated_fragment_ranges_are_charged_once_when_new_to_the_primary_stream() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"ABC---DEF", &arena, &DecodePolicy::default())
            .expect("create tracked disjoint-source decode context");
    let (_, owner) = ctx
        .record_owner(
            "joined",
            root.child(0, 3)
                .expect("select primary record source window"),
            || {
                assert_eq!(root.read_at(0, 3), Some(b"ABC".as_slice()));
                Ok(())
            },
        )
        .expect("declare disjoint-source primary owner");
    let before = ctx.retained_bytes_available();
    for _ in 0..100 {
        ctx.record_fragment(
            &owner,
            root.child(6, 9)
                .expect("select disjoint fragment source window"),
            || {
                assert_eq!(root.read_at(6, 3), Some(b"DEF".as_slice()));
                Ok(())
            },
        )
        .expect("accept repeated disjoint-source fragment");
    }
    assert_eq!(ctx.retained_bytes_available(), before);
    let captures = ctx
        .take_record_captures()
        .expect("extract disjoint-source record captures");
    assert_eq!(
        before - ctx.retained_bytes_available(),
        u64::try_from(size_of::<std::ops::Range<u64>>()).expect("one retained range size fits u64")
    );
    assert_eq!(captures.len(), 1);
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (0, 3));
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".into(), vec![0..3, 6..9])])
    );
}

#[test]
fn one_hundred_fragments_charge_only_two_new_actual_stream_entries() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABCDEF", &arena, &DecodePolicy::default())
        .expect("create tracked multi-stream decode context");
    let a = ctx
        .register_named_slice(
            root,
            crate::decode::ByteRange { start: 0, end: 3 },
            "member-a",
        )
        .expect("register member-a original source slice");
    let b = ctx
        .register_named_slice(
            root,
            crate::decode::ByteRange { start: 3, end: 6 },
            "member-b",
        )
        .expect("register member-b original source slice");
    let (_, owner) = ctx
        .record_owner("joined", root, || {
            assert_eq!(root.read_at(0, 6), Some(b"ABCDEF".as_slice()));
            Ok(())
        })
        .expect("declare multi-stream primary record owner");
    let before = ctx.retained_bytes_available();
    for _ in 0..50 {
        for (member, expected) in [(a, b"A".as_slice()), (b, b"D".as_slice())] {
            ctx.record_fragment(&owner, member, || {
                assert_eq!(member.read_at(0, 1), Some(expected));
                Ok(())
            })
            .expect("accept repeated actual-member fragment");
        }
    }
    assert_eq!(ctx.retained_bytes_available(), before);
    let captures = ctx
        .take_record_captures()
        .expect("extract merged multi-stream record captures");
    let entry = size_of::<(String, Vec<std::ops::Range<u64>>)>();
    let range = size_of::<std::ops::Range<u64>>();
    let retained = entry + "member-a".len() + range + entry + "member-b".len() + range;
    assert_eq!(
        before - ctx.retained_bytes_available(),
        u64::try_from(retained).expect("new stream entry retention size fits u64")
    );
    assert_eq!(captures.len(), 1);
    assert_eq!(
        (
            captures[0].stream(),
            captures[0].offset(),
            captures[0].byte_len()
        ),
        ("file", 0, 6)
    );
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([
            (
                "file".into(),
                std::iter::once(0..6).collect::<Vec<std::ops::Range<u64>>>()
            ),
            (
                "member-a".into(),
                std::iter::once(0..1).collect::<Vec<std::ops::Range<u64>>>()
            ),
            (
                "member-b".into(),
                std::iter::once(0..1).collect::<Vec<std::ops::Range<u64>>>()
            ),
        ])
    );
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn nested_transactions_keep_fragment_read_staging_uncharged_through_every_parent() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABC", &arena, &DecodePolicy::default())
        .expect("create tracked nested-transaction decode context");
    let first = ctx.read_transaction().expect("open first read transaction");
    let second = ctx
        .read_transaction()
        .expect("open second read transaction");
    let third = ctx.read_transaction().expect("open third read transaction");
    third.commit().expect("commit third read transaction");
    second.commit().expect("commit second read transaction");
    first.commit().expect("commit first read transaction");
    let (_, owner) = ctx
        .record_owner("joined", root, || {
            assert_eq!(root.read_at(0, 3), Some(b"ABC".as_slice()));
            Ok(())
        })
        .expect("declare nested-transaction primary record owner");
    let before = ctx.retained_bytes_available();
    for _ in 0..100 {
        ctx.record_fragment(&owner, root, || {
            let outer = ctx.read_transaction()?;
            let inner = ctx.read_transaction()?;
            assert_eq!(root.read_at(0, 3), Some(b"ABC".as_slice()));
            inner.commit()?;
            outer.commit()?;
            Ok(())
        })
        .expect("accept fragment through nested transactions");
        assert_eq!(ctx.retained_bytes_available(), before);
    }
    let captures = ctx
        .take_record_captures()
        .expect("extract nested-transaction record captures");
    assert_eq!(ctx.retained_bytes_available(), before);
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "joined");
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (0, 3));
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([(
            "file".into(),
            std::iter::once(0..3).collect::<Vec<std::ops::Range<u64>>>()
        )])
    );
}

#[test]
fn one_hundred_fragments_of_an_existing_read_fit_the_primary_capture_budget() {
    let id = "joined";
    let retained = size_of::<super::super::SessionSlot>()
        + size_of::<crate::operation_budget::OperationBudget>()
        + size_of::<super::super::Frame>()
        + size_of::<super::super::RecordCapture>()
        + id.len()
        + "file".len()
        + size_of::<std::ops::Range<u64>>()
        + size_of::<(String, Vec<std::ops::Range<u64>>)>()
        + "file".len();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes =
        u64::try_from(retained).expect("retained capture budget fits u64");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABC", &arena, &policy)
        .expect("create tracked primary-source decode context");
    let (_, owner) = ctx
        .record_owner(id, root, || {
            assert_eq!(root.read_at(0, 3), Some(b"ABC".as_slice()));
            Ok(())
        })
        .expect("declare primary record owner");
    assert_eq!(ctx.retained_bytes_available(), 0);
    for _ in 0..100 {
        ctx.record_fragment(&owner, root, || {
            assert_eq!(root.read_at(0, 3), Some(b"ABC".as_slice()));
            Ok(())
        })
        .expect("accept repeated primary-source fragment");
        assert_eq!(ctx.retained_bytes_available(), 0);
    }
    let captures = ctx
        .take_record_captures()
        .expect("extract accepted primary record captures");
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), id);
    assert_eq!(
        (
            captures[0].stream(),
            captures[0].offset(),
            captures[0].byte_len()
        ),
        ("file", 0, 3)
    );
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([(
            "file".into(),
            std::iter::once(0..3).collect::<Vec<std::ops::Range<u64>>>()
        )])
    );
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn a_new_fragment_range_refuses_at_final_retention_without_refunding_or_exposing_captures() {
    let id = "joined";
    let retained = size_of::<super::super::SessionSlot>()
        + size_of::<crate::operation_budget::OperationBudget>()
        + size_of::<super::super::Frame>()
        + size_of::<super::super::RecordCapture>()
        + id.len()
        + "file".len()
        + size_of::<std::ops::Range<u64>>()
        + size_of::<(String, Vec<std::ops::Range<u64>>)>()
        + "file".len();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes =
        u64::try_from(retained).expect("primary-only retained budget fits u64");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABC---DEF", &arena, &policy)
        .expect("create tracked final-retention decode context");
    let (_, owner) = ctx
        .record_owner(
            id,
            root.child(0, 3)
                .expect("select final-retention primary source window"),
            || {
                assert_eq!(root.read_at(0, 3), Some(b"ABC".as_slice()));
                Ok(())
            },
        )
        .expect("declare final-retention primary record owner");
    assert_eq!(ctx.retained_bytes_available(), 0);
    for _ in 0..100 {
        ctx.record_fragment(
            &owner,
            root.child(6, 9)
                .expect("select final-retention fragment source window"),
            || {
                assert_eq!(root.read_at(6, 3), Some(b"DEF".as_slice()));
                Ok(())
            },
        )
        .expect("stage fragment before final retention");
        assert_eq!(ctx.retained_bytes_available(), 0);
        assert!(ctx.resource_refusal().is_none());
    }
    let failure = match ctx.take_record_captures() {
        Err(CodecError::ResourceLimit(failure)) => failure,
        other => panic!("expected actual retained-range refusal, got {other:?}"),
    };
    assert_eq!(failure.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(failure.scope, crate::decode::LimitScope::Session);
    assert_eq!(
        failure.reason,
        crate::decode::ResourceFailure::BudgetExceeded
    );
    assert_eq!(
        failure.limit,
        u64::try_from(retained).expect("retained refusal limit fits u64")
    );
    assert_eq!(
        failure.used,
        u64::try_from(retained).expect("retained refusal used bytes fit u64")
    );
    assert_eq!(
        failure.additional,
        u64::try_from(size_of::<std::ops::Range<u64>>())
            .expect("retained refusal range increment fits u64")
    );
    assert_eq!(ctx.resource_refusal(), Some(failure));
    assert!(
        matches!(ctx.take_record_captures(), Err(CodecError::ResourceLimit(repeated)) if repeated == failure)
    );
    let mut ran = false;
    assert!(ctx
        .record_fragment(&owner, root, || {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    assert_eq!(ctx.resource_refusal(), Some(failure));
    assert_eq!(ctx.retained_bytes_available(), 0);
}
