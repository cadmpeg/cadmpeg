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
