// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use crate::draft::{CommitSession, DraftAccounting, DraftError, ModelDraft};
use crate::math::Point3;
use crate::provenance::Exactness;
use crate::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};
use crate::topology::{Point, Vertex};
use crate::{Annotations, CadIr, CodecFormat};

fn point(id: &str) -> Point {
    Point::new(
        id.try_into().expect("point identity"),
        crate::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite position"),
        None,
    )
}

fn point_draft(id: &str, exactness: Exactness) -> ModelDraft<DraftAccounting> {
    let mut draft = ModelDraft::new().with_accounting();
    draft.insert(point(id)).expect("candidate point");
    draft.exactness(id, exactness);
    draft
}

fn spec(level: &str) -> PartSpec {
    PartSpec::new(
        PartLevel::parse(level).expect("Appendix C level"),
        Vec::new(),
    )
}

#[test]
fn a_failed_child_loses_its_admitted_exactness_while_the_parent_and_peer_survive() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::service())
        .expect("registered source");
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let mut annotations = Annotations::default();
    let prefix = "test:model:point#prefix";
    let rejected = "test:model:point#failed";
    let surviving = "test:model:point#peer";
    session
        .commit(point_draft(prefix, Exactness::Inferred), &mut annotations)
        .expect("prefix admission")
        .expect("valid prefix");
    let mut parts = ParserPartOutput::new(CodecFormat::Fcstd);
    let first = root.child(0, 1).expect("first child");
    let second = root.child(1, 2).expect("second child");
    let result = session
        .annotated_model_append_transaction(&mut annotations, |parent_stage| {
            ctx.decode_parser_scope(&mut parts, root, spec("zip.archive"), |parent_parts| {
                assert_eq!(
                    parent_stage.annotated_model_append_transaction(|child_stage| {
                        ctx.decode_parser_part(parent_parts, first, spec("zip.entry"), || {
                            ctx.record(rejected, first, || {
                                assert_eq!(first.read_at(0, 1), Some(b"A".as_slice()));
                                child_stage
                                    .commit(point_draft(rejected, Exactness::Derived))?
                                    .expect("valid tentative child");
                                assert_eq!(
                                    child_stage.annotations().exactness()[rejected].entity(),
                                    Exactness::Derived
                                );
                                Err::<(), _>(CodecError::malformed("child parser cannot finish"))
                            })
                        })
                    })?,
                    None
                );
                assert!(!parent_stage.contains(rejected)?);
                assert!(!parent_stage
                    .annotations()
                    .exactness()
                    .contains_key(rejected));
                assert_eq!(
                    parent_stage.annotated_model_append_transaction(|child_stage| {
                        ctx.decode_parser_part(parent_parts, second, spec("zip.entry"), || {
                            ctx.record(surviving, second, || {
                                assert_eq!(second.read_at(1, 1), Some(b"B".as_slice()));
                                child_stage
                                    .commit(point_draft(surviving, Exactness::Derived))?
                                    .expect("valid peer admission");
                                Ok(7)
                            })
                        })
                    })?,
                    Some(7)
                );
                Ok(9)
            })
        })
        .expect("successful parent");
    assert_eq!(result, Some(9));
    assert_eq!(session.document().model.points.len(), 2);
    assert_eq!(annotations.exactness().len(), 2);
    assert_eq!(
        annotations.exactness()[prefix].entity(),
        Exactness::Inferred
    );
    assert_eq!(
        annotations.exactness()[surviving].entity(),
        Exactness::Derived
    );
    assert!(!annotations.exactness().contains_key(rejected));
    let (_, native, fidelity, losses) = parts
        .into_parts(&ctx)
        .expect("parser dependency closure")
        .into_parts();
    assert_eq!(fidelity.parts().len(), 3);
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(
        native
            .namespace("fcstd")
            .expect("failure namespace")
            .arena_as::<crate::NativeUnknownRecord>("unknowns")
            .expect("failure home")
            .len(),
        1
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    let captures = ctx.take_record_captures().expect("one capture drain");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), surviving);
}

#[test]
fn a_parent_give_up_discards_a_successful_child_transfer_without_copying_prefix_notes() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let mut annotations = Annotations::default();
    let prefix = "test:model:point#prefix";
    let child = "test:model:point#child";
    session
        .commit(point_draft(prefix, Exactness::Inferred), &mut annotations)
        .expect("prefix admission")
        .expect("valid prefix");
    let prefix_key = annotations
        .exactness()
        .keys()
        .next()
        .expect("prefix key")
        .as_ptr();
    assert_eq!(
        session
            .annotated_model_append_transaction(&mut annotations, |parent| {
                assert_eq!(
                    parent.annotated_model_append_transaction(|stage| {
                        stage
                            .commit(point_draft(child, Exactness::Derived))?
                            .expect("valid child admission");
                        Ok(Some(7))
                    })?,
                    Some(7)
                );
                assert!(parent.annotations().exactness().contains_key(child));
                Ok(None::<()>)
            })
            .expect("parent give-up"),
        None
    );
    assert_eq!(session.document().model.points.len(), 1);
    assert_eq!(annotations.exactness().len(), 1);
    assert_eq!(
        annotations.exactness()[prefix].entity(),
        Exactness::Inferred
    );
    assert_eq!(
        annotations
            .exactness()
            .keys()
            .next()
            .expect("prefix key")
            .as_ptr(),
        prefix_key
    );
    session
        .annotated_model_append_transaction(&mut annotations, |stage| {
            stage
                .commit(point_draft(child, Exactness::Derived))?
                .expect("re-admit rolled-back identity and exactness");
            Ok(Some(()))
        })
        .expect("peer publication")
        .expect("published peer");
    assert_eq!(annotations.exactness()[child].entity(), Exactness::Derived);
}

