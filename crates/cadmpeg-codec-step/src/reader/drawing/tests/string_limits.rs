// SPDX-License-Identifier: Apache-2.0
//! Resource limits for STEP drawing value text.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::Value;
use crate::reader::RecordExt;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";

fn exchange(records: &str) -> (String, crate::parse::Exchange) {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid drawing exchange");
    (source, exchange)
}

fn value_refusal(value: &Value, operation: &'static str) {
    let (source, exchange) = exchange("#1=ITEM();");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                .expect("root fits retained policy");
            let reports =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
            super::super::value_text(
                &exchange,
                value,
                (&mut Vec::new(), &reports),
                1,
                "value",
                &ctx,
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::RetainedBytes && refusal.operation == operation)
    );
}

#[test]
fn drawing_string_value_refuses_retained_limit() {
    value_refusal(&Value::String(b"drawing text".to_vec()), "step_string_text");
}

#[test]
fn drawing_constant_value_refuses_retained_limit() {
    value_refusal(
        &Value::ConstantEntity("long-name".into()),
        "step_drawing_value_text",
    );
}

#[test]
fn drawing_list_text_refuses_retained_limit() {
    // Retained admission is the five output bytes: two digits, comma and parentheses.
    value_refusal(
        &Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        "step_drawing_value_text",
    );
}

#[test]
fn drawing_binary_text_refuses_retained_limit() {
    let (_source, exchange) = exchange("#1=ITEM(\"0FF\");");
    let value = exchange
        .records()
        .get(&1)
        .and_then(|record| record.parameter(0))
        .expect("binary parameter");
    let Value::Binary(_) = value else {
        panic!("expected binary parameter");
    };
    // Retained admission is the binary prefix plus two hex digits per byte.
    value_refusal(value, "step_drawing_value_text");
}

#[test]
fn drawing_decode_propagates_string_refusal() {
    let (source, exchange) = exchange("#1=DRAWING_DEFINITION('Main','detail');");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_string_text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                .expect("root fits retained policy");
            (super::super::decode(
                &exchange,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &HashSet::new(),
                &BTreeMap::new(),
                &ctx,
            ))
            .map(|_| ())
        },
    );
    assert!(
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text")
    );
}

#[test]
fn drawing_sheet_usage_sequence_propagates_string_refusal() {
    let (source, exchange) = exchange("#1=DRAWING_DEFINITION('','');#2=DRAWING_REVISION('',#1,'');#3=REPRESENTATION_CONTEXT('','');#4=PRESENTATION_VIEW('',(),#3);#5=DRAWING_SHEET_REVISION('',(),#3,#2);#6=DRAWING_SHEET_REVISION_USAGE(#5,#2,'sequence');");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "step_string_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                    .expect("root fits retained policy");

                (super::super::decode(
                    &exchange,
                    &mut cadmpeg_ir::document::CadIr::empty(),
                    &HashSet::new(),
                    &BTreeMap::new(),
                    &ctx,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "step_string_text")
    };
    assert!(
        refused,
        "no retained limit refused the drawing sequence string"
    );
}

#[test]
fn nested_drawing_text_retains_only_the_complete_result() {
    let (_source, exchange) = exchange("#1=ITEM();");
    let mut value = Value::String(b"AB".to_vec());
    for _ in 0..48 {
        value = Value::Typed("WRAP".into(), Box::new(value));
    }
    let expected = format!("{}AB{}", "WRAP(".repeat(48), ")".repeat(48));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Forty-eight typed wrappers contribute six bytes each; the leaf contributes two.
    policy.limits.max_retained_bytes = u64::try_from(expected.len()).expect("fixture length");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
    let text = super::super::value_text(
        &exchange,
        &value,
        (&mut Vec::new(), &reports),
        1,
        "fixture",
        &ctx,
    )
    .expect("only final text retained");
    assert_eq!(text.as_deref(), Some(expected.as_str()));
}

#[test]
fn invalid_drawing_text_does_not_admit_an_unvisited_suffix() {
    let (_, exchange) = exchange("#1=ITEM();");
    let mut values = vec![Value::String(b"\\X2\\D83D\\X0\\".to_vec())];
    values.extend(std::iter::repeat_n(Value::Omitted, 100_000));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 10_000;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut losses = Vec::new();
        let text = super::super::value_text(
            &exchange,
            &Value::List(values),
            (&mut losses, &reports),
            1,
            "fixture",
            ctx,
        )
        .expect("invalid prefix fits without visiting suffix");
        assert!(text.is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::StepLossCode::MetadataStringInvalid.kind()
        );
    });
}
