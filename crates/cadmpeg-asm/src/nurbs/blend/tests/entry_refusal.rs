// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::CodecError;

use crate::nurbs::toks::Cur;

#[test]
fn rolling_ball_curve_propagates_cache_refusal_before_missing_bounds() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let tokens = [Token::Ident("intcurve".into()), Token::False,
        Token::SubtypeOpen, Token::Ident("exact_int_cur".into()),
        Token::Ident("nubs".into()), Token::SubtypeClose];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    // The balanced scope scan uses no collection. Its owned nubs marker needs
    // exactly one index entry before any missing degree or bounds can recover.
    let mut cur = Cur::at(&tokens, 0);
    let Some(Err(CodecError::ResourceLimit(first))) = super::super::rolling_ball_curve(&ctx, &mut cur, None) else {
        panic!("cache marker refusal must precede missing bound recovery");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "ASM owned spline markers");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for _ in 0..64 {
        let mut cur = Cur::at(&tokens, 0);
        assert!(matches!(super::super::rolling_ball_curve(&ctx, &mut cur, None),
            Some(Err(CodecError::ResourceLimit(last))) if last == first));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}
