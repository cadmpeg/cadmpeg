// SPDX-License-Identifier: Apache-2.0
use super::PartOutput;
use crate::ids::Identity;
use crate::source_fidelity::parts::{DecodeParts, PartLevel, PartSpec};
use crate::{CodecFormat, NativeUnknownRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    id: Identity,
    required: Option<Identity>,
    listed: Vec<Identity>,
}
rewrite_record!(Record, []; {id, required, listed});
#[derive(Debug, Serialize, Deserialize)]
struct Pair {
    id: Identity,
    references: [Identity; 2],
}
rewrite_record!(Pair, []; {id, references});

fn id(number: u8) -> Identity {
    format!("step:native:record#{number}")
        .try_into()
        .expect("identity")
}
fn fail(ctx: &DecodeContext<'_>, output: &mut PartOutput, source: View<'_>, built: bool) {
    let start = u64::try_from(source.start()).expect("source start");
    let end = u64::try_from(source.end()).expect("source end");
    let spec = PartSpec::new(
        PartLevel::parse("step.simple_entity_instance").expect("Appendix C"),
        vec![start..start + 3, end - 1..end],
    );
    ctx.decode_part(output, source, spec, |staged| {
        if built {
            staged.insert_native(
                ctx,
                "records",
                Record {
                    id: id(2),
                    required: None,
                    listed: vec![],
                },
            )?;
        }
        Err::<(), _>(CodecError::NotImplemented(
            "UNKNOWN field in the source entity".into(),
        ))
    })
    .expect("bounded unsupported entity");
}

#[test]
fn owned_handoff_runs_required_closure_and_list_pruning_without_an_advisory_call() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"#2=ITEM(2,UNKNOWN);", &arena, &DecodePolicy::service())
            .expect("source");
    let mut output = PartOutput::new(CodecFormat::Step);
    output
        .insert_native(
            &ctx,
            "records",
            Record {
                id: id(1),
                required: Some(id(2)),
                listed: vec![],
            },
        )
        .expect("prior required user");
    output
        .insert_native(
            &ctx,
            "records",
            Record {
                id: id(3),
                required: None,
                listed: vec![id(2), id(4)],
            },
        )
        .expect("list user");
    output
        .insert_native(
            &ctx,
            "records",
            Record {
                id: id(4),
                required: None,
                listed: vec![],
            },
        )
        .expect("independent sibling");
    fail(&ctx, &mut output, root, true);
    let finalized = output.into_parts(&ctx).expect("real resource checks");
    assert_eq!(
        finalized.dropped_records(),
        [id(1).as_str(), id(2).as_str()]
    );
    let ((_, native, _, losses), dropped) = finalized.into_parts_with_dropped();
    assert_eq!(dropped, [id(1).as_str(), id(2).as_str()]);
    let records = native
        .namespace("step")
        .expect("namespace")
        .arena_as::<Record>("records")
        .expect("actual record owner");
    assert_eq!(
        records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        [id(3).as_str(), id(4).as_str()]
    );
    assert_eq!(records[0].listed, [id(4)]);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .ends_with("dropped=[\"step:native:record#1\",\"step:native:record#2\"]"));
}

#[test]
fn fixed_list_owner_of_a_dropped_record_is_not_formed_and_drops_at_handoff() {
    // #215 §4.5 rule 1: "A record that cannot be formed without such a field
    // is not formed, and rule 2 drops the records that need it." Pair #1 holds
    // #2 in a fixed two-entry list, so it cannot be formed once #2 drops.
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"#2=ITEM(2,UNKNOWN);", &arena, &DecodePolicy::service())
            .expect("source");
    let mut output = PartOutput::new(CodecFormat::Step);
    output
        .insert_native(
            &ctx,
            "pairs",
            Pair {
                id: id(1),
                references: [id(2), id(4)],
            },
        )
        .expect("checked fixed list");
    fail(&ctx, &mut output, root, true);
    let finalized = output.into_parts(&ctx).expect("dropped, not deferred");
    let (_, native, fidelity, losses) = finalized.into_parts();
    let namespace = native.namespace("step").expect("namespace");
    assert!(namespace
        .arena_as::<Pair>("pairs")
        .expect("actual pair owner")
        .is_empty());
    assert_eq!(
        namespace
            .arena_as::<NativeUnknownRecord>("unknowns")
            .expect("part stub")
            .len(),
        1
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .ends_with("dropped=[\"step:native:record#1\",\"step:native:record#2\"]"));
}

