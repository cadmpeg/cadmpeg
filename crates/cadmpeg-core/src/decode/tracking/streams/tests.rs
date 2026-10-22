// SPDX-License-Identifier: Apache-2.0
//! Literal source mappings, semantic reads, and transaction isolation.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::decode::{
    ByteRange, DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, ResourceDimension,
};
use crate::CodecError;

fn reads(entries: &[(&str, &[Range<u64>])]) -> BTreeMap<String, Vec<Range<u64>>> {
    entries
        .iter()
        .map(|(name, ranges)| ((*name).to_owned(), ranges.to_vec()))
        .collect()
}

#[test]
fn stored_aliases_project_exact_ranges_through_nested_names() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789abcdef", &arena, &DecodePolicy::default())
            .expect("root");
    let stored = ctx
        .register_named_slice(root, ByteRange { start: 4, end: 12 }, "archive/stored")
        .expect("stored");
    let nested = ctx
        .register_named_slice(stored, ByteRange { start: 2, end: 6 }, "nested")
        .expect("nested");
    ctx.record("record", nested, || {
        assert_eq!(nested.read_at(1, 2), Some(&b"78"[..]));
        Ok(())
    })
    .expect("record");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures[0].stream(), "nested");
    assert_eq!((captures[0].offset(), captures[0].byte_len()), (0, 4));
    assert_eq!(
        captures[0].reads(),
        &reads(&[
            ("nested", &[1..3]),
            ("archive/stored", &[3..5]),
            ("file", &[7..9])
        ])
    );
}

#[test]
fn expansion_copy_and_checksum_alone_do_not_consume_source() {
    let arena = DecodeArena::new();
    // A literal raw DEFLATE stored block containing abc, between framing bytes.
    let source = [90, 1, 3, 0, 252, 255, b'a', b'b', b'c', 91];
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::default()).expect("root");
    let compressed = root.child(1, 9).expect("compressed block");
    let decoded = ctx
        .record("opening", root, || {
            let output = ctx
                .inflate_probe(compressed, 3, false)?
                .expect("valid raw DEFLATE");
            assert_eq!(output, b"abc");
            // ZIP's CRC-32, calculated over copied bytes, is structural evidence.
            let crc = output.iter().fold(!0u32, |crc, byte| {
                (0..8).fold(crc ^ u32::from(*byte), |crc, _| {
                    (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1))
                })
            }) ^ !0u32;
            assert_eq!(crc, 0x352441c2);
            let mut expansion = ctx.begin_expand(ExpandSpec::Exact(3))?;
            expansion.write(&output)?;
            let decoded = expansion.finalize_from(compressed, "decoded")?;
            assert_eq!(decoded.window().to_vec(), b"abc");
            Ok(decoded)
        })
        .expect("open");
    ctx.record("value", decoded, || {
        assert_eq!(decoded.read_at(1, 1), Some(&b"b"[..]));
        Ok(())
    })
    .expect("semantic value");
    let captures = ctx.take_record_captures().expect("captures");
    assert!(captures[0].reads().is_empty());
    assert_eq!(
        captures[1].reads(),
        &reads(&[("decoded", &[1..2]), ("file", &[1..9])])
    );
}

#[test]
fn failed_child_removes_ancestor_contributions_but_preserves_overlapping_sibling() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789abcdef", &arena, &DecodePolicy::default())
            .expect("root");
    let make = |source, name| {
        let mut expansion = ctx.begin_expand(ExpandSpec::Exact(3)).expect("expansion");
        expansion.write(b"abc").expect("write");
        expansion.finalize_from(source, name).expect("named output")
    };
    let good = make(root.child(2, 8).expect("good extent"), "good");
    let bad = make(root.child(5, 12).expect("bad extent"), "bad");
    ctx.record("outer", root, || {
        assert_eq!(good.read_at(0, 1), Some(&b"a"[..]));
        let failed = ctx.record("failed", bad, || {
            assert_eq!(bad.read_at(1, 1), Some(&b"b"[..]));
            ctx.record("committed-descendant", bad, || {
                assert_eq!(bad.read_at(2, 1), Some(&b"c"[..]));
                Ok(())
            })?;
            Err::<(), _>(CodecError::malformed("invalid source part"))
        });
        assert!(failed.is_err());
        Ok(())
    })
    .expect("outer");
    let captures = ctx.take_record_captures().expect("captures");
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads(),
        &reads(&[("good", &[0..1]), ("file", &[2..8])])
    );
}

