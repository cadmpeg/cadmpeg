// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use crate::draft::{CommitSession, DraftError, ModelDraft};
use crate::math::Point3;
use crate::source_fidelity::parts::{
    DecodeParts, ParserPartOutput, PartLevel, PartSpec, PartState,
};
use crate::topology::{Point, Vertex};
use crate::{CadIr, CodecFormat};

fn point_draft(id: &str) -> ModelDraft {
    let mut draft = ModelDraft::new();
    draft
        .insert(Point::new(
            id.try_into().expect("point identity"),
            crate::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite point"),
            None,
        ))
        .expect("insert candidate point");
    draft
}

fn vertex_draft(id: &str, point: &str) -> ModelDraft {
    let mut draft = ModelDraft::new();
    draft
        .insert(Vertex {
            native_ref: None,
            id: id.try_into().expect("vertex identity"),
            point: point.try_into().expect("point reference"),
            tolerance: None,
        })
        .expect("insert candidate vertex");
    draft
}

fn spec(level: &str) -> PartSpec {
    PartSpec::new(
        PartLevel::parse(level).expect("Appendix C level"),
        Vec::new(),
    )
}

fn feature(id: &str, ordinal: u64) -> crate::features::Feature {
    use crate::features::{FeatureDefinition, FeatureEvaluation, FeatureOperation};
    crate::features::Feature {
        id: id.try_into().expect("feature identity"),
        ordinal,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        )),
        native_ref: None,
    }
}

fn feature_pair(parent: &str, child: &str) -> ModelDraft {
    let mut draft = ModelDraft::new();
    draft.insert(feature(parent, 20)).expect("parent feature");
    draft.insert(feature(child, 30)).expect("child feature");
    draft
        .model_mut()
        .set_feature_regeneration_parent(
            &child.try_into().expect("child identity"),
            &parent.try_into().expect("parent identity"),
        )
        .expect("candidate regeneration relation");
    draft
}

#[test]
fn a_given_up_parent_discards_admitted_child_geometry_and_keeps_the_valid_peer() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"ABCD", &arena, &DecodePolicy::service())
        .expect("registered source");
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let existing = "test:model:point#prefix";
    let rejected = "test:model:point#failed-child";
    let surviving = "test:model:point#valid-peer";
    session
        .commit_model(point_draft(existing))
        .expect("prefix admission")
        .expect("valid prefix");
    let mut parts = ParserPartOutput::new(CodecFormat::Fcstd);
    let first = root.child(0, 2).expect("first parent");
    let first_child = first.child(0, 1).expect("first child");
    let result = session
        .model_append_transaction(|stage| {
            ctx.decode_parser_scope(&mut parts, first, spec("zip.archive"), |parent| {
                ctx.decode_parser_part(parent, first_child, spec("zip.entry"), || {
                    ctx.record(rejected, first_child, || {
                        assert_eq!(first_child.read_at(0, 1), Some(b"A".as_slice()));
                        stage
                            .commit_model(point_draft(rejected))?
                            .expect("child is valid before its parent fails");
                        assert!(stage.contains(rejected)?);
                        Ok(())
                    })
                })?;
                Err::<(), _>(CodecError::malformed("parent parser cannot finish"))
            })
        })
        .expect("bounded parent give-up");
    assert_eq!(result, None);
    assert!(session.contains(existing).expect("prefix cache"));
    assert!(!session.contains(rejected).expect("discarded child cache"));
    assert_eq!(session.document().model.points.len(), 1);

    let second = root.child(2, 4).expect("second parent");
    let second_child = second.child(2, 3).expect("second child");
    assert_eq!(
        session
            .model_append_transaction(|stage| {
                ctx.decode_parser_scope(&mut parts, second, spec("zip.archive"), |parent| {
                    ctx.decode_parser_part(parent, second_child, spec("zip.entry"), || {
                        ctx.record(surviving, second_child, || {
                            assert_eq!(second_child.read_at(2, 1), Some(b"C".as_slice()));
                            stage
                                .commit_model(point_draft(surviving))?
                                .expect("valid peer geometry");
                            stage
                                .commit_model(vertex_draft(
                                    "test:model:vertex#valid-peer",
                                    surviving,
                                ))?
                                .expect("reference to the peer's admitted point");
                            Ok(7)
                        })
                    })
                })
            })
            .expect("second parent"),
        Some(Some(7))
    );
    assert_eq!(session.document().model.points.len(), 2);
    assert_eq!(session.document().model.vertices.len(), 1);
    assert_eq!(
        session.document().model.vertices[0].point.as_str(),
        surviving
    );

    let (_, native, fidelity, losses) = parts
        .into_parts(&ctx)
        .expect("parser dependency closure")
        .into_parts();
    assert_eq!(fidelity.parts().len(), 4);
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .count(),
        1
    );
    assert_eq!(
        native
            .namespace("fcstd")
            .expect("single failure namespace")
            .arena_as::<crate::NativeUnknownRecord>("unknowns")
            .expect("typed failure stub")
            .len(),
        1
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(
        fidelity
            .retained_records()
            .values()
            .next()
            .expect("one retained parent")
            .data(),
        Some(b"AB".as_slice())
    );
    let (failed_id, failed_parent) = fidelity
        .parts()
        .iter()
        .find(|(_, part)| part.state() == &PartState::GivenUp {})
        .expect("one given-up parent");
    assert_eq!(failed_parent.level().as_str(), "zip.archive");
    assert_eq!(failed_parent.offset(), 0);
    assert_eq!(failed_parent.byte_len(), 2);
    assert!(fidelity.parts().values().any(|part| {
        part.parent() == Some(failed_id)
            && part.state()
                == &PartState::Covered {
                    by: failed_id.clone(),
                }
    }));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, crate::report::Severity::Error);
    assert!(losses[0].message.contains(rejected));
    let captures = ctx.take_record_captures().expect("single capture drain");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), surviving);
}

