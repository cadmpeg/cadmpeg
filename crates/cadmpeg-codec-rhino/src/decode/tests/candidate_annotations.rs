// SPDX-License-Identifier: Apache-2.0

use super::{scan_with_objects, with_transaction_limits, CandidateError, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

#[test]
fn rejected_candidate_annotations_do_not_consume_retained_storage() {
    let scan = scan_with_objects(&[]);
    with_transaction_limits(&scan, u64::MAX, Some(0), Some(8192), |expand| {
        let mut context = DecodeContext::new(&scan, expand).unwrap();
        let mut annotations = AnnotationBuilder::new();
        annotations.exactness("rhino:test:point#candidate", Exactness::Derived);
        context.annotations = annotations.build();
        let original = context.annotations.clone();
        let session = expand.ctx();
        let result = context.validate_candidate_fallible(|_, annotations| {
            assert_eq!(annotations, &original);
            let text = session.copy_retained_text("candidate-only", "candidate annotation mutation")?;
            assert_eq!(text, "candidate-only");
            Err::<(), CodecError>(CodecError::malformed("candidate rejected"))
        });
        assert!(matches!(result, Err(CandidateError::Codec(CodecError::Malformed(ref message))) if message == "candidate rejected"));
        assert_eq!(context.annotations, original);
        let reservation = session.reserve_scoped(8192, "candidate annotation storage released").unwrap();
        drop(reservation);
    });
}

#[test]
fn candidate_annotation_copy_preserves_materialized_refusal() {
    let scan = scan_with_objects(&[]);
    with_transaction_limits(&scan, u64::MAX, None, Some(0), |expand| {
        let mut context = DecodeContext::new(&scan, expand).unwrap();
        let mut annotations = AnnotationBuilder::new();
        annotations.exactness("rhino:test:point#candidate", Exactness::Derived);
        context.annotations = annotations.build();
        let original = context.annotations.clone();
        let result = context.validate_candidate::<()>(|_, _| panic!("copy must refuse before applying the candidate"));
        assert!(matches!(result, Err(CandidateError::Codec(CodecError::ResourceLimit(limit))) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes && limit.operation == "Rhino speculative annotations"));
        assert_eq!(context.annotations, original);
    });
}

#[test]
fn candidate_native_projection_preserves_materialized_refusal() {
    let scan = scan_with_objects(&[super::object_record(
        super::ArchiveVersion::V5, 1, super::POINT_CLASS,
    )]);
    with_transaction_limits(&scan, u64::MAX, None, Some(0), |expand| {
        let mut context = DecodeContext::new(&scan, expand).unwrap();
        let before = context.ir.clone();
        let result = context.validate_candidate::<()>(|_, _| ());
        let Err(CandidateError::Codec(CodecError::ResourceLimit(limit))) = result else {
            panic!("native projection refusal must reach the candidate unchanged");
        };
        assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
        assert_eq!(context.ir, before);
        assert!(matches!(expand.ctx().charge_work(0, "test native projection fuse"), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    });
}

fn admission_findings() -> cadmpeg_ir::report::check::ValidationReport {
    use cadmpeg_ir::report::{check::{Check, Finding}, Severity};
    cadmpeg_ir::report::check::ValidationReport {
        entity_counts: Default::default(),
        findings: vec![
            Finding { check: Check::Annotations, severity: Severity::Warning, message: "skip warning".into(), entity: None },
            Finding { check: Check::Identity, severity: Severity::Error, message: "invalid identity".into(), entity: Some("rhino:test:point#1".into()) },
            Finding { check: Check::NativeLinks, severity: Severity::Blocking, message: "unresolved link".into(), entity: None },
            Finding { check: Check::ArenaOrder, severity: Severity::Error, message: "unsorted arena".into(), entity: None },
            Finding { check: Check::Counts, severity: Severity::Error, message: "fourth finding".into(), entity: None },
        ],
        losses: Vec::new(),
    }
}

#[test]
fn candidate_validation_message_preserves_order_and_three_error_limit() {
    let report = admission_findings();
    let text = super::super::validation_findings(&cadmpeg_test_support::service_decode_context(), &report).unwrap();
    assert_eq!(text, "identity (rhino:test:point#1): invalid identity; native_links: unresolved link; arena_order: unsorted arena");
}

#[test]
fn candidate_validation_message_preserves_resource_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let report = admission_findings();
    for dimension in [ResourceDimension::WorkUnits, ResourceDimension::RetainedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = super::super::validation_findings(&ctx, &report) else { panic!("finding construction must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}