#[test]
fn stable_names_reopen_same_origin_and_refuse_conflicting_or_foreign_origins() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdef", &arena, &DecodePolicy::default()).expect("root");
    let first = ctx
        .register_named_slice(root, ByteRange { start: 1, end: 4 }, "entry")
        .expect("entry");
    let again = ctx
        .register_named_slice(root, ByteRange { start: 1, end: 4 }, "entry")
        .expect("same entry");
    assert_eq!(first.space(), again.space());
    assert!(ctx
        .register_named_slice(root, ByteRange { start: 2, end: 5 }, "entry")
        .is_err());
    assert!(ctx
        .register_named_slice(root, ByteRange { start: 1, end: 4 }, "")
        .is_err());
    assert!(ctx.name_stream(root, "entry").is_err());
    let (foreign, foreign_root) =
        DecodeContext::from_root_bytes(b"abcdef", &arena, &DecodePolicy::default())
            .expect("foreign root");
    assert!(ctx
        .register_named_slice(foreign_root, ByteRange { start: 0, end: 1 }, "foreign")
        .is_err());
    let mut expansion = ctx.begin_expand(ExpandSpec::Exact(1)).expect("expansion");
    expansion.write(b"x").expect("write");
    assert!(expansion.finalize_from(foreign_root, "foreign").is_err());
    foreign.finish_session().expect("foreign session");
}

#[test]
fn stale_named_view_keeps_its_bytes_without_attributing_to_reused_session() {
    let arena = DecodeArena::new();
    let (old, root) =
        DecodeContext::from_root_bytes(b"OLD", &arena, &DecodePolicy::default()).expect("old");
    let stale = old
        .register_named_slice(root, ByteRange { start: 0, end: 3 }, "old/entry")
        .expect("old entry");
    drop(old);
    assert_eq!(stale.read_at(0, 3), Some(&b"OLD"[..]));
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"NEW", &arena, &DecodePolicy::default()).expect("new");
    assert!(ctx
        .register_named_slice(stale, ByteRange { start: 0, end: 3 }, "new")
        .is_err());
    ctx.record("new", root, || {
        assert_eq!(stale.read_at(0, 3), Some(&b"OLD"[..]));
        assert_eq!(root.read_at(1, 1), Some(&b"E"[..]));
        Ok(())
    })
    .expect("new record");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &reads(&[("file", &[1..2])])
    );
}

#[test]
fn source_lineage_depth_uses_and_fuses_the_callers_budget() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).expect("root");
    let child = ctx
        .register_named_slice(root, ByteRange { start: 0, end: 3 }, "child")
        .expect("one level");
    let error = ctx
        .register_named_slice(child, ByteRange { start: 0, end: 3 }, "grandchild")
        .expect_err("two levels refused");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("typed budget refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert!(matches!(
        ctx.record("after refusal", root, || Ok(())),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn composite_seam_reads_project_only_the_intersected_physical_sectors() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"AAAAbbbbCCCCdddd", &arena, &DecodePolicy::default())
            .expect("root");
    let parts = [
        root.child(4, 8).expect("sector one"),
        root.child(12, 16).expect("sector two"),
    ];
    let assembled = ctx
        .record("opening", root, || {
            ctx.concat_named_views(&parts, "sector-chain")
        })
        .expect("assembly");
    assert_eq!(assembled.window(), b"bbbbdddd");
    let same = ctx
        .concat_named_views(&parts, "sector-chain")
        .expect("same chain");
    assert_eq!(assembled.space(), same.space());
    assert!(ctx
        .concat_named_views(&[parts[1], parts[0]], "sector-chain")
        .is_err());
    let record = assembled.child(2, 6).expect("cross-sector record");
    ctx.record("value", record, || {
        assert_eq!(record.read_at(2, 4), Some(&b"bbdd"[..]));
        Ok(())
    })
    .expect("read");
    let captures = ctx.take_record_captures().expect("captures");
    assert!(captures[0].reads().is_empty());
    assert_eq!((captures[1].offset(), captures[1].byte_len()), (2, 4));
    assert_eq!(
        captures[1].reads(),
        &reads(&[("sector-chain", &[2..6]), ("file", &[6..8, 12..14])])
    );
}

