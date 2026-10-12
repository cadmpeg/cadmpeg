// SPDX-License-Identifier: Apache-2.0
//! Kernel unknown-record refusal propagation through decode.

use crate::container::InventorContainer;
use crate::decode::decode_container;
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream, primary_envelope_fixture_with_kernel, EnvelopeDeclarations,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn kernel_unknown_attachment_preserves_original_resource_refusal() {
    let mut kernel = acis_sphere_kernel_stream(21_800);
    let name = kernel
        .windows(b"sphere".len())
        .position(|window| window == b"sphere")
        .expect("generated sphere record");
    // Keep the same six-byte identifier width and valid SAB token framing.
    // The opaque surface referenced by the face must retain its original record.
    kernel[name..name + b"sphere".len()].copy_from_slice(b"opaque");
    let bytes = primary_envelope_fixture_with_kernel(EnvelopeDeclarations::default(), &kernel);
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("fixture context");
    let container = InventorContainer::open(&setup, root).expect("fixture container");
    let decoded = decode_container(&setup, &container).expect("opaque carrier recovery");
    let unknowns = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace")
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .expect("native unknowns");
    assert_eq!(
        unknowns.len(),
        1,
        "the fixture executes the attachment route"
    );

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("decode context");
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "native unknown product identity",
        None,
    );
    let error = decode_container(&ctx, &container).expect_err("attachment identity must refuse");
    drop(probe);
    let CodecError::ResourceLimit(original) = error else {
        panic!("original resource refusal");
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(original.operation, "native unknown product identity");
    assert!(original.additional > 0);
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}