#[test]
fn helper_only_unknown_stub_has_its_actual_typed_owner_at_handoff() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"#2=UNKNOWN();", &arena, &DecodePolicy::service())
            .expect("source");
    let mut output = PartOutput::new(CodecFormat::Step);
    fail(&ctx, &mut output, root, false);
    let finalized = output.into_parts(&ctx).expect("owner visitation");
    let (_, native, fidelity, losses) = finalized.into_parts();
    let records = native
        .namespace("step")
        .expect("namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("actual typed stub");
    assert_eq!(records.len(), 1);
    assert_eq!(
        fidelity
            .retained_record(records[0].id.as_str())
            .expect("single helper home")
            .data(),
        Some(root.window())
    );
    assert_eq!(losses.len(), 1);
}

#[test]
fn finalized_dependency_drops_remove_only_their_actual_captures_and_keep_peer_reads() {
    let bytes = b"#1=X(2);#2=X(?);#3=X(3);";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    for (number, from, to, required) in [(1, 0, 8, true), (3, 16, 24, false)] {
        let source = root.child(from, to).unwrap();
        ctx.decode_part(
            &mut output,
            source,
            PartSpec::new(
                PartLevel::parse("step.simple_entity_instance").unwrap(),
                vec![],
            ),
            |stage| {
                ctx.record(id(number).as_str(), source, || {
                    let digit = source
                        .read_slice(from + 5..from + 6)
                        .ok_or_else(|| CodecError::malformed("actual digit is absent"))?[0];
                    stage.insert_native(
                        &ctx,
                        "records",
                        Record {
                            id: id(number),
                            required: required.then(|| id(digit - b'0')),
                            listed: vec![],
                        },
                    )
                })
            },
        )
        .unwrap()
        .unwrap();
    }
    fail(&ctx, &mut output, root.child(8, 16).unwrap(), true);
    let (_, native, mut fidelity, losses, pruning) = output
        .into_parts(&ctx)
        .unwrap()
        .into_parts_with_capture_pruning();
    let records = native
        .namespace("step")
        .unwrap()
        .arena_as::<Record>("records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, id(3));
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    fidelity
        .install_pruned_record_captures(&ctx, pruning, |_, _| Ok(()))
        .unwrap();
    assert!(!fidelity.record_layout().contains_key(id(1).as_str()));
    assert!(!fidelity.record_layout().contains_key(id(2).as_str()));
    let kept = &fidelity.record_layout()[id(3).as_str()];
    assert_eq!(kept.offset(), 16);
    assert_eq!(kept.byte_len(), 8);
    assert_eq!(kept.reads()["file"], [21..22]);
    assert!(ctx.take_record_captures().is_err());
}

#[test]
fn foreign_pruning_proof_refuses_before_extracting_the_other_sessions_same_named_capture() {
    let bytes = b"#2=X(?);";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    fail(&ctx, &mut output, root, true);
    let (_, _, _, _, proof) = output
        .into_parts(&ctx)
        .unwrap()
        .into_parts_with_capture_pruning();
    drop(ctx);
    let (other, source) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    other
        .record(id(2).as_str(), source, || {
            source
                .read_slice(0..1)
                .ok_or_else(|| CodecError::malformed("source byte absent"))?;
            Ok(())
        })
        .unwrap();
    let mut fidelity = crate::SourceFidelity::default();
    let error = fidelity
        .install_pruned_record_captures(&other, proof, |_, _| Ok(()))
        .unwrap_err();
    assert!(
        matches!(error, CodecError::Malformed(message) if message == "capture pruning belongs to another decode session")
    );
    assert!(fidelity.record_layout().is_empty());
    let captures = other.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), id(2).as_str());
    assert_eq!(captures[0].reads()["file"], [0..1]);
}

