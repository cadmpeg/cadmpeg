use super::super::hole_child_tokens;
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
        let tokens = hole_child_tokens(&ctx, text)
            .unwrap()
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
            let result = hole_child_tokens(&ctx, "α,β")?.next().unwrap();
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.reason == ResourceFailure::BudgetExceeded
            && limit.operation == "scan SLDPRT hole child references"));
}
