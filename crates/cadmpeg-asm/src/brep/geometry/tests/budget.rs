// SPDX-License-Identifier: Apache-2.0

use super::super::{collect_carrier, pcurve_tail_metadata, tolerant_coedge_extension};
use crate::sab::{Record, Token};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn record(tokens: Vec<Token>) -> Record {
    crate::test_support::sab::record(
1,
"tcoedge".into(),
tokens.into(),
0,
0
)
}

#[test]
fn analytic_carrier_storage_is_scoped_and_released() {
    let record = record((0..12).map(|_| Token::Position([0.0; 3])).collect());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 2048;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..100 {
        let carrier = collect_carrier(&ctx, &record).unwrap();
        assert_eq!(carrier.positions.len(), 12);
    }
}

fn embedded_coedge() -> Record {
    let mut tokens = (0..13).map(|_| Token::Ref(-1)).collect::<Vec<_>>();
    tokens.extend([
        Token::Ref(2),
        Token::Long(1),
        Token::True,
        Token::SubtypeOpen,
        Token::Ident("curve".into()),
        Token::Long(4),
        Token::SubtypeOpen,
        Token::Long(5),
        Token::SubtypeClose,
        Token::SubtypeClose,
        Token::Ident("null_curve".into()),
        Token::False,
        Token::False,
        Token::Long(0),
        Token::Ident("null_curve".into()),
    ]);
    record(tokens)
}

#[test]
fn tolerant_coedge_payload_walk_refuses_before_visiting() {
    let record = embedded_coedge();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "ASM tolerant coedge payload",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            tolerant_coedge_extension(&ctx, &record)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal")
    };
    assert_eq!(limit.operation, "ASM tolerant coedge payload");
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(matches!(
        tolerant_coedge_extension(&ctx, &record).unwrap(),
        Some(
            crate::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: Some(2),
                curve_reversed: true,
                payload_token_count: 4,
                parameter_range: None,
            }
        )
    ));
}

#[test]
fn pcurve_tail_search_ignores_payload_identifiers() {
    let record = record(vec![
        Token::Ident("ignored".into()),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Long(0),
        Token::True,
        Token::False,
        Token::True,
        Token::False,
        Token::Ident("null_curve".into()),
        Token::Double(2.0),
        Token::Double(3.0),
        Token::Ident("null_curve".into()),
    ]);
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        pcurve_tail_metadata(&ctx, &record)
            .map(|tail| (tail.flags, tail.parameter_range))
            .unwrap(),
        (Some([true, false, true, false]), Some([2.0, 3.0]))
    );
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "ASM pcurve parameter tail",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            pcurve_tail_metadata(&ctx, &record)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal")
    };
    assert_eq!(limit.operation, "ASM pcurve parameter tail");
}

#[test]
fn record_sense_search_admits_raw_tokens_and_keeps_scope_precedence() {
    use super::super::record_reversed;
    let tokens = vec![
        Token::Ident("ignored".into()),
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        Token::True,
        Token::False,
        Token::Ident("ignored".into()),
        Token::SubtypeOpen,
        Token::SubtypeClose,
    ];
    let record = { let base = record(tokens); crate::test_support::sab::record(
base.index,
"intcurve".into(),
base.tokens,
base.offset,
base.len
) };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "ASM record sense tokens",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            record_reversed(&ctx, &record)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal")
    };
    assert_eq!(limit.operation, "ASM record sense tokens");
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(!record_reversed(&ctx, &record).unwrap());
    let plain = { let base = record; crate::test_support::sab::record(
base.index,
"intcurve".into(),
vec![
            Token::Ident("ignored".into()),
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::True,
        ]
        .into(),
base.offset,
base.len
) };
    assert!(record_reversed(&ctx, &plain).unwrap());
    let spline = { let base = plain; crate::test_support::sab::record(
base.index,
"spline".into(),
base.tokens,
base.offset,
base.len
) };
    assert!(!record_reversed(&ctx, &spline).unwrap());
}
