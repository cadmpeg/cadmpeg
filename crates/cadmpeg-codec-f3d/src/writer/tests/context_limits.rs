// SPDX-License-Identifier: Apache-2.0
//! Every writer child uses the caller's operation account and source length.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn native_writer_load_keeps_the_original_callers_retained_refusal() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let record = crate::records::recipes::CreationTimestamp {
        id: "f3d:generated:creation-timestamp#0".into(),
        target: cadmpeg_ir::attributes::AttributeTarget::Body(ir.model.bodies[0].id.clone()),
        record_index: 0,
        unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
    };
    crate::test_support::with_decode_context(|ctx| {
        ir.native
            .namespace_mut("f3d")
            .set_arena(ctx, "creation_timestamps", &[record])
            .unwrap();
    });
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    ctx.retain_bytes(
        ctx.retained_bytes_available(),
        "exhaust caller retained allowance",
    )
    .unwrap();
    let error = crate::writer::primitives::f3d_native(&ctx, &ir).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("caller retained refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(ctx.finish_session().is_err());
}

#[test]
fn patch_source_uses_the_original_callers_physical_input_ceiling() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut output = Vec::new();
    let error = crate::writer::patch::write_semantic(
        &ctx,
        &cadmpeg_ir::CadIr::empty(),
        b"larger source",
        &mut output,
        &mut Vec::new(),
    )
    .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("actual source input refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::InputBytes);
    assert_eq!(limit.limit, 1);
    assert!(output.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn nested_protein_source_shares_the_callers_input_ceiling() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::materials::patch_protein_appearances(
        &ctx,
        b"larger nested source",
        &std::collections::BTreeMap::new(),
        &mut Vec::new(),
    )
    .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("actual nested input refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::InputBytes);
    assert_eq!(limit.limit, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn zstd_write_expansion_uses_the_callers_single_expansion_ceiling() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let plain = b"12345678";
    let archive = zip_fixture::archive(&[RawEntry::compressed(
        "member",
        zip_fixture::ZSTD,
        &zip_fixture::zstd_stored_frame(plain),
        plain,
    )]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let mut source = zip::ZipArchive::new(std::io::Cursor::new(&archive)).unwrap();
    let mut entry = source.by_index_raw(0).unwrap();
    let error = crate::zip_write::expand_zstd_member(&ctx, &mut entry, "member").unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("caller expansion refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::DecompressedBytes);
    assert_eq!(limit.limit, 7);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