#[test]
fn an_existing_part_stage_cannot_be_finalized_by_a_foreign_session() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"#2=X(?);", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    fail(&ctx, &mut output, root, true);
    let other_arena = DecodeArena::new();
    let (other, _) =
        DecodeContext::from_root_bytes(b"#2=X(?);", &other_arena, &DecodePolicy::service())
            .unwrap();
    let error = output.into_parts(&other).unwrap_err();
    assert!(
        matches!(error, CodecError::Malformed(message) if message == "part output belongs to another decode session")
    );
}

#[test]
fn pruning_install_respects_pending_ancestors_and_the_original_session_fuse() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"#3=X(3);", &arena, &DecodePolicy::service()).unwrap();
    let ancestor = ctx.read_transaction().unwrap();
    ctx.record(id(3).as_str(), source, || {
        source
            .read_slice(5..6)
            .ok_or_else(|| CodecError::malformed("source digit absent"))?;
        Ok(())
    })
    .unwrap();
    let output = PartOutput::new(CodecFormat::Step);
    let (_, _, mut fidelity, _, proof) = output
        .into_parts(&ctx)
        .unwrap()
        .into_parts_with_capture_pruning();
    assert!(fidelity
        .install_pruned_record_captures(&ctx, proof, |_, _| Ok(()))
        .is_err());
    assert!(fidelity.record_layout().is_empty());
    ancestor.commit().unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), id(3).as_str());

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"source", &arena, &policy).unwrap();
    let (_, _, mut fidelity, _, proof) = PartOutput::new(CodecFormat::Step)
        .into_parts(&ctx)
        .unwrap()
        .into_parts_with_capture_pruning();
    let refusal = ctx.charge_entities(1, "actual fused producer").unwrap_err();
    let error = fidelity
        .install_pruned_record_captures(&ctx, proof, |_, _| Ok(()))
        .unwrap_err();
    assert!(
        matches!((refusal, error), (CodecError::ResourceLimit(expected), CodecError::ResourceLimit(actual)) if actual == expected)
    );
    assert!(fidelity.record_layout().is_empty());
}

#[test]
fn dependency_pruning_keeps_same_named_capture_from_an_independent_member_batch() {
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(
        b"#1=X(1);#1=X(2);#2=X(?);",
        &arena,
        &DecodePolicy::service(),
    )
    .unwrap();
    let (_, kept_batch) = ctx
        .record_batch(|| {
            let extent = source.child(0, 8).expect("first actual member extent");
            ctx.record(id(1).as_str(), extent, || {
                extent
                    .read_slice(5..6)
                    .ok_or_else(|| CodecError::malformed("member digit absent"))?;
                Ok(())
            })
        })
        .unwrap();
    let (output, dropped_batch) = ctx
        .record_batch(|| {
            let mut output = PartOutput::new(CodecFormat::Step);
            let extent = source.child(8, 16).expect("second actual member extent");
            ctx.record(id(1).as_str(), extent, || {
                extent
                    .read_slice(13..14)
                    .ok_or_else(|| CodecError::malformed("member digit absent"))?;
                output.insert_native(
                    &ctx,
                    "records",
                    Record {
                        id: id(1),
                        required: Some(id(2)),
                        listed: vec![],
                    },
                )
            })?;
            fail(
                &ctx,
                &mut output,
                source.child(16, 24).expect("actual failed child extent"),
                true,
            );
            Ok(output)
        })
        .unwrap();
    let (_, native, mut fidelity, _, proof) = output
        .into_parts(&ctx)
        .unwrap()
        .into_parts_with_capture_pruning();
    assert!(native
        .namespace("step")
        .unwrap()
        .arena_as::<Record>("records")
        .unwrap()
        .is_empty());
    assert!(!proof.was_pruned(id(1).as_str()));
    assert!(!proof.was_pruned_in_batch(id(1).as_str(), Some(kept_batch)));
    assert!(proof.was_pruned_in_batch(id(1).as_str(), Some(dropped_batch)));
    let mut mapped = 0;
    fidelity
        .install_mapped_pruned_record_captures(&ctx, proof, |capture| {
            assert_eq!(capture.batch_id(), Some(kept_batch));
            assert_eq!(capture.id(), id(1).as_str());
            mapped += 1;
            capture.with_mapping(&ctx, Some("step:native:member-a#1".to_owned()), "member-a/")
        })
        .unwrap();
    assert_eq!(mapped, 1);
    let kept = &fidelity.record_layout()["step:native:member-a#1"];
    assert_eq!(kept.offset(), 0);
    assert_eq!(kept.byte_len(), 8);
    assert_eq!(kept.reads()["member-a/file"], [5..6]);
    assert!(!fidelity.record_layout().contains_key(id(1).as_str()));
}

