// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::parameter::{macro_parameter_data_with_context, MacroDataError, ParameterDefect};

fn with_work_limit<T>(
    source: &[u8],
    max_work_units: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = max_work_units;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
    run(&ctx)
}

fn assert_work_limit(error: cadmpeg_core::CodecError, operation: &str, additional: u64) {
    use cadmpeg_core::decode::ResourceDimension;

    assert!(matches!(error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.additional == additional
                && limit.operation == operation
    ));
}

fn assert_macro_work_refusal(error: MacroDataError, operation: &str, additional: u64) {
    use cadmpeg_core::decode::ResourceDimension;

    assert!(matches!(error,
        MacroDataError::Refusal(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.additional == additional
                && limit.operation == operation
    ));
}

#[test]
fn macro_span_leading_whitespace_refuses_work_before_probe() {
    let error = with_work_limit(b" X", 0, |ctx| {
        crate::parameter::trim_macro_span(b" X", 0..2, ctx).unwrap_err()
    });
    assert_work_limit(error, "iges macro leading whitespace", 1);
}

#[test]
fn macro_span_trailing_whitespace_refuses_work_before_probe() {
    let error = with_work_limit(b"X ", 1, |ctx| {
        crate::parameter::trim_macro_span(b"X ", 0..2, ctx).unwrap_err()
    });
    assert_work_limit(error, "iges macro trailing whitespace", 1);
}

#[test]
fn macro_hollerith_digit_scan_refuses_work_before_probe() {
    let error = with_work_limit(b"1Ha", 0, |ctx| {
        crate::parameter::macro_hollerith_end(b"1Ha", 0, ctx).unwrap_err()
    });
    assert_macro_work_refusal(error, "iges macro Hollerith digits", 1);
}

#[test]
fn macro_hollerith_count_refuses_utf8_work() {
    let error = with_work_limit(b"1Ha", 2, |ctx| {
        crate::parameter::macro_hollerith_end(b"1Ha", 0, ctx).unwrap_err()
    });
    assert_macro_work_refusal(error, "iges macro Hollerith count", 1);
}

#[test]
fn macro_field_leading_whitespace_refuses_work_before_probe() {
    let error = with_work_limit(b"X,", 0, |ctx| {
        crate::parameter::macro_next_field(b"X,", 0, b',', b';', ctx).unwrap_err()
    });
    assert_macro_work_refusal(error, "iges macro leading whitespace", 1);
}

#[test]
fn macro_header_scan_refuses_work_after_leading_probe() {
    let error = with_work_limit(b"X,", 1, |ctx| {
        crate::parameter::macro_next_field(b"X,", 0, b',', b';', ctx).unwrap_err()
    });
    assert_macro_work_refusal(error, "iges macro header field scan", 1);
}

#[test]
fn macro_integer_refuses_utf8_work() {
    let error = with_work_limit(b"621", 0, |ctx| {
        crate::parameter::macro_integer(b"621", &(0..3), ctx).unwrap_err()
    });
    assert_work_limit(error, "iges macro integer", 3);
}

#[test]
fn macro_statement_scan_refuses_work_before_statement_probe() {
    let bytes = b"306,MACRO,621,X;BODY;ENDM;";
    let error = with_work_limit(bytes, 0, |ctx| {
        macro_parameter_data_with_context(bytes, b',', b';', ctx).unwrap_err()
    });
    assert_macro_work_refusal(error, "iges macro statement scan", 1);
}

#[test]
fn macro_statement_spans_refuse_collection_limit_before_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let bytes = b"306,MACRO,621,X;BODY;ENDM;";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let result = crate::parameter::macro_parameter_data_with_context(bytes, b',', b';', &ctx);
    assert!(matches!(
        result,
        Err(crate::parameter::MacroDataError::Refusal(
            cadmpeg_core::CodecError::ResourceLimit(limit)
        )) if limit.dimension == ResourceDimension::CollectionItems
            && limit.used == 0
            && limit.additional == 1
            && limit.operation == "iges macro statement spans"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        crate::parameter::macro_parameter_data_with_context(bytes, b',', b';', &ctx)
            .ok()
            .map(|data| data.statement_spans.len()),
        Some(3)
    );
}

#[test]
fn macro_parameter_data_keeps_language_delimiters_outside_hollerith_payloads() {
    let bytes = b"306,MACRO,621,X,Y;LET $S=3Ha;b;ENDM;comment bytes";
    let data = crate::test_support::with_service_context(bytes, |ctx| {
        macro_parameter_data_with_context(bytes, b',', b';', ctx)
    })
    .unwrap();

    assert_eq!(data.defined_entity_type, 621);
    assert_eq!(data.statement_spans.len(), 3);
    assert_eq!(
        data.statement_spans
            .iter()
            .map(|span| &bytes[span.clone()])
            .collect::<Vec<_>>(),
        vec![
            b"306,MACRO,621,X,Y".as_slice(),
            b"LET $S=3Ha;b".as_slice(),
            b"ENDM".as_slice()
        ]
    );
    assert_eq!(
        data.record_end,
        b"306,MACRO,621,X,Y;LET $S=3Ha;b;ENDM;".len()
    );
}

#[test]
fn macro_parameter_data_requires_the_assigned_type_and_arguments() {
    for (bytes, defect) in [
        (
            b"306,MACRO,599,X;ENDM;".as_slice(),
            ParameterDefect::MacroEntityTypeOutOfRange,
        ),
        (
            b"306,MACRO,621;ENDM;".as_slice(),
            ParameterDefect::MacroArgumentListMissing,
        ),
        (
            b"306,NOT_MACRO,621,X;ENDM;".as_slice(),
            ParameterDefect::MacroHeaderMalformed,
        ),
        (
            b"306,MACRO,621,X;LET X=1;".as_slice(),
            ParameterDefect::MacroTerminatorMissing,
        ),
    ] {
        let error = crate::test_support::with_service_context(bytes, |ctx| {
            macro_parameter_data_with_context(bytes, b',', b';', ctx)
        })
        .unwrap_err();
        assert!(matches!(error, MacroDataError::Defect(found, _) if found == defect));
    }
}

#[test]
fn macro_parameter_data_requires_nonempty_language_statements() {
    let bytes = b"306,MACRO,621,X;;ENDM;";
    let error = crate::test_support::with_service_context(bytes, |ctx| {
        macro_parameter_data_with_context(bytes, b',', b';', ctx)
    })
    .unwrap_err();
    assert!(matches!(
        error,
        MacroDataError::Defect(ParameterDefect::MacroStatementEmpty, _)
    ));
}
