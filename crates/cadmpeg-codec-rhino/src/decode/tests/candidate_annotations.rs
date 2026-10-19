// SPDX-License-Identifier: Apache-2.0

use super::{scan_with_objects, with_expand, CandidateError, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

fn admission_findings() -> cadmpeg_ir::report::check::ValidationReport {
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };
    cadmpeg_ir::report::check::ValidationReport {
        entity_counts: std::collections::BTreeMap::default(),
        findings: vec![
            Finding {
                related: Vec::new(),
                check: Check::Annotations,
                severity: Severity::Warning,
                message: "skip warning".into(),
                entity: None,
            },
            Finding {
                related: Vec::new(),
                check: Check::Identity,
                severity: Severity::Error,
                message: "invalid identity".into(),
                entity: Some("rhino:test:point#1".into()),
            },
            Finding {
                related: Vec::new(),
                check: Check::NativeLinks,
                severity: Severity::Blocking,
                message: "unresolved link".into(),
                entity: None,
            },
            Finding {
                related: Vec::new(),
                check: Check::ArenaOrder,
                severity: Severity::Error,
                message: "unsorted arena".into(),
                entity: None,
            },
            Finding {
                related: Vec::new(),
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
    let text = super::super::validation_findings(&report);
    assert_eq!(text, "identity (rhino:test:point#1): invalid identity; native_links: unresolved link; arena_order: unsorted arena");
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
fn rejected_candidate_leaves_annotations_unchanged() {
    let scan = scan_with_objects(&[]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).unwrap();
        let mut annotations = AnnotationBuilder::new();
        annotations.exactness("rhino:test:point#candidate", Exactness::Derived);
        context.annotations = annotations.build();
        let original = context.annotations.clone();
        let result = context.validate_candidate_fallible(|_, annotations| {
            assert_eq!(annotations.base(), &original);
            assert!(annotations.annotations().exactness().is_empty());
            Err::<(), CodecError>(CodecError::malformed("candidate rejected"))
        });
        assert!(
            matches!(result, Err(CandidateError::Codec(CodecError::Malformed(ref message))) if message == "candidate rejected")
        );
        assert_eq!(context.annotations, original);
    });
}