#[test]
fn real_candidate_rejections_and_outer_errors_preserve_the_prefix_and_allow_identity_reuse() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let prefix = "test:model:point#prefix";
    let candidate = "test:model:point#candidate";
    session
        .commit_model(point_draft(prefix))
        .expect("prefix admission")
        .expect("valid prefix");
    let error = session
        .model_append_transaction(|stage| {
            assert!(matches!(
                stage.commit_model(point_draft(prefix))?,
                Err(DraftError::IdentityCollision(id)) if id == prefix
            ));
            assert!(matches!(
                stage.commit_model(vertex_draft("test:model:vertex#missing", candidate))?,
                Err(DraftError::UnresolvedReference { owner, target })
                    if owner == "test:model:vertex#missing" && target == candidate
            ));
            stage
                .commit_model(point_draft(candidate))?
                .expect("admit candidate");
            assert_eq!(stage.document().model.points.len(), 2);
            Err::<Option<()>, _>(CodecError::malformed("outer output preflight rejected"))
        })
        .expect_err("outer rejection");
    assert!(matches!(error, CodecError::Malformed(_)));
    assert_eq!(session.document().model.points.len(), 1);
    assert!(session.document().model.vertices.is_empty());
    assert!(!session.contains(candidate).expect("rolled-back identity"));
    session
        .model_append_transaction(|stage| {
            stage
                .commit_model(point_draft(candidate))?
                .expect("re-admit the rolled-back identity");
            Ok(Some(()))
        })
        .expect("successful publication")
        .expect("published candidate");
    assert!(session.contains(candidate).expect("published identity"));
}

