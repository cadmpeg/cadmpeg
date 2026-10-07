// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;

fn incomplete_native_segment() -> CadIr {
    let mut ir = CadIr::empty();
    ir.native.0.insert(
        "nx".into(),
        serde_json::from_value(serde_json::json!({
            "display_jt_segments": [{"id": "nx:display-jt:segment#0"}]
        }))
        .unwrap(),
    );
    ir
}

#[test]
fn native_validation_format_preserves_the_finding() {
    let ir = incomplete_native_segment();
    let namespace = ir.native.namespace("nx").unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let expected =
            crate::native::display_jt::admission::DisplayJtGraph::from_namespace_with_context(
                ctx, namespace,
            )
            .unwrap_err()
            .to_string();
        let findings = crate::NxCodec::validate_native(ctx, &ir).unwrap();
        assert_eq!(
            findings,
            vec![Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: expected,
                entity: None,
            }],
        );
    });
}

fn validation_message_refusal(dimension: ResourceDimension) {
    let ir = incomplete_native_segment();
    cadmpeg_test_support::refusal::resource_limit_at(
        dimension,
        "NX native validation message",
        |cap| {
            crate::test_support::with_decode_context_over(
                &[],
                |policy| match dimension {
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                    _ => panic!("finding messages use work and retained bytes"),
                },
                |ctx| {
                    let result = crate::NxCodec::validate_native(ctx, &ir);
                    if let Err(CodecError::ResourceLimit(limit)) = &result {
                        assert_eq!(ctx.resource_refusal(), Some(*limit));
                    }
                    result
                },
            )
        },
    );
}

#[test]
fn native_validation_format_refuses_work() {
    validation_message_refusal(ResourceDimension::WorkUnits);
}

#[test]
fn native_validation_format_refuses_retained_bytes() {
    validation_message_refusal(ResourceDimension::RetainedBytes);
}

#[test]
fn native_validation_propagates_namespace_admission_refusal() {
    let ir = incomplete_native_segment();
    crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "load typed native record",
        |ctx| crate::NxCodec::validate_native(ctx, &ir),
    );
}
