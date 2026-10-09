// SPDX-License-Identifier: Apache-2.0

use super::parameter_record;
use super::super::{parameter_source_kind_text, parse_design_parameter, ParsedParameterSourceKind};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn user_source_kind_fixed_tag_needs_no_temporary_text_and_keeps_original_fuse() {
    let mut bytes = Vec::new();
    crate::test_support::lp_utf16(&mut bytes, "User Parameter");
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let (kind, end) = parameter_source_kind_text(ctx, &bytes, 0).unwrap().unwrap();
        assert!(matches!(kind, ParsedParameterSourceKind::User));
        assert_eq!(kind.as_str(), "User Parameter");
        assert_eq!(end, 32);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original fixed user source refusal")
        else { panic!("zero-work original refusal"); };
        for (input, at) in [(bytes.as_slice(), 0), (&[][..], usize::MAX)] {
            assert!(matches!(parameter_source_kind_text(ctx, input, at),
                Err(CodecError::ResourceLimit(later)) if later == original));
        }
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn user_source_kind_retains_only_actual_parsed_expression_name_and_class() {
    let bytes = parameter_record(None, "1", "User Parameter", None, "p", 1.0);
    // Only expression "1", name "p" and the three-byte class tag survive parsing.
    for retained in [4, 5] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_materialized_bytes = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let parsed = parse_design_parameter(ctx, &bytes);
            if retained == 4 {
                assert!(matches!(parsed, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.used == 2 && limit.additional == 3
                        && limit.operation == "copy F3D Design parameter class tag"));
            } else {
                let parsed = parsed.unwrap().unwrap();
                assert!(matches!(parsed.source_kind, ParsedParameterSourceKind::User));
                assert_eq!(parsed.expression, "1");
                assert_eq!(parsed.name, "p");
                assert_eq!(parsed.record_index, 71);
                assert_eq!(ctx.resource_refusal(), None);
            }
        });
    }
}

#[test]
fn user_source_kind_dynamic_owned_tag_keeps_actual_text_and_refusal() {
    let mut bytes = Vec::new();
    crate::test_support::lp_utf16(&mut bytes, "AlongDistance");
    crate::test_support::with_decode_context(|ctx| {
        let (kind, end) = parameter_source_kind_text(ctx, &bytes, 0).unwrap().unwrap();
        assert!(matches!(kind, ParsedParameterSourceKind::Owned(_)));
        assert_eq!(kind.as_str(), "AlongDistance");
        assert_eq!(end, bytes.len());
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(matches!(parameter_source_kind_text(ctx, &bytes, 0),
            Err(CodecError::ResourceLimit(limit)) if limit.additional == 1
                && limit.operation == "validate F3D UTF-16 text"));
    });
}
