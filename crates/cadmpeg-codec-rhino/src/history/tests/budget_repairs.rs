// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn subd_orientation_rejection_pays_only_for_visited_prefix() {
    for invalid_index in [0, 5] {
        let count = 32_i32;
        let mut chain = [0_u8; 16].to_vec();
        chain.extend(count.to_le_bytes());
        chain.extend(count.to_le_bytes());
        for edge in 0..count {
            chain.extend(u32::try_from(edge).unwrap().to_le_bytes());
        }
        chain.extend(count.to_le_bytes());
        let orientation_offset = chain.len();
        let mut orientations = vec![0_u8; usize::try_from(count).unwrap()];
        orientations[invalid_index] = 2;
        chain.extend(orientations);
        let bytes = anonymous_value(1, &chain);
        let error = cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino subd edge chain traversal", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
            super::super::subd_edge_chain(&ctx, &bytes, 0, bytes.len(), ArchiveVersion::V8, &mut Diagnostics::new())
                .map_err(|error| match error {
                    crate::chunks::FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
                    other => panic!("search must refuse before structural rejection: {other:?}"),
                })
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("work refusal"); };
        assert_eq!(limit.additional, 1);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Parsing plus visits through the first invalid orientation; no suffix visits.
        policy.limits.max_work_units = limit.used + u64::try_from(invalid_index + 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false).unwrap();
        let expected_offset = chunk.body().start + 8 + orientation_offset + invalid_index;
        assert!(matches!(super::super::subd_edge_chain(&ctx, &bytes, 0, bytes.len(), ArchiveVersion::V8, &mut Diagnostics::new()),
            Err(crate::chunks::FramingError::Structural { offset, message })
                if offset == expected_offset && message == "invalid history SubD edge orientation"));
        assert!(ctx.resource_refusal().is_none());
    }
}