#[test]
fn a_real_caller_refusal_rolls_back_model_appends_without_resetting_the_budget() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"A", &arena, &policy).expect("caller context");
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let error = session
        .model_append_transaction(|stage| {
            stage
                .commit_model(point_draft("test:model:point#tentative"))?
                .expect("valid neutral candidate");
            ctx.charge_entities(1, "actual candidate admission")?;
            Ok(Some(()))
        })
        .expect_err("caller entity limit");
    assert!(matches!(error, CodecError::ResourceLimit(_)));
    assert!(session.document().model.points.is_empty());
    drop(session);
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn swallowing_a_real_caller_refusal_cannot_publish_or_reenter_the_transaction() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"A", &arena, &policy).expect("caller context");
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let error = session
        .model_append_transaction(|stage| {
            stage
                .commit_model(point_draft("test:model:point#tentative"))?
                .expect("valid neutral candidate");
            let refusal = ctx
                .charge_entities(1, "actual candidate admission")
                .expect_err("real caller entity refusal");
            assert!(matches!(refusal, CodecError::ResourceLimit(_)));
            Ok(Some(()))
        })
        .expect_err("swallowed refusal remains fatal");
    let CodecError::ResourceLimit(first) = error else {
        panic!("actual caller resource refusal");
    };
    assert!(session.document().model.points.is_empty());
    let later = session
        .model_append_transaction::<()>(|_stage| panic!("latched refusal must prevent entry"))
        .expect_err("same latched refusal");
    assert!(matches!(later, CodecError::ResourceLimit(limit) if limit == first));
}

#[test]
fn prefix_parent_edits_are_rejected_and_only_appended_feature_relations_are_rolled_back() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut base = CadIr::empty();
    let prefix = "test:model:feature#prefix";
    base.model.features.push(feature(prefix, 10));
    let mut session = CommitSession::new(base, &ctx, None).expect("commit session");
    let parent = "test:model:feature#parent";
    let child = "test:model:feature#child";
    let mut prefix_edit = ModelDraft::new();
    let prospective_parent = "test:model:feature#prospective-parent";
    prefix_edit
        .insert(feature(prospective_parent, 5))
        .expect("prospective parent");
    prefix_edit
        .insert(feature(prefix, 10))
        .expect("local prefix representation");
    prefix_edit
        .model_mut()
        .set_feature_regeneration_parent(
            &prefix.try_into().expect("prefix identity"),
            &prospective_parent
                .try_into()
                .expect("prospective parent identity"),
        )
        .expect("locally consistent parent edit");
    prefix_edit.model_mut().features.pop();
    assert_eq!(
        session
            .model_append_transaction(|stage| {
                assert!(matches!(
                    stage.commit_model(prefix_edit)?,
                    Err(DraftError::FeatureParents { owner, .. }) if owner.as_str() == prefix
                ));
                stage
                    .commit_model(feature_pair(parent, child))?
                    .expect("valid appended feature graph");
                assert!(stage
                    .document()
                    .model
                    .feature_regeneration_parent(&child.try_into().expect("child identity"))
                    .is_some());
                Ok(None::<()>)
            })
            .expect("discard operation"),
        None
    );
    assert_eq!(session.document().model.features.len(), 1);
    assert!(session
        .document()
        .model
        .feature_regeneration_parent(&prefix.try_into().expect("prefix identity"))
        .is_none());
    assert!(session
        .document()
        .model
        .feature_regeneration_parent(&child.try_into().expect("child identity"))
        .is_none());
    session
        .model_append_transaction(|stage| {
            stage
                .commit_model(feature_pair(parent, child))?
                .expect("re-admit the rolled-back feature graph");
            Ok(Some(()))
        })
        .expect("successful publication")
        .expect("published feature graph");
    assert_eq!(
        session
            .document()
            .model
            .feature_regeneration_parent(&child.try_into().expect("child identity"))
            .map(|id| id.as_str()),
        Some(parent)
    );
}

