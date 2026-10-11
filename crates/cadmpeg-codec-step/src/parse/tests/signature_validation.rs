// SPDX-License-Identifier: Apache-2.0
//! Signature validation errors retain their escaped text backing.

use cadmpeg_core::decode::DecodePolicy;

#[test]
fn signature_validation_error_escapes_with_retained_storage() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;AAAA\nENDSEC;";
    crate::test_support::with_service_context(SOURCE, |input, ctx| {
        let error = crate::parse::parse_inner(input, ctx).unwrap_err();
        assert!(
            matches!(error, crate::parse::ParseError::Syntax { offset, ref message }
            if offset == SOURCE.windows(4).position(|window| window == b"AAAA").unwrap()
                && message == "invalid detached CMS SIGNATURE payload: unexpected BER tag")
        );
    });
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "STEP signature validation error",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            crate::test_support::with_policy_context(SOURCE, &policy, |input, ctx| {
                crate::parse::parse_inner(input, ctx)
                    .map(|_| ())
                    .map_err(|error| match error {
                        crate::parse::ParseError::Resource(error) => error,
                        crate::parse::ParseError::Syntax { .. } => {
                            cadmpeg_core::CodecError::malformed("syntax result")
                        }
                        error => panic!("unexpected parse error: {error:?}"),
                    })
            })
        },
    );
}
