// SPDX-License-Identifier: Apache-2.0

use super::super::local_reference_candidates;
use cadmpeg_core::decode::ResourceDimension;

#[test]
fn local_reference_candidate_growth_refuses_collection_limit() {
    let mut bytes = vec![0; 12];
    bytes[0] = 1;
    bytes[1..9].copy_from_slice(&257_u64.to_le_bytes());

    crate::test_support::with_decode_context(|ctx| {
        let candidates = local_reference_candidates(ctx, &bytes, 0, true)
            .expect("four valid local reference candidates");
        assert_eq!(candidates.len(), 4);
        assert_eq!(candidates[0].target, 257);
        assert_eq!(candidates[0].end, 11);
        assert_eq!(candidates[0].inline_type_guid, None);
        assert_eq!(candidates[0].padding.trailing_zeros(), 2);
        assert_eq!(candidates[1].target, 257);
        assert_eq!(candidates[1].end, 12);
        assert_eq!(candidates[1].padding.trailing_zeros(), 2);
        assert_eq!(candidates[2].target, 1);
        assert_eq!(candidates[2].end, 10);
        assert_eq!(candidates[2].padding.trailing_zeros(), 0);
        assert_eq!(candidates[3].target, 1);
        assert_eq!(candidates[3].end, 11);
        assert_eq!(candidates[3].padding.trailing_zeros(), 0);
    });

    for skip in 0..4 {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "collect F3D local reference candidates",
            skip,
            |ctx| local_reference_candidates(ctx, &bytes, 0, true).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect F3D local reference candidates"
                    && limit.additional == 1
        ));
    }
}
