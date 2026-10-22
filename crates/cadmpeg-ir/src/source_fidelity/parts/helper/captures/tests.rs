// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use crate::native::bytes::NativeBytes;
use crate::SourceFidelity;

#[test]
fn same_local_ids_keep_actual_batches_and_distinct_target_streams() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"0123456789ABCDEF", &arena, &DecodePolicy::default())
            .expect("root source");
    let first = ctx
        .register_named_slice(root, ByteRange { start: 0, end: 8 }, "first")
        .expect("first actual source member");
    let second = ctx
        .register_named_slice(root, ByteRange { start: 8, end: 16 }, "second")
        .expect("second actual source member");
    let (_, first_batch) = ctx
        .record_batch(|| {
            ctx.record("local", first, || {
                assert_eq!(first.read_at(1, 2), Some(b"12".as_slice()));
                Ok(())
            })
        })
        .expect("first real batch");
    let (_, second_batch) = ctx
        .record_batch(|| {
            ctx.record("local", second, || {
                assert_eq!(second.read_at(2, 2), Some(b"AB".as_slice()));
                Ok(())
            })
        })
        .expect("second real batch");
    assert_ne!(first_batch, second_batch);
    let mut visited = Vec::new();
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_mapped_record_captures(&ctx, |mut capture| {
            assert_eq!(capture.id(), "local");
            let batch = capture.batch_id().expect("actual member batch survives");
            visited.push(batch);
            let (target, prefix, pointer) = if batch == first_batch {
                ("target:first", "outer/inner/", 3)
            } else {
                assert_eq!(batch, second_batch);
                ("target:second", "other/", 5)
            };
            capture
                .fields_mut()
                .insert_pointer("offset".into(), pointer)?;
            capture
                .fields_mut()
                .insert_payload("payload".into(), NativeBytes::from(vec![0xAB]))?;
            capture.with_mapping(&ctx, Some(target.into()), prefix)
        })
        .expect("authentic mapped capture handoff");
    assert_eq!(visited, [first_batch, second_batch]);
    assert_eq!(fidelity.record_layout().len(), 2);
    for (id, stream, root_stream, offset, reads, pointer) in [
        (
            "target:first",
            "outer/inner/first",
            "outer/inner/file",
            0,
            1..3,
            3,
        ),
        ("target:second", "other/second", "other/file", 0, 2..4, 5),
    ] {
        let layout = &fidelity.record_layout()[id];
        assert_eq!(layout.stream(), stream);
        assert_eq!(layout.offset(), offset);
        assert_eq!(layout.byte_len(), 8);
        let root_reads = if id == "target:first" { 1..3 } else { 10..12 };
        assert_eq!(
            layout.reads(),
            &BTreeMap::from([
                (stream.to_owned(), vec![reads]),
                (root_stream.to_owned(), vec![root_reads]),
            ])
        );
        assert_eq!(layout.pointers()["offset"], pointer);
        assert_eq!(layout.payloads()["payload"].as_ref(), &[0xAB]);
    }
    assert!(
        ctx.take_record_captures().is_err(),
        "captures drain only once"
    );
}

#[test]
fn enclosing_transaction_must_commit_before_a_mapper_sees_any_capture() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcd", &arena, &DecodePolicy::default()).expect("root");
    let transaction = ctx.read_transaction().expect("real enclosing transaction");
    ctx.record("record", root, || {
        assert_eq!(root.read_at(0, 1), Some(b"a".as_slice()));
        Ok(())
    })
    .expect("completed inner record");
    let mut called = false;
    let mut fidelity = SourceFidelity::default();
    assert!(fidelity
        .install_mapped_record_captures(&ctx, |capture| {
            called = true;
            Ok(capture)
        })
        .is_err());
    assert!(!called);
    assert!(fidelity.record_layout().is_empty());
    transaction.commit().expect("actual ancestor commit");
    fidelity
        .install_mapped_record_captures(&ctx, |capture| {
            assert_eq!(capture.id(), "record");
            assert!(capture.batch_id().is_none());
            Ok(capture)
        })
        .expect("same capture survives the refused early handoff");
    assert_eq!(fidelity.record_layout()["record"].reads()["file"], [0..1]);
}

#[test]
fn real_stream_prefix_allocation_obeys_the_remaining_caller_budget() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcd", &arena, &DecodePolicy::default()).expect("root");
    ctx.record("record", root, || {
        assert_eq!(root.read_at(0, 1), Some(b"a".as_slice()));
        Ok(())
    })
    .expect("real source capture");
    let mut fidelity = SourceFidelity::default();
    let error = fidelity
        .install_mapped_record_captures(&ctx, |capture| {
            ctx.retain_bytes(ctx.retained_bytes_available(), "held caller records")?;
            capture.with_mapping(&ctx, None, "member/")
        })
        .expect_err("a real new stream name has no remaining allowance");
    assert!(matches!(error, CodecError::ResourceLimit(_)));
    assert!(fidelity.record_layout().is_empty());
}

#[test]
fn duplicate_actual_target_mapping_never_replaces_an_installed_capture() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcd", &arena, &DecodePolicy::default()).expect("root");
    for (id, index) in [("first", 0), ("second", 1)] {
        ctx.record(id, root, || {
            assert!(root.read_at(index, 1).is_some());
            Ok(())
        })
        .expect("distinct real captured owner");
    }
    let mut fidelity = SourceFidelity::default();
    assert!(fidelity
        .install_mapped_record_captures(&ctx, |capture| {
            capture.with_mapping(&ctx, Some("same".into()), "")
        })
        .is_err());
    assert!(fidelity.record_layout().is_empty());
}

#[test]
fn a_stream_prefix_charges_only_its_added_bytes() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcd", &arena, &DecodePolicy::default()).expect("root");
    ctx.record("record", root, || {
        assert_eq!(root.read_at(0, 1), Some(b"a".as_slice()));
        Ok(())
    })
    .expect("real source capture");
    let prefix = "member/";
    let mut charged = None;
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_mapped_record_captures(&ctx, |capture| {
            let before = ctx.retained_bytes_available();
            let mapped = capture.with_mapping(&ctx, None, prefix)?;
            charged = Some(before - ctx.retained_bytes_available());
            Ok(mapped)
        })
        .expect("prefixed capture");
    // The containing stream and one read stream each gain the prefix.
    assert_eq!(charged, Some(2 * prefix.len() as u64));
    assert_eq!(
        fidelity.record_layout()["record"].reads()["member/file"],
        [0..1]
    );
}

#[test]
fn install_record_captures_refuses_a_batched_capture_and_installs_nothing() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcd", &arena, &DecodePolicy::default()).expect("root");
    ctx.record("plain", root, || Ok(()))
        .expect("unbatched capture");
    ctx.record_batch(|| ctx.record("batched", root, || Ok(())))
        .expect("batched capture");
    let mut fidelity = SourceFidelity::default();
    assert!(fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .is_err());
    assert!(fidelity.record_layout().is_empty());
}
