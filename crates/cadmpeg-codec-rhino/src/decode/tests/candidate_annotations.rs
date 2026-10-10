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
        annotations
            .exactness(
                &cadmpeg_test_support::service_decode_context(),
                "rhino:test:point#candidate",
                Exactness::Derived,
            )
            .unwrap();
        context.annotations = annotations.build();
        let original = context.annotations.clone();
        let session = expand.ctx();
        let result = context.validate_candidate_fallible(|_, annotations| {
            assert_eq!(annotations.base(), &original);
            assert!(annotations.annotations().exactness().is_empty());
            let text =
                session.copy_retained_text("candidate-only", "candidate annotation mutation")?;
            assert_eq!(text, "candidate-only");
            Err::<(), CodecError>(CodecError::malformed("candidate rejected"))
        });
        assert!(
            matches!(result, Err(CandidateError::Codec(CodecError::Malformed(ref message))) if message == "candidate rejected")
        );
        assert_eq!(context.annotations, original);
        let reservation = session
            .reserve_scoped(8192, "candidate annotation storage released")
            .unwrap();
        drop(reservation);
    });
}

#[test]
fn candidate_annotation_edit_preserves_materialized_refusal() {
    let scan = scan_with_objects(&[]);
    with_transaction_limits(&scan, u64::MAX, None, Some(0), |expand| {
        let mut context = DecodeContext::new(&scan, expand).unwrap();
        let original = context.annotations.clone();
        let result = context.validate_candidate_fallible(|_, annotations| {
            annotations.exactness("rhino:test:point#candidate", Exactness::Derived)
        });
        assert!(
            matches!(result, Err(CandidateError::Codec(CodecError::ResourceLimit(limit)))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
        );
        assert_eq!(context.annotations, original);
    });
}

#[test]
fn candidate_native_projection_preserves_materialized_refusal() {
    let scan = scan_with_objects(&[super::object_record(
        super::ArchiveVersion::V5,
        1,
        super::POINT_CLASS,
    )]);
    // Candidate lookup keys admit scoped storage before unknown record staging.
    with_transaction_limits(&scan, u64::MAX, None, Some(0), |expand| {
        let Err(CodecError::ResourceLimit(first)) = DecodeContext::new(&scan, expand) else {
            panic!("candidate keys must refuse before construction");
        };
        assert_eq!(
            first.dimension,
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes
        );
        assert_eq!(first.operation, "Rhino object candidate keys");
        assert_eq!(expand.ctx().resource_refusal(), Some(first));
    });
    // Native admission borrows unknown records and first reserves their identity-order slots.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "source product identity slots",
        |cap| {
            with_transaction_limits(&scan, u64::MAX, None, Some(cap), |expand| {
                let mut context = DecodeContext::new(&scan, expand)?;
                let before = context.session.document().clone();
                match context.validate_candidate::<()>(|_, _| ()) {
                    Err(CandidateError::Codec(CodecError::ResourceLimit(limit))) => {
                        assert_eq!(
                            limit.dimension,
                            cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                        );
                        assert_eq!(context.session.document(), &before);
                        assert!(
                            matches!(expand.ctx().charge_work(0, "test native projection fuse"),
                        Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
                        );
                        Err(CodecError::ResourceLimit(limit))
                    }
                    Ok(()) => Ok(()),
                    other => panic!("unexpected candidate admission result: {other:?}"),
                }
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "source product identity slots"));
}

fn admission_findings() -> cadmpeg_ir::report::check::ValidationReport {
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };
    cadmpeg_ir::report::check::ValidationReport {
        entity_counts: std::collections::BTreeMap::default(),
        findings: vec![
            Finding {
                check: Check::Annotations,
                severity: Severity::Warning,
                message: "skip warning".into(),
                entity: None,
            },
            Finding {
                check: Check::Identity,
                severity: Severity::Error,
                message: "invalid identity".into(),
                entity: Some("rhino:test:point#1".into()),
            },
            Finding {
                check: Check::NativeLinks,
                severity: Severity::Blocking,
                message: "unresolved link".into(),
                entity: None,
            },
            Finding {
                check: Check::ArenaOrder,
                severity: Severity::Error,
                message: "unsorted arena".into(),
                entity: None,
            },
            Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: "fourth finding".into(),
                entity: None,
            },
        ],
        losses: Vec::new(),
    }
}

#[test]
fn candidate_validation_message_preserves_order_and_three_error_limit() {
    let report = admission_findings();
    let text =
        super::super::validation_findings(&cadmpeg_test_support::service_decode_context(), &report)
            .unwrap();
    assert_eq!(text, "identity (rhino:test:point#1): invalid identity; native_links: unresolved link; arena_order: unsorted arena");
}

#[test]
fn candidate_validation_message_preserves_resource_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let report = admission_findings();
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::RetainedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) =
            super::super::validation_findings(&ctx, &report)
        else {
            panic!("finding construction must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}

#[test]
fn candidate_session_reuses_source_identity_and_borrows_retained_image() {
    let scan = scan_with_objects(&[super::object_record(
        super::ArchiveVersion::V5,
        1,
        super::POINT_CLASS,
    )]);
    super::with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).unwrap();
        let id = context.session.unknowns()[0].id().as_str().to_owned();
        let image = context.session.unknowns()[0].data().unwrap().as_ptr();
        assert!(context.session.contains(&id).unwrap());
        for _ in 0..2 {
            context.validate_candidate(|_, _| ()).unwrap();
            assert!(context.session.contains(&id).unwrap());
            assert_eq!(
                context.session.unknowns()[0].data().unwrap().as_ptr(),
                image
            );
            assert!(context
                .session
                .document()
                .native_unknowns("rhino")
                .unwrap()
                .is_empty());
        }
        let decoded = context.commit().unwrap();
        assert_eq!(decoded.ir.native_unknowns("rhino").unwrap().len(), 1);
        assert_eq!(decoded.source_fidelity.retained_records().len(), 1);
    });
}

#[test]
fn candidate_source_link_grammar_failure_preserves_admission_classification() {
    let scan = scan_with_objects(&[super::object_record(
        super::ArchiveVersion::V5,
        1,
        super::POINT_CLASS,
    )]);
    super::with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).unwrap();
        context
            .session
            .unknown_links_mut(0)
            .unwrap()
            .unwrap()
            .1
            .push("invalid".into());
        let before = context.session.document().clone();
        let Err(CandidateError::Admission(message)) = context.validate_candidate(|_, _| ()) else {
            panic!("source link grammar remains an admission failure");
        };
        let expected =
            cadmpeg_ir::NativeUnknownRecord::try_from(&context.session.unknowns()[0]).unwrap_err();
        assert_eq!(message, expected.to_string());
        assert_eq!(context.session.document(), &before);
    });
}

#[test]
fn source_link_insertion_preserves_work_refusal_before_mutation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut links = vec!["test:model:point#later".to_owned()];
    let before = links.clone();
    let Err(CodecError::ResourceLimit(limit)) = super::super::append_link_to_record(
        &ctx,
        "test:source:unknown#owner",
        &mut links,
        "test:model:point#earlier",
    ) else {
        panic!("comparison work must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(links, before);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}