#[test]
fn nested_compression_closes_every_named_ancestor_without_opening_reads() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"---OUTERzip---", &arena, &DecodePolicy::default())
            .expect("root");
    let mut outer = ctx
        .begin_expand(ExpandSpec::Exact(6))
        .expect("outer expansion");
    outer.write(b"xINNER").expect("write outer");
    let outer = outer
        .finalize_from(root.child(3, 11).expect("outer source"), "outer")
        .expect("outer");
    let stored = ctx
        .register_named_slice(outer, ByteRange { start: 1, end: 6 }, "outer/stored")
        .expect("stored");
    let mut inner = ctx
        .begin_expand(ExpandSpec::Exact(3))
        .expect("inner expansion");
    inner.write(b"XYZ").expect("write inner");
    let inner = inner
        .finalize_from(
            stored.child(1, 4).expect("inner source"),
            "outer/stored/inner",
        )
        .expect("inner");
    ctx.record("record", inner, || {
        assert_eq!(inner.read_at(1, 1), Some(&b"Y"[..]));
        Ok(())
    })
    .expect("read");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &reads(&[
            ("outer/stored/inner", &[1..2]),
            ("outer/stored", &[1..4]),
            ("outer", &[2..5]),
            ("file", &[3..11])
        ])
    );
}

#[test]
fn composite_alias_and_compression_project_distinct_parent_coordinates() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"01234567ZIP!cdef", &arena, &DecodePolicy::default())
            .expect("root");
    let stored = ctx
        .register_named_slice(root, ByteRange { start: 0, end: 4 }, "stored")
        .expect("stored");
    let mut expanded = ctx.begin_expand(ExpandSpec::Exact(4)).expect("expansion");
    expanded.write(b"wxyz").expect("write");
    let expanded = expanded
        .finalize_from(root.child(8, 12).expect("compressed source"), "expanded")
        .expect("expanded");
    let combined = ctx
        .concat_named_views(
            &[
                stored.child(1, 3).expect("stored portion"),
                expanded.child(1, 4).expect("expanded portion"),
            ],
            "combined",
        )
        .expect("combined");
    assert_eq!(combined.window(), b"12xyz");
    ctx.record("outer", root, || {
        assert_eq!(root.read_at(2, 1), Some(&b"2"[..]));
        let failed = ctx.read_transaction()?;
        assert_eq!(combined.read_at(1, 4), Some(&b"2xyz"[..]));
        drop(failed);
        assert_eq!(combined.read_at(1, 3), Some(&b"2xy"[..]));
        Ok(())
    })
    .expect("outer");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &reads(&[
            ("combined", &[1..4]),
            ("stored", &[2..3]),
            ("expanded", &[1..3]),
            ("file", &[2..3, 8..12])
        ])
    );
}

#[test]
fn shared_composite_ancestors_are_projected_without_exponential_revisits() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"a", &arena, &DecodePolicy::default()).expect("root");
    let mut current = root;
    let mut expected = reads(&[("file", &[0..1])]);
    for level in 0..20 {
        let joined_name = format!("joined/{level}");
        let joined = ctx
            .concat_named_views(&[current, current], &joined_name)
            .expect("shared source");
        let mut expanded = ctx.begin_expand(ExpandSpec::Exact(1)).expect("expansion");
        expanded.write(b"a").expect("write");
        let expanded_name = format!("expanded/{level}");
        current = expanded
            .finalize_from(joined, &expanded_name)
            .expect("named expansion");
        expected.insert(joined_name, vec![0..2]);
        expected.insert(expanded_name, vec![0..1]);
    }
    ctx.record("record", current, || {
        assert_eq!(current.read_at(0, 1), Some(&b"a"[..]));
        Ok(())
    })
    .expect("read");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &expected
    );
}

