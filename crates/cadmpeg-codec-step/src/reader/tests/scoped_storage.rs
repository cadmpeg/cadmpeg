// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn scratch_text_releases_storage_without_retaining_valid_text() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM('ABCD');ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let value = exchange
        .records()
        .get(&1)
        .expect("record")
        .partials
        .first()
        .parameters
        .first()
        .expect("text");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    for _ in 0..2 {
        let mut storage = ctx.reserve_scoped(0, "text fixture").expect("scope");
        let mut losses = Vec::new();
        let text = super::super::decode_text_scoped(
            &exchange,
            value,
            (
                &mut losses,
                &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope")),
            ),
            1,
            ("fixture", crate::loss::StepLossCode::MetadataStringInvalid),
            &ctx,
            &mut storage,
        )
        .expect("scratch string");
        assert_eq!(text.as_deref(), Some("ABCD"));
        assert!(losses.is_empty());
    }
    assert!(ctx.reserve_scoped(4, "released scratch").is_ok());
}

#[test]
fn invalid_scratch_text_keeps_its_loss_retained() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM('\\X\\GG');ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let value = exchange
        .records()
        .get(&1)
        .expect("record")
        .partials
        .first()
        .parameters
        .first()
        .expect("text");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_invalid_string_loss_text",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
            let mut storage = ctx.reserve_scoped(0, "text fixture").expect("scope");
            let result = super::super::decode_text_scoped(
                &exchange,
                value,
                (
                    &mut Vec::new(),
                    &std::cell::RefCell::new(
                        ctx.reserve_scoped(0, "report fixture").expect("scope"),
                    ),
                ),
                1,
                ("fixture", crate::loss::StepLossCode::MetadataStringInvalid),
                &ctx,
                &mut storage,
            );
            if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                assert_eq!(ctx.resource_refusal(), Some(refusal.clone()));
            }
            result
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(refusal) if refusal.operation == "step_invalid_string_loss_text")
    );
}
