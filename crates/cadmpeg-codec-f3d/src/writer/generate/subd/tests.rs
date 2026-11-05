// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

use crate::F3dCodec;

#[test]
fn cage_record_limit_counts_only_written_records() {
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_form(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let surface = &decoded.ir().model.subds[0];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_entities = 25;
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    assert!(super::encode(&ctx, surface, true).unwrap().is_some());

    let arena = cadmpeg_core::decode::DecodeArena::new();
    policy.limits.max_entities = 24;
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let error = super::encode(&ctx, surface, true).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
            && (limit.limit, limit.used, limit.additional) == (24, 24, 1))
    );

    let arena = cadmpeg_core::decode::DecodeArena::new();
    policy.limits.max_entities = 0;
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    assert!(super::encode(&ctx, surface, false).unwrap().is_some());
}
