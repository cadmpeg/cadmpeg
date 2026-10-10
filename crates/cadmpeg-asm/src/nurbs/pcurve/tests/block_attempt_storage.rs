// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn block() -> Vec<Token> {
    vec![Token::Ident("nubs".into()), Token::Long(1), Token::Enum(0), Token::Long(2),
        Token::Double(0.0), Token::Long(1), Token::Double(1.0), Token::Long(1),
        Token::Double(0.0), Token::Double(0.0), Token::Double(1.0), Token::Double(0.0)]
}

#[test]
fn rejected_pcurve_attempt_releases_knots_and_poles() {
    let mut tokens = block();
    tokens.pop();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..4096 {
        assert!(super::super::pcurve_block_with_end(&ctx, &tokens, 0).is_none());
    }
    ctx.finish_session().unwrap();
}

#[test]
fn accepted_pcurve_attempt_transfers_storage_to_retained() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes, "ASM pcurve block attempt", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            super::super::pcurve_block_with_end(&ctx, &block(), 0).unwrap()
        });
    let CodecError::ResourceLimit(limit) = error else { panic!("attempt retention refusal"); };
    assert_eq!(limit.operation, "ASM pcurve block attempt");
    assert_eq!(limit.additional, u64::try_from(4 * std::mem::size_of::<f64>()
        + 2 * std::mem::size_of::<cadmpeg_ir::units::FinitePoint2>()).unwrap());
}