#[test]
fn parser_scope_rejects_a_callback_replacing_its_stage_with_foreign_output() {
    use crate::source_fidelity::parts::ParserPartOutput;
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"#2=X(?);", &arena, &DecodePolicy::service()).unwrap();
    let other_arena = DecodeArena::new();
    let (other, other_source) =
        DecodeContext::from_root_bytes(b"#2=X(?);", &other_arena, &DecodePolicy::service())
            .unwrap();
    let spec = || {
        PartSpec::new(
            PartLevel::parse("step.simple_entity_instance").unwrap(),
            vec![],
        )
    };
    let mut foreign = ParserPartOutput::new(CodecFormat::Step);
    other
        .decode_parser_part(&mut foreign, other_source, spec(), || Ok(()))
        .unwrap()
        .unwrap();
    let mut local = ParserPartOutput::new(CodecFormat::Step);
    let outcome = ctx
        .decode_parser_scope(&mut local, source, spec(), |stage| {
            *stage = foreign;
            Ok(())
        })
        .unwrap();
    assert!(outcome.is_none());
    let (_, _, fidelity, losses) = local.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(fidelity.parts().len(), 1);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("part output belongs to another decode session"));
    assert!(other.take_record_captures().unwrap().is_empty());
}

#[test]
fn record_batch_staging_charges_only_the_existing_identity_index() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("actual decode session");
    let identity = id(1);
    let mut output = PartOutput::new(CodecFormat::Step);
    let ((), batch) = ctx
        .record_batch(|| {
            output.bind_context(&ctx)?;
            let before = ctx.retained_bytes_available();
            output.admit_id(&ctx, identity.as_str())?;
            let expected = identity.as_str().len() + size_of::<String>();
            assert_eq!(
                before - ctx.retained_bytes_available(),
                u64::try_from(expected).expect("original identity index storage")
            );
            Ok(())
        })
        .expect("actual record batch");
    assert_eq!(output.record_batches.len(), 1);
    assert_eq!(output.record_batches[0].0, identity.as_str());
    assert_eq!(output.record_batches[0].1, Some(batch));
}

fn required_record_stage(ctx: &DecodeContext<'_>, source: View<'_>) -> PartOutput {
    let mut output = PartOutput::new(CodecFormat::Step);
    output
        .insert_native(
            ctx,
            "records",
            Record {
                id: id(1),
                required: Some(id(2)),
                listed: vec![],
            },
        )
        .expect("actual typed dependent record");
    fail(ctx, &mut output, source, true);
    output
}

fn dependency_finalization_cost(finalize: bool) -> u64 {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"#2=X(?);", &arena, &DecodePolicy::service())
            .expect("original failed source entity");
    let mut output = required_record_stage(&ctx, source);
    let before = ctx.retained_bytes_available();
    if finalize {
        let (_, _, _, _, proof) = output
            .into_parts(&ctx)
            .expect("actual dependency finalization")
            .into_parts_with_capture_pruning();
        assert!(proof.was_pruned(id(1).as_str()));
    } else {
        let crate::source_fidelity::parts::PartDependencyPruning::Complete { dropped_records } =
            output
                .prune_dependencies(&ctx)
                .expect("same dependency closure without proof handoff");
        assert!(dropped_records
            .iter()
            .any(|record| record == id(1).as_str()));
    }
    before - ctx.retained_bytes_available()
}