#[test]
fn raw_derived_bytes_remain_unclaimed_and_foreign_composites_are_refused() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdef", &arena, &DecodePolicy::default()).expect("root");
    let (foreign, foreign_root) =
        DecodeContext::from_root_bytes(b"FOREIGN", &arena, &DecodePolicy::default())
            .expect("foreign");
    assert!(ctx
        .concat_named_views(&[root, foreign_root], "mixed sessions")
        .is_err());
    let raw = ctx.concat_views(&[root]).expect("raw copy");
    assert!(ctx.concat_named_views(&[raw], "detached").is_err());
    ctx.record("record", root, || {
        assert_eq!(raw.read_at(0, 3), Some(&b"abc"[..]));
        Ok(())
    })
    .expect("record");
    assert!(ctx.take_record_captures().expect("captures")[0]
        .reads()
        .is_empty());
    foreign.finish_session().expect("foreign session");
}

#[test]
fn expansion_size_refusal_cannot_register_a_complete_named_stream() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::default()).expect("root");
    let mut partial = ctx.begin_expand(ExpandSpec::Exact(3)).expect("expansion");
    partial.write(b"ab").expect("partial write");
    assert!(partial.finalize_from(root, "entry").is_err());
    let mut complete = ctx.begin_expand(ExpandSpec::Exact(3)).expect("expansion");
    complete.write(b"abc").expect("complete write");
    let complete = complete
        .finalize_from(root, "entry")
        .expect("no phantom registration");
    ctx.record("record", complete, || {
        assert_eq!(complete.read_at(2, 1), Some(&b"c"[..]));
        Ok(())
    })
    .expect("read");
    assert_eq!(
        ctx.take_record_captures().expect("captures")[0].reads(),
        &reads(&[("entry", &[2..3]), ("file", &[0..6])])
    );
}

#[test]
fn semantic_projection_obeys_active_depth_and_does_not_downgrade_a_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 2;
    let (ctx, root) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).expect("root");
    let child = ctx
        .register_named_slice(root, ByteRange { start: 0, end: 3 }, "child")
        .expect("child");
    let guard = ctx
        .enter_nested("owning parser recursion")
        .expect("active parser");
    let error = ctx
        .record("record", child, || {
            assert_eq!(child.read_at(0, 1), None);
            Ok(())
        })
        .expect_err("projection shares caller depth");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource fuse remains typed")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    drop(guard);
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn inventory_reports_canonical_names_domains_and_exact_transform_segments() {
    use crate::decode::SourceOrigin;

    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789abcdef", &arena, &DecodePolicy::default())
            .expect("root");
    let stored = ctx
        .register_named_slice(root, ByteRange { start: 4, end: 12 }, "stored")
        .expect("stored");
    let reopened = ctx
        .register_named_slice(root, ByteRange { start: 4, end: 12 }, "stored")
        .expect("same stored source");
    assert_eq!(stored.space(), reopened.space());
    let mut expanded = ctx.begin_expand(ExpandSpec::Exact(4)).expect("expansion");
    expanded.write(b"ABCD").expect("write");
    let expanded = expanded
        .finalize_from(stored.child(2, 7).expect("compressed source"), "decoded")
        .expect("expanded");
    let combined = ctx
        .concat_named_views(
            &[
                root.child(1, 3).expect("physical segment"),
                expanded.child(1, 4).expect("decoded segment"),
            ],
            "combined",
        )
        .expect("composite");
    assert_eq!(combined.window(), b"12BCD");
    let inventory = ctx.source_streams().expect("snapshot");
    assert_eq!(
        inventory
            .iter()
            .map(|stream| (stream.name(), stream.byte_len()))
            .collect::<Vec<_>>(),
        [("combined", 5), ("decoded", 4), ("file", 16), ("stored", 8)]
    );
    let SourceOrigin::Composite { segments } = inventory[0].origin() else {
        panic!("composite origin")
    };
    assert_eq!(segments.len(), 2);
    assert_eq!(
        (
            segments[0].child(),
            segments[0].parent(),
            segments[0].extent()
        ),
        (
            ByteRange { start: 0, end: 2 },
            "file",
            ByteRange { start: 1, end: 3 }
        )
    );
    assert_eq!(
        (
            segments[1].child(),
            segments[1].parent(),
            segments[1].extent()
        ),
        (
            ByteRange { start: 2, end: 5 },
            "decoded",
            ByteRange { start: 1, end: 4 }
        )
    );
    assert!(
        matches!(inventory[1].origin(), SourceOrigin::Expanded { parent, extent } if parent == "stored" && *extent == ByteRange { start: 2, end: 7 })
    );
    assert_eq!(inventory[2].origin(), &SourceOrigin::Root);
    assert!(
        matches!(inventory[3].origin(), SourceOrigin::Alias { parent, extent } if parent == "file" && *extent == ByteRange { start: 4, end: 12 })
    );
}

