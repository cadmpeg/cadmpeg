// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use crate::test_support::test_bytes::zlib_compress;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::test_support::test_streams::primary_point_stream;

#[test]
fn actual_scan_keeps_expanded_views_and_exact_nonzero_physical_parent_after_source_flip() {
    let mut values = Vec::new();
    for payload in [b"source-A", b"source-B"] {
        let compressed = zlib_compress(payload);
        let mut part = b"before-member".to_vec();
        part.extend(&compressed);
        part.extend(b"after-member");
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
        let prefix = b"outside NX root";
        let mut bytes = prefix.to_vec();
        bytes.extend(&file);
        bytes.extend(b"outside suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let scan = crate::decode::scan(
            &ctx,
            root.child(prefix.len(), prefix.len() + file.len()).unwrap(),
        )
        .unwrap();
        assert_eq!(scan.streams.len(), 1);
        let stream = &scan.streams[0];
        let source = stream.source.expect("actual producer View");
        assert_eq!(source.window(), payload);
        assert_eq!(stream.inflated.as_ref(), payload);
        assert_eq!(stream.consumed, u64::try_from(compressed.len()).unwrap());
        values.push(source.window()[7]);
    }
    assert_eq!(values, [b'A', b'B']);
}

#[test]
fn rejected_structural_candidate_consumes_its_member_without_a_live_expanded_stream() {
    let ignored = zlib_compress(b"preview candidate");
    let expected = primary_point_stream();
    let accepted = zlib_compress(&expected);
    let mut bytes = ignored.clone();
    bytes.extend(&accepted);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut streams = Vec::new();
    super::super::append_all_zlib_streams(&ctx, root, 0, &mut streams, true).unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].file_offset, ignored.len());
    assert_eq!(streams[0].source.unwrap().window(), expected);
}

#[test]
fn clear_streams_keep_the_original_nonzero_view_and_real_expansion_limits_escape() {
    let clear = primary_point_stream();
    let mut bytes = b"outside".to_vec();
    let start = bytes.len();
    bytes.extend(&clear);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(start, bytes.len()).unwrap();
    let streams = super::super::extract_legacy_streams(&ctx, source).unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].source.unwrap().location(), source.location());
    assert_eq!(streams[0].source.unwrap().window(), clear);

    let compressed = zlib_compress(b"expansion");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&compressed, &arena, &policy).unwrap();
    assert!(matches!(
        super::inflate(&ctx, root, |_, _| Ok(false)),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(ctx.resource_refusal().is_some());
}

#[test]
fn a_real_per_member_limit_rolls_back_before_a_smaller_independent_sibling() {
    let large = zlib_compress(b"too large");
    let small = zlib_compress(b"ok");
    let mut bytes = large.clone();
    bytes.extend(&small);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 4;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let first = root.child(0, large.len()).unwrap();
    let outcome = ctx
        .run_part(|| {
            super::inflate(&ctx, first, |_, _| {
                panic!("limit precedes source admission")
            })
        })
        .unwrap();
    assert!(matches!(outcome, Err(CodecError::ResourceLimit(limit))
        if limit.scope == cadmpeg_core::decode::LimitScope::Part));
    assert_eq!(ctx.resource_refusal(), None);
    let Some(super::Candidate::Accepted { source, consumed }) = super::inflate(
        &ctx,
        root.child(large.len(), bytes.len()).unwrap(),
        |_, _| Ok(true),
    )
    .unwrap() else {
        panic!("the independently bounded small member remains available");
    };
    assert_eq!(source.window(), b"ok");
    assert_eq!(consumed, u64::try_from(small.len()).unwrap());
    assert_eq!(source.window().get(0..2), Some(b"ok".as_slice()));
}

#[test]
fn actual_clear_and_expanded_streams_borrow_the_registered_source_bytes() {
    for compressed in [false, true] {
        let mut payload = primary_point_stream();
        payload.resize(8192, b'.');
        let bytes = if compressed {
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", zlib_compress(&payload))])
        } else {
            payload.clone()
        };
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let streams = if compressed {
            crate::decode::scan(&ctx, root).unwrap().streams
        } else {
            super::super::extract_legacy_streams(&ctx, root).unwrap()
        };
        assert_eq!(streams.len(), 1);
        let source = streams[0].source.expect("actual source View");
        let std::borrow::Cow::Borrowed(borrowed) = &streams[0].inflated else {
            panic!("actual producer does not clone the source payload");
        };
        assert_eq!(*borrowed, payload);
        assert!(std::ptr::eq(borrowed.as_ptr(), source.window().as_ptr()));
    }
}