#[test]
fn finalized_pruning_proof_adds_no_retained_charge_to_dependency_closure() {
    assert_eq!(
        dependency_finalization_cost(true),
        dependency_finalization_cost(false)
    );
}

#[test]
fn appending_actual_pruning_proofs_keeps_temporary_storage_uncharged() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"#2=X(?);#2=X(?);", &arena, &DecodePolicy::service())
            .expect("two independent actual member extents");
    let (mut first, first_batch) = ctx
        .record_batch(|| {
            let output =
                required_record_stage(&ctx, source.child(0, 8).expect("first source member"));
            Ok(output.into_parts(&ctx)?.into_parts_with_capture_pruning().4)
        })
        .expect("first actual finalized member");
    let (second, second_batch) = ctx
        .record_batch(|| {
            let output =
                required_record_stage(&ctx, source.child(8, 16).expect("second source member"));
            Ok(output.into_parts(&ctx)?.into_parts_with_capture_pruning().4)
        })
        .expect("second actual finalized member");
    let before = ctx.retained_bytes_available();
    first
        .append(&ctx, second)
        .expect("same-session proof append");
    assert_eq!(ctx.retained_bytes_available(), before);
    assert!(first.was_pruned_in_batch(id(1).as_str(), Some(first_batch)));
    assert!(first.was_pruned_in_batch(id(1).as_str(), Some(second_batch)));
    assert!(!first.was_pruned(id(1).as_str()));
}

#[test]
fn readmitted_identity_prunes_every_actual_own_member_batch() {
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(
        b"#1=X(1);#1=X(2);#2=X(?);",
        &arena,
        &DecodePolicy::service(),
    )
    .expect("two original member records and their failed dependency");
    let mut output = PartOutput::new(CodecFormat::Step);
    let ((), earlier_batch) = ctx
        .record_batch(|| {
            let extent = source.child(0, 8).expect("earlier actual member extent");
            ctx.record(id(1).as_str(), extent, || {
                extent
                    .read_slice(5..6)
                    .ok_or_else(|| CodecError::malformed("earlier source digit absent"))?;
                output.insert_native(
                    &ctx,
                    "records",
                    Record {
                        id: id(1),
                        required: Some(id(2)),
                        listed: vec![],
                    },
                )
            })?;
            fail(
                &ctx,
                &mut output,
                source
                    .child(16, 24)
                    .expect("actual failed dependency extent"),
                true,
            );
            Ok(())
        })
        .expect("earlier actual member batch");
    let crate::source_fidelity::parts::PartDependencyPruning::Complete { dropped_records } = output
        .prune_dependencies(&ctx)
        .expect("advisory dependency pruning before readmission");
    assert!(dropped_records
        .iter()
        .any(|record| record == id(1).as_str()));
    assert!(!output.ids.contains(id(1).as_str()));
    let ((), latest_batch) = ctx
        .record_batch(|| {
            let extent = source.child(8, 16).expect("latest actual member extent");
            ctx.record(id(1).as_str(), extent, || {
                extent
                    .read_slice(13..14)
                    .ok_or_else(|| CodecError::malformed("latest source digit absent"))?;
                output.insert_native(
                    &ctx,
                    "records",
                    Record {
                        id: id(1),
                        required: None,
                        listed: vec![],
                    },
                )
            })
        })
        .expect("same identity readmitted in a later actual batch");
    let (_, native, mut fidelity, _, proof) = output
        .into_parts(&ctx)
        .expect("final dependency closure includes the original failed home")
        .into_parts_with_capture_pruning();
    assert!(native
        .namespace("step")
        .expect("actual native namespace")
        .arena_as::<Record>("records")
        .expect("actual typed records")
        .is_empty());
    assert!(proof.was_pruned_in_batch(id(1).as_str(), Some(earlier_batch)));
    assert!(proof.was_pruned_in_batch(id(1).as_str(), Some(latest_batch)));
    assert!(!proof.was_pruned(id(1).as_str()));
    let unknowns = native
        .namespace("step")
        .expect("actual native namespace")
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("actual failed-part stub");
    let [unknown] = unknowns.as_slice() else {
        panic!("exactly one actual failed dependency home")
    };
    assert_eq!(fidelity.record_layout().len(), 1);
    assert!(fidelity.record_layout().contains_key(unknown.id.as_str()));
    assert_eq!(
        fidelity
            .retained_record(unknown.id.as_str())
            .expect("required failed source bytes")
            .data(),
        Some(source.child(16, 24).expect("actual failed source").window())
    );
    let failed_home_layouts = fidelity.record_layout().clone();
    let mut mapped = 0;
    fidelity
        .install_mapped_pruned_record_captures(&ctx, proof, |capture| {
            mapped += 1;
            Ok(capture)
        })
        .expect("all actual own dropped captures are removed");
    assert_eq!(mapped, 0);
    assert_eq!(fidelity.record_layout(), &failed_home_layouts);
    assert!(!fidelity.record_layout().contains_key(id(1).as_str()));
}

