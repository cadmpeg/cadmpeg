// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::units::FinitePoint2;

#[test]
fn pcurve_tolerance_releases_a_failed_cache_before_the_next_candidate() {
    let tokens = [
        Token::SubtypeOpen, Token::Ident("exp_par_cur".into()),
        Token::Ident("nubs".into()), Token::Long(1), Token::Enum(0), Token::Long(2),
        Token::Double(0.0), Token::Long(1), Token::Double(1.0), Token::Long(1),
        Token::Double(0.0), Token::Double(0.0), Token::Double(1.0), Token::Double(1.0),
        Token::Double(0.125),
        // Reverse candidate order visits this malformed block first. Its two
        // endpoint runs state 512 poles, but no pole coordinates follow.
        Token::Ident("nubs".into()), Token::Long(1), Token::Enum(0), Token::Long(2),
        Token::Double(0.0), Token::Long(256), Token::Double(1.0), Token::Long(256),
        Token::SubtypeClose,
    ];
    // Two marker pushes use core's four-slot amortized usize minimum.
    let marker_bytes = 4 * std::mem::size_of::<usize>();
    let knot_bytes = 514 * std::mem::size_of::<f64>();
    let pole_bytes = 512 * std::mem::size_of::<FinitePoint2>();
    let peak = u64_from_index(marker_bytes + knot_bytes + pole_bytes);
    for cap in [peak - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scope = crate::nurbs::toks::subtype_span(&ctx, &tokens, 0).unwrap().unwrap();
        let result = super::super::pcurve_fit_tolerance(&ctx, scope);
        if cap < peak {
            let Some(Err(CodecError::ResourceLimit(first))) = result else {
                panic!("expected malformed-candidate pole allocation refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "ASM polynomial pcurve poles");
            assert_eq!((first.limit, first.used, first.additional),
                (cap, u64_from_index(marker_bytes + knot_bytes), u64_from_index(pole_bytes)));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert!(matches!(result, Some(Ok(value)) if value == 0.125));
            let released = ctx.reserve_scoped(peak, "all pcurve tolerance attempts released").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        }
    }
}