#[test]
fn inventory_and_raw_derived_spaces_add_no_reads_or_claimed_source_domains() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::default()).expect("root");
    ctx.record("record", root, || {
        let raw_alias = ctx.register_slice(root, ByteRange { start: 1, end: 4 })?;
        let raw_composite = ctx.concat_views(&[raw_alias, root])?;
        let mut raw_expansion = ctx.begin_expand(ExpandSpec::Exact(3))?;
        raw_expansion.write(b"abc")?;
        let raw_expansion = raw_expansion.finalize()?;
        assert_eq!(raw_composite.window(), b"oursource");
        assert_eq!(raw_expansion.read_at(0, 3), Some(&b"abc"[..]));
        let streams = ctx.source_streams()?;
        assert_eq!(streams.len(), 1);
        assert_eq!((streams[0].name(), streams[0].byte_len()), ("file", 6));
        Ok(())
    })
    .expect("record");
    assert!(ctx.take_record_captures().expect("captures")[0]
        .reads()
        .is_empty());
    assert_eq!(
        ctx.source_streams()
            .expect("available after capture extraction")
            .len(),
        1
    );
}

#[test]
fn inventory_snapshot_is_immutable_across_later_registration_and_root_rename() {
    use crate::decode::SourceOrigin;

    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdef", &arena, &DecodePolicy::default()).expect("root");
    ctx.register_named_slice(root, ByteRange { start: 1, end: 4 }, "entry")
        .expect("entry");
    let before = ctx.source_streams().expect("snapshot before rename");
    ctx.name_stream(root, "source")
        .expect("root rename before captures");
    ctx.register_named_slice(root, ByteRange { start: 4, end: 6 }, "next")
        .expect("later source");
    let after = ctx.source_streams().expect("later snapshot");
    assert_eq!(
        before
            .iter()
            .map(|stream| stream.name())
            .collect::<Vec<_>>(),
        ["entry", "file"]
    );
    assert!(matches!(before[0].origin(), SourceOrigin::Alias { parent, .. } if parent == "file"));
    assert_eq!(
        after.iter().map(|stream| stream.name()).collect::<Vec<_>>(),
        ["entry", "next", "source"]
    );
    assert!(matches!(after[0].origin(), SourceOrigin::Alias { parent, .. } if parent == "source"));
}

#[test]
fn inventory_is_session_local_even_when_two_contexts_share_an_arena() {
    let arena = DecodeArena::new();
    let (first, first_root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("first");
    first
        .register_named_slice(first_root, ByteRange { start: 0, end: 2 }, "entry")
        .expect("first entry");
    let (second, _) =
        DecodeContext::from_root_bytes(b"12345", &arena, &DecodePolicy::default()).expect("second");
    let first_inventory = first.source_streams().expect("first inventory");
    let second_inventory = second.source_streams().expect("second inventory");
    assert_eq!(
        first_inventory
            .iter()
            .map(|stream| (stream.name(), stream.byte_len()))
            .collect::<Vec<_>>(),
        [("entry", 2), ("file", 3)]
    );
    assert_eq!(
        second_inventory
            .iter()
            .map(|stream| (stream.name(), stream.byte_len()))
            .collect::<Vec<_>>(),
        [("file", 5)]
    );
}

#[test]
fn owned_inventory_storage_cannot_bypass_the_shared_retained_budget() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::default()).expect("root");
    ctx.register_named_slice(root, ByteRange { start: 0, end: 2 }, "entry")
        .expect("entry");
    ctx.retain_bytes(
        ctx.retained_bytes_available(),
        "actual retained output fills caller allowance",
    )
    .expect("remaining allowance");
    let error = ctx
        .source_streams()
        .expect_err("snapshot storage must be charged");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == crate::decode::LimitScope::Session && limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit.scope == crate::decode::LimitScope::Session)
    );
}