#[test]
fn required_failed_child_retires_its_actual_parent_capture_without_crediting_discarded_reads() {
    let first = b"#1=USER(#2);";
    let peer = b"#3=PEER();";
    let failed = b"#2=UNKNOWN();";
    let bytes = [first.as_slice(), peer.as_slice(), failed.as_slice()].concat();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    let number = |position| -> Result<Identity, CodecError> {
        let digit = root
            .read_slice(position..position + 1)
            .ok_or_else(|| CodecError::malformed("record digit is absent"))?[0];
        Ok(id(digit.checked_sub(b'0').ok_or_else(|| {
            CodecError::malformed("record digit is invalid")
        })?))
    };
    ctx.record(id(1).as_str(), root.child(0, first.len()).unwrap(), || {
        output.insert_native(
            &ctx,
            "records",
            Record {
                id: number(1)?,
                required: Some(number(9)?),
                listed: vec![],
            },
        )
    })
    .unwrap();
    let peer_start = first.len();
    ctx.record(
        id(3).as_str(),
        root.child(peer_start, peer_start + peer.len()).unwrap(),
        || {
            output.insert_native(
                &ctx,
                "records",
                Record {
                    id: number(peer_start + 1)?,
                    required: None,
                    listed: vec![],
                },
            )
        },
    )
    .unwrap();
    let failed_start = first.len() + peer.len();
    let extent = root.child(failed_start, bytes.len()).unwrap();
    let spec = PartSpec::new(
        PartLevel::parse("step.simple_entity_instance").unwrap(),
        vec![],
    );
    ctx.decode_part(&mut output, extent, spec, |stage| {
        ctx.record(id(2).as_str(), extent, || {
            stage.insert_native(
                &ctx,
                "records",
                Record {
                    id: number(failed_start + 1)?,
                    required: None,
                    listed: vec![],
                },
            )?;
            Err::<(), _>(CodecError::NotImplemented(
                "unknown actual child grammar".into(),
            ))
        })
    })
    .unwrap();
    let (_, native, mut fidelity, losses, pruning) = output
        .into_parts(&ctx)
        .unwrap()
        .into_parts_with_capture_pruning();
    let rows = native
        .namespace("step")
        .unwrap()
        .arena_as::<Record>("records")
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id(3));
    assert_eq!(losses.len(), 1);
    assert_eq!(fidelity.retained_records().len(), 1);
    fidelity
        .install_pruned_record_captures(&ctx, pruning, |_, _| Ok(()))
        .expect("actual dependency-pruned capture installation");
    assert_eq!(fidelity.record_layout().len(), 2);
    assert!(!fidelity.record_layout().contains_key(id(1).as_str()));
    assert!(!fidelity.record_layout().contains_key(id(2).as_str()));
    assert_eq!(
        fidelity.record_layout()[id(3).as_str()].reads()["file"],
        [u64::try_from(peer_start + 1).unwrap()..u64::try_from(peer_start + 2).unwrap()]
    );
}
