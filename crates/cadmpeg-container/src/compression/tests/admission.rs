// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, ResourceDimension,
};
use cadmpeg_core::decode::{LimitScope, SourceOrigin};
use cadmpeg_core::CodecError;
use flate2::{write::ZlibEncoder, Compression};
use std::io::Write;

fn member() -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(b"source-A").unwrap();
    encoder.finish().unwrap()
}

#[test]
fn conditional_zlib_admission_keeps_exact_parent_and_existing_tracked_parity() {
    let compressed = member();
    let mut bytes = b"prefix".to_vec();
    let start = bytes.len();
    bytes.extend(&compressed);
    let end = bytes.len();
    bytes.extend(b"suffix");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(start, bytes.len()).unwrap();
    assert!(crate::compression::inflate_zlib_member_if(
        &ctx,
        source,
        ExpandSpec::Exact(8),
        |value, consumed| {
            assert_eq!(value, b"source-A");
            assert_eq!(consumed, compressed.len());
            Ok(false)
        }
    )
    .unwrap()
    .is_none());
    assert_eq!(ctx.source_streams().unwrap().len(), 1);
    let (accepted, consumed) =
        crate::compression::inflate_zlib_member_if(&ctx, source, ExpandSpec::Exact(8), |_, _| {
            Ok(true)
        })
        .unwrap()
        .unwrap();
    let (existing, old_consumed) =
        crate::compression::inflate_zlib_member_tracked(&ctx, source, ExpandSpec::Exact(8))
            .unwrap();
    assert_eq!(accepted.window(), existing.window());
    assert_eq!(consumed, old_consumed);
    let inventory = ctx.source_streams().unwrap();
    let expanded = inventory
        .iter()
        .filter_map(|stream| match stream.origin() {
            SourceOrigin::Expanded { parent, extent } => Some((parent, extent)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(expanded.len(), 1);
    assert_eq!(expanded[0].0, "file");
    assert_eq!(expanded[0].1.start, u64::try_from(start).unwrap());
    assert_eq!(expanded[0].1.end, u64::try_from(end).unwrap());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn rejection_cannot_hide_bad_checksum_exact_size_or_foreign_source() {
    let mut corrupt = member();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    let valid = member();
    for (bytes, size) in [
        (&corrupt, ExpandSpec::Unknown),
        (&valid, ExpandSpec::Exact(9)),
    ] {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
        let mut called = false;
        assert!(
            crate::compression::inflate_zlib_member_if(&ctx, root, size, |_, _| {
                called = true;
                Ok(false)
            })
            .is_err()
        );
        assert!(!called);
        assert_eq!(ctx.source_streams().unwrap().len(), 1);
    }
    let arena = DecodeArena::new();
    let (_owner, foreign) =
        DecodeContext::from_root_bytes(&valid, &arena, &DecodePolicy::service()).unwrap();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&valid, &arena, &DecodePolicy::service()).unwrap();
    let mut called = false;
    assert!(crate::compression::inflate_zlib_member_if(
        &ctx,
        foreign,
        ExpandSpec::Unknown,
        |_, _| {
            called = true;
            Ok(false)
        }
    )
    .is_err());
    assert!(!called);
    assert_eq!(ctx.source_streams().unwrap().len(), 1);
}

#[test]
fn rejected_expansion_keeps_session_charge_and_no_registered_child() {
    let bytes = member();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = 8;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(crate::compression::inflate_zlib_member_if(
        &ctx,
        root,
        ExpandSpec::Unknown,
        |_, _| Ok(false)
    )
    .unwrap()
    .is_none());
    assert_eq!(ctx.source_streams().unwrap().len(), 1);
    let error =
        crate::compression::inflate_zlib_member_if(&ctx, root, ExpandSpec::Unknown, |_, _| {
            panic!("a session refusal precedes admission")
        })
        .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.scope == LimitScope::Session && limit.dimension == ResourceDimension::DecompressedBytes));
    assert!(ctx.resource_refusal().is_some());
}