#[test]
fn unwinding_discards_the_suffix_without_changing_native_or_source_records() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut base = CadIr::empty();
    let native_id = "test:native:record#prefix";
    base.native.namespace_mut("test").arenas_mut().insert(
        "records".to_owned(),
        vec![crate::NativeRecord::new(
            native_id.try_into().expect("native identity"),
            serde_json::Map::new(),
        )
        .expect("native prefix")],
    );
    let native_pointer = base
        .native
        .namespace("test")
        .expect("native prefix")
        .arenas()["records"]
        .as_ptr();
    let source_id = "test:source:unknown#prefix";
    let source = crate::UnknownRecord::retained(
        source_id.try_into().expect("source identity"),
        7,
        vec![1, 2, 3],
        Vec::new(),
    );
    let mut session = CommitSession::new(base, &ctx, None).expect("commit session");
    session.push_unknown(source).expect("source prefix");
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _result = session.model_append_transaction::<()>(|stage| {
            stage
                .commit_model(point_draft("test:model:point#unwound"))?
                .expect("valid tentative point");
            panic!("outer parser unwound");
        });
    }));
    assert!(caught.is_err());
    assert!(session.document().model.points.is_empty());
    assert_eq!(
        session
            .document()
            .native
            .namespace("test")
            .expect("native prefix")
            .arenas()["records"]
            .as_ptr(),
        native_pointer
    );
    assert!(session.contains(native_id).expect("native identity cache"));
    assert_eq!(session.unknowns().len(), 1);
    assert_eq!(session.unknowns()[0].id().as_str(), source_id);
    assert_eq!(session.unknowns()[0].data(), Some([1, 2, 3].as_slice()));
}

#[test]
fn an_independent_child_give_up_rolls_back_only_its_suffix_inside_a_successful_parent() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"AB", &arena, &DecodePolicy::service())
        .expect("registered source");
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let mut parts = ParserPartOutput::new(CodecFormat::Fcstd);
    let rejected = "test:model:point#failed-child";
    let surviving = "test:model:point#valid-peer";
    let first = root.child(0, 1).expect("failed child extent");
    let second = root.child(1, 2).expect("valid peer extent");
    let result = session
        .model_append_transaction(|parent_stage| {
            ctx.decode_parser_scope(&mut parts, root, spec("zip.archive"), |parent_parts| {
                let first_result = parent_stage.model_append_transaction(|child_stage| {
                    ctx.decode_parser_part(parent_parts, first, spec("zip.entry"), || {
                        ctx.record(rejected, first, || {
                            assert_eq!(first.read_at(0, 1), Some(b"A".as_slice()));
                            child_stage
                                .commit_model(point_draft(rejected))?
                                .expect("valid tentative child");
                            Err::<(), _>(CodecError::malformed("child parser cannot finish"))
                        })
                    })
                })?;
                assert_eq!(first_result, None);
                assert!(!parent_stage.contains(rejected)?);
                assert_eq!(
                    parent_stage.model_append_transaction(|child_stage| {
                        ctx.decode_parser_part(parent_parts, second, spec("zip.entry"), || {
                            ctx.record(surviving, second, || {
                                assert_eq!(second.read_at(1, 1), Some(b"B".as_slice()));
                                child_stage
                                    .commit_model(point_draft(surviving))?
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
        .expect("successful parent with recovered child");
    assert_eq!(result, Some(9));
    assert_eq!(session.document().model.points.len(), 1);
    assert_eq!(session.document().model.points[0].id.as_str(), surviving);
    let (_, native, fidelity, losses) = parts
        .into_parts(&ctx)
        .expect("parser dependency closure")
        .into_parts();
    assert_eq!(fidelity.parts().len(), 3);
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|p| p.state() == &PartState::GivenUp {})
            .count(),
        1
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(
        native
            .namespace("fcstd")
            .expect("failure namespace")
            .arena_as::<crate::NativeUnknownRecord>("unknowns")
            .expect("failure stub")
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
fn an_actual_shared_output_preflight_error_rolls_back_records_and_read_captures() {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(b"A", &arena, &DecodePolicy::service())
        .expect("registered source");
    let mut parts = ParserPartOutput::new(CodecFormat::Fcstd);
    ctx.decode_parser_part(&mut parts, root, spec("zip.entry"), || Ok(()))
        .expect("first actual part invocation")
        .expect("decoded first part");
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).expect("commit session");
    let id = "test:model:point#unpublished";
    let admitted = std::cell::Cell::new(false);
    let error = session
        .model_append_transaction(|stage| {
            ctx.decode_parser_part(&mut parts, root, spec("zip.entry"), || {
                ctx.record(id, root, || {
                    assert_eq!(root.read_at(0, 1), Some(b"A".as_slice()));
                    stage
                        .commit_model(point_draft(id))?
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
