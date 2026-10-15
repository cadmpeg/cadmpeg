// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn compressed_global_stream_accounts_for_both_live_streams_and_releases_scratch() {
    let mut card = [b' '; 80];
    card[..7].copy_from_slice(b"1H,,1H;");
    // Each call holds a 72-byte stream and a 72-byte digit buffer. The digit
    // buffer ends at return, while the returned stream remains live.
    let simultaneous_peak = 3 * 72;
    for cap in [simultaneous_peak - 1, simultaneous_peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let first = logical_global_stream(&[&card], &ctx).unwrap();
        assert_eq!(first.0, b"1H,,1H;");
        let second = logical_global_stream(&[&card], &ctx);
        if cap < simultaneous_peak {
            let refusal = match second.as_ref() {
                Err(CodecError::ResourceLimit(refusal)) => *refusal,
                _ => panic!("expected second stream digit-buffer refusal"),
            };
            drop(second);
            assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(refusal.operation, "iges_compressed_global_digits");
            assert_eq!((refusal.limit, refusal.used, refusal.additional), (cap, 144, 72));
            assert_eq!(first.0, b"1H,,1H;");
            assert_eq!(ctx.resource_refusal(), Some(refusal));
            drop(first);
            assert!(matches!(
                logical_global_stream(&[], &ctx),
                Err(CodecError::ResourceLimit(original)) if original == refusal
            ));
            assert!(matches!(
                ctx.finish_session(),
                Err(CodecError::ResourceLimit(original)) if original == refusal
            ));
        } else {
            let second = second.unwrap();
            assert_eq!(second.0, b"1H,,1H;");
            drop(second);
            drop(first);
            let storage = ctx
                .reserve_scoped(simultaneous_peak, "all compressed Global scratch released")
                .unwrap();
            drop(storage);
            ctx.finish_session().unwrap();
        }
    }
}