#[test]
fn actual_candidate_refusals_and_unowned_annotation_edits_leave_both_destinations_unchanged() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let prefix = "test:model:point#prefix";
    let fresh = "test:model:point#fresh";
    let mut base = CadIr::empty();
    base.model.points.push(point(prefix));
    let mut session = CommitSession::new(base, &ctx, None).expect("commit session");
    let mut annotations = Annotations::default();
    session
        .annotated_model_append_transaction(&mut annotations, |stage| {
            assert!(matches!(
                stage.commit(point_draft(prefix, Exactness::Derived))?,
                Err(DraftError::IdentityCollision(id)) if id == prefix
            ));
            let mut missing = ModelDraft::new().with_accounting();
            let vertex = "test:model:vertex#missing";
            missing
                .insert(Vertex {
                    id: vertex.try_into().expect("vertex identity"),
                    point: fresh.try_into().expect("missing target"),
                    tolerance: None,
                })
                .expect("candidate vertex");
            missing.exactness(vertex, Exactness::Derived);
            assert!(matches!(stage.commit(missing)?,
                Err(DraftError::UnresolvedReference { owner, target })
                    if owner == vertex && target == fresh));
            let mut unowned = point_draft(fresh, Exactness::Derived);
            unowned.exactness(prefix, Exactness::Derived);
            assert!(matches!(
                stage.commit(unowned)?,
                Err(DraftError::Admission(_))
            ));
            assert_eq!(stage.document().model.points.len(), 1);
            assert!(stage.annotations().exactness().is_empty());
            Ok(Some(()))
        })
        .expect("refused candidates")
        .expect("unchanged outer operation");
    assert_eq!(session.document().model.points.len(), 1);
    assert!(annotations.exactness().is_empty());
}

#[test]
fn preexisting_annotation_owners_cannot_be_replaced_even_before_their_record_is_present() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let id = "test:model:point#held-note";
    let mut builder = crate::annotations::AnnotationBuilder::new();
    builder.exactness(id, Exactness::Inferred);
    let mut annotations = builder.build();
    let before = annotations.clone();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    session
        .annotated_model_append_transaction(&mut annotations, |stage| {
            assert!(matches!(
                stage.commit(point_draft(id, Exactness::Derived))?,
                Err(DraftError::Admission(_))
            ));
            Ok(Some(()))
        })
        .expect("refused annotation edit")
        .expect("unchanged operation");
    assert!(session.document().model.points.is_empty());
    assert_eq!(annotations, before);
}

#[test]
fn a_swallowed_real_caller_refusal_discards_exactness_and_records_together() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"A", &arena, &policy).expect("caller context");
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let mut annotations = Annotations::default();
    let result = session.annotated_model_append_transaction(&mut annotations, |stage| {
        stage
            .commit(point_draft(
                "test:model:point#tentative",
                Exactness::Derived,
            ))?
            .expect("valid tentative admission");
        assert!(matches!(
            ctx.charge_entities(1, "actual candidate admission"),
            Err(CodecError::ResourceLimit(_))
        ));
        Ok(Some(()))
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
    assert!(session.document().model.points.is_empty());
    assert!(annotations.exactness().is_empty());
    assert!(ctx.resource_refusal().is_some());
}

#[test]
fn unwinding_rolls_back_accounted_appends_and_preserves_the_prefix() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let mut annotations = Annotations::default();
    let prefix = "test:model:point#prefix";
    session
        .commit(point_draft(prefix, Exactness::Inferred), &mut annotations)
        .expect("prefix admission")
        .expect("valid prefix");
    let before = annotations.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _result = session.annotated_model_append_transaction::<()>(&mut annotations, |stage| {
            stage
                .commit(point_draft(
                    "test:model:point#tentative",
                    Exactness::Derived,
                ))?
                .expect("valid tentative admission");
            panic!("outer parser unwound");
        });
    }));
    assert!(result.is_err());
    assert_eq!(session.document().model.points.len(), 1);
    assert_eq!(annotations, before);
}

#[test]
fn an_actual_shared_output_preflight_error_rolls_back_records_notes_and_reads() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"A", &arena, &DecodePolicy::service())
        .expect("registered source");
    let mut parts = ParserPartOutput::new(CodecFormat::Fcstd);
    ctx.decode_parser_part(&mut parts, root, spec("zip.entry"), || Ok(()))
        .expect("first actual part invocation")
        .expect("decoded first part");
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let mut annotations = Annotations::default();
    let id = "test:model:point#unpublished";
    let admitted = std::cell::Cell::new(false);
    let error = session
        .annotated_model_append_transaction(&mut annotations, |stage| {
            ctx.decode_parser_part(&mut parts, root, spec("zip.entry"), || {
                ctx.record(id, root, || {
                    assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
                    stage
                        .commit(point_draft(id, Exactness::Derived))?
                        .expect("valid tentative candidate");
                    admitted.set(true);
                    Ok(7)
                })
            })
        })
        .expect_err("shared helper rejects the actual duplicate invocation identity");
    assert!(
        admitted.get(),
        "the shared callback admitted its candidate before output preflight failed"
    );
    assert!(matches!(error, CodecError::Malformed(ref message)
        if message.contains("duplicate part invocation identity")));
    assert!(session.document().model.points.is_empty());
    assert!(annotations.exactness().is_empty());
    assert!(ctx.take_record_captures().expect("single drain").is_empty());
    let (_, native, fidelity, losses) = parts
        .into_parts(&ctx)
        .expect("unchanged original parser output")
        .into_parts();
    assert_eq!(fidelity.parts().len(), 1);
    assert!(fidelity.retained_records().is_empty());
    assert!(native.namespace("fcstd").is_none());
    assert!(losses.is_empty());
}
