use super::child_tokens;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure,
};
use cadmpeg_core::CodecError;

#[test]
fn hole_child_tokens_preserve_unicode_and_empty_parts() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (text, expected) in [
        (" α,,β, ", vec![" α", "", "β", " "]),
        ("", vec![""]),
        ("α,", vec!["α", ""]),
    ] {
        let tokens = child_tokens(&ctx, text, "scan SLDPRT hole child references")
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(tokens, expected);
    }
}

#[test]
fn hole_child_tokens_refuse_on_first_text_visit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "scan SLDPRT hole child references",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut children = child_tokens(&ctx, "α,β", "scan SLDPRT hole child references");
            children.next().unwrap()
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.reason == ResourceFailure::BudgetExceeded
            && limit.operation == "scan SLDPRT hole child references"
            && limit.used == 0
            && limit.additional == 1
            && limit.limit == 0));
}

#[test]
fn child_tokens_preserve_leading_repeated_and_trailing_delimiters() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let tokens = super::child_tokens(&ctx, ",7,,9,", "test SLDPRT child delimiters")
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(tokens, ["", "7", "", "9", ""]);
    ctx.finish_session().unwrap();
}
