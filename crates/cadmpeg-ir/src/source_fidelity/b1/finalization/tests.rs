// SPDX-License-Identifier: Apache-2.0
//! Exact-span loss attribution, source closure and atomic classification.

use super::*;
use crate::document::ArenaClass;
use crate::ids::UnknownId;
use crate::source_fidelity::RetainedSourceRecord;
use crate::CadIr;
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodePolicy, View};
use serde_json::json;

const MODEL: &str = "test:model:product#A";
const DESIGN: &str = "test:design:configuration#A";

fn document() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model
        .product_definitions
        .push(serde_json::from_value(json!({"id":MODEL,"kind":"part"})).unwrap());
    ir.model
        .configurations
        .push(serde_json::from_value(json!({"id":DESIGN})).unwrap());
    ir
}

fn fidelity(ctx: &DecodeContext<'_>, root: View<'_>) -> SourceFidelity {
    ctx.record(MODEL, root.child(0, 6).unwrap(), || {
        assert_eq!(root.read_at(0, 2), Some(&b"ab"[..]));
        Ok(())
    })
    .unwrap();
    ctx.record(DESIGN, root.child(6, 12).unwrap(), || {
        assert_eq!(root.read_at(9, 1), Some(&b"j"[..]));
        Ok(())
    })
    .unwrap();
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(ctx, |_, _| Ok(()))
        .unwrap();
    fidelity
}

fn partition(ctx: &DecodeContext<'_>, fidelity: &SourceFidelity) -> StreamPartition {
    super::super::partition_stream(
        ctx,
        "file",
        12,
        fidelity.record_layout(),
        &[2..3, 6..7],
        &[10..12],
        &[],
    )
    .unwrap()
}

fn loss(stream: &str, offset: u64, byte_len: u64) -> LossNote {
    DecodeLossCode::SidecarStale
        .note("existing codec loss")
        .with_provenance(
            SourceProvenance::in_stream(
                "synthetic",
                StreamName::try_from(stream.to_owned()).unwrap(),
                offset,
            )
            .with_byte_len(byte_len)
            .unwrap(),
        )
}

#[test]
fn finalization_derives_both_classes_and_exact_warning_extents_without_a_source_image() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdefghijkl", &arena, &DecodePolicy::service()).unwrap();
    let ir = document();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    let mut fidelity = fidelity(&ctx, root);
    let partitions = [partition(&ctx, &fidelity)];
    let mut losses = Vec::new();
    fidelity
        .finalize_b1(&ctx, &index, "synthetic", &partitions, &mut losses)
        .unwrap();
    assert_eq!(
        fidelity.unrepresented(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
    assert!(fidelity.retained_records().is_empty());
    let spans = fidelity.b1_spans()["file"].spans();
    assert_eq!(spans[0].range(), 3..6);
    assert_eq!(spans[0].classes(), ArenaClasses::from([ArenaClass::Model]));
    assert_eq!(spans[1].range(), 7..9);
    assert_eq!(spans[1].classes(), ArenaClasses::from([ArenaClass::Design]));
    assert_eq!(losses.len(), 2);
    for (note, (offset, byte_len)) in losses.iter().zip([(3, 3), (7, 2)]) {
        assert_eq!(note.code.namespace(), "decode");
        assert_eq!(note.code.local_code(), "source.bytes-undecoded");
        assert_eq!(note.severity, crate::report::Severity::Warning);
        let provenance = note.provenance.as_ref().unwrap();
        assert_eq!(provenance.format(), "synthetic");
        assert_eq!(provenance.stream_name(), Some("file"));
        assert_eq!(provenance.offset, offset);
        assert_eq!(provenance.byte_len(), Some(byte_len));
    }
    fidelity
        .check_b1_closure(&ctx, &index, "synthetic", &partitions, &losses)
        .unwrap();
    let before = fidelity.clone();
    fidelity
        .finalize_b1(&ctx, &index, "synthetic", &partitions, &mut losses)
        .unwrap();
    assert_eq!(fidelity, before);
    assert_eq!(losses.len(), 2);
    let wire = serde_json::to_value(&fidelity).unwrap();
    assert_eq!(wire["b1_spans"]["file"]["byte_len"], 12);
    assert_eq!(
        serde_json::from_value::<SourceFidelity>(wire).unwrap(),
        fidelity
    );
}

#[test]
fn only_an_exact_named_span_suppresses_the_generic_warning() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdefghijkl", &arena, &DecodePolicy::service()).unwrap();
    let ir = document();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    let baseline = fidelity(&ctx, root);
    let partitions = [partition(&ctx, &baseline)];
    let controls = [
        loss("file", 3, 3),
        loss("file", 0, 12),
        loss("file", 3, 4),
        loss("other", 3, 3),
        DecodeLossCode::SidecarStale.note("file offset3 length3"),
        DecodeLossCode::SidecarStale
            .note("point only")
            .with_provenance(SourceProvenance::in_stream(
                "synthetic",
                crate::stream_name!("file"),
                3,
            )),
        loss("file", 3, 3).with_provenance(
            SourceProvenance::in_stream("other-format", crate::stream_name!("file"), 3)
                .with_byte_len(3)
                .unwrap(),
        ),
        DecodeLossCode::SidecarStale
            .note("unnamed root")
            .with_provenance(
                SourceProvenance::root("synthetic", 3)
                    .with_byte_len(3)
                    .unwrap(),
            ),
    ];
    for (i, existing) in controls.into_iter().enumerate() {
        let mut fidelity = baseline.clone();
        let mut losses = vec![existing.clone()];
        fidelity
            .finalize_b1(&ctx, &index, "synthetic", &partitions, &mut losses)
            .unwrap();
        assert_eq!(losses[0], existing);
        assert_eq!(losses.len(), if i == 0 { 2 } else { 3 }, "control{i}");
        fidelity
            .check_b1_closure(&ctx, &index, "synthetic", &partitions, &losses)
            .unwrap();
    }
}

#[test]
fn missing_exact_loss_fails_closure_without_message_or_containment_fallback() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdefghijkl", &arena, &DecodePolicy::service()).unwrap();
    let ir = document();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    let mut fidelity = fidelity(&ctx, root);
    let partitions = [partition(&ctx, &fidelity)];
    let mut losses = vec![loss("file", 0, 12)];
    fidelity
        .finalize_b1(&ctx, &index, "synthetic", &partitions, &mut losses)
        .unwrap();
    losses.remove(1);
    assert!(fidelity
        .check_b1_closure(&ctx, &index, "synthetic", &partitions, &losses)
        .unwrap_err()
        .to_string()
        .contains("exact loss provenance"));
}

#[test]
fn invalid_later_stream_leaves_prior_classes_images_and_losses_unchanged() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdefghijkl", &arena, &DecodePolicy::service()).unwrap();
    ctx.register_named_slice(root, ByteRange { start: 0, end: 4 }, "member")
        .unwrap();
    let ir = document();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    let mut fidelity = fidelity(&ctx, root);
    fidelity
        .insert_retained_record(
            UnknownId::mint("test:source:image#file").unwrap(),
            RetainedSourceRecord::whole("file", b"abcdefghijkl".to_vec()),
        )
        .unwrap();
    let good = partition(&ctx, &fidelity);
    let bad =
        super::super::partition_stream(&ctx, "member", 5, &BTreeMap::new(), &[], &[], &[]).unwrap();
    let before = fidelity.clone();
    let mut losses = vec![loss("file", 0, 12)];
    let before_losses = losses.clone();
    assert!(fidelity
        .finalize_b1(&ctx, &index, "synthetic", &[good, bad], &mut losses)
        .is_err());
    assert_eq!(fidelity, before);
    assert_eq!(losses, before_losses);
}

#[test]
fn source_image_classes_equal_the_b1_union_inside_their_actual_extent() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdefghijkl", &arena, &DecodePolicy::service()).unwrap();
    let ir = document();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    let mut fidelity = fidelity(&ctx, root);
    fidelity
        .insert_retained_record(
            UnknownId::mint("test:source:image#whole").unwrap(),
            RetainedSourceRecord::whole("file", b"abcdefghijkl".to_vec()),
        )
        .unwrap();
    fidelity
        .insert_retained_record(
            UnknownId::mint("test:source:image#design").unwrap(),
            RetainedSourceRecord::from_bytes(
                "file",
                6,
                crate::RetainedBytes::Inline {
                    data: b"ghijkl".to_vec(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    let partitions = [partition(&ctx, &fidelity)];
    let mut losses = Vec::new();
    fidelity
        .finalize_b1(&ctx, &index, "synthetic", &partitions, &mut losses)
        .unwrap();
    assert_eq!(
        fidelity
            .retained_record("test:source:image#whole")
            .unwrap()
            .untyped(),
        &ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
    assert_eq!(
        fidelity
            .retained_record("test:source:image#design")
            .unwrap()
            .untyped(),
        &ArenaClasses::from([ArenaClass::Design])
    );
    fidelity
        .check_b1_closure(&ctx, &index, "synthetic", &partitions, &losses)
        .unwrap();
}

#[test]
fn strict_b1_wire_rejects_empty_overflow_adjacent_and_out_of_domain_spans() {
    let valid = json!({"byte_len":12,"spans":[{"offset":3,"byte_len":3,"classes":["model"]}]});
    let _: ClassifiedStream = serde_json::from_value(valid.clone()).unwrap();
    for invalid in [
        json!({"byte_len":12,"spans":[{"offset":3,"byte_len":0,"classes":["model"]}]}),
        json!({"byte_len":12,"spans":[{"offset":3,"byte_len":3,"classes":[]}]}),
        json!({"byte_len":12,"spans":[{"offset":u64::MAX,"byte_len":1,"classes":["model"]}]}),
        json!({"byte_len":12,"spans":[{"offset":10,"byte_len":3,"classes":["model"]}]}),
        json!({"byte_len":12,"spans":[{"offset":3,"byte_len":3,"classes":["model"]},{"offset":6,"byte_len":1,"classes":["design"]}]}),
        json!({"byte_len":12,"spans":[],"arbitrary":true}),
        json!({"byte_len":12,"spans":[{"offset":3,"byte_len":3,"classes":["model"],"mask":1}]}),
    ] {
        assert!(serde_json::from_value::<ClassifiedStream>(invalid).is_err());
    }
    let wire = serde_json::to_value(SourceFidelity::default()).unwrap();
    let mut wrong_union = wire;
    wrong_union["b1_spans"] = json!({"file":valid});
    assert!(serde_json::from_value::<SourceFidelity>(wrong_union).is_err());
}

#[test]
fn exact_provenance_length_is_optional_strict_and_overflow_checked() {
    let old = SourceProvenance::in_stream("synthetic", crate::stream_name!("file"), 3);
    assert_eq!(
        serde_json::to_value(&old).unwrap(),
        json!({"format":"synthetic","stream":"file","offset":3})
    );
    let exact = old.clone().with_byte_len(3).unwrap();
    let wire = json!({"format":"synthetic","stream":"file","offset":3,"byte_len":3});
    assert_eq!(serde_json::to_value(&exact).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SourceProvenance>(wire).unwrap(),
        exact
    );
    assert!(old.with_byte_len(0).is_err());
    for invalid in [
        json!({"format":"synthetic","stream":"file","offset":3,"byte_len":null}),
        json!({"format":"synthetic","stream":"file","offset":3,"byte_len":0}),
        json!({"format":"synthetic","stream":"file","offset":u64::MAX,"byte_len":1}),
    ] {
        assert!(serde_json::from_value::<SourceProvenance>(invalid).is_err());
    }
}

#[test]
fn retained_budget_refusal_cannot_publish_partial_coverage_or_loss_additions() {
    let arena = DecodeArena::new();
    let (source_ctx, _) =
        DecodeContext::from_root_bytes(b"abcd", &arena, &DecodePolicy::service()).unwrap();
    let partition =
        super::super::partition_stream(&source_ctx, "file", 4, &BTreeMap::new(), &[], &[], &[])
            .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let blocked_arena = DecodeArena::new();
    let (blocked, _) = DecodeContext::from_root_bytes(b"abcd", &blocked_arena, &policy).unwrap();
    let ir = CadIr::empty();
    let index = ClassificationIndex::new(&blocked, &ir).unwrap();
    let mut fidelity = SourceFidelity::default();
    let mut losses = vec![DecodeLossCode::SidecarStale.note("preserve this note")];
    let before = fidelity.clone();
    let before_losses = losses.clone();
    assert!(matches!(
        fidelity.finalize_b1(&blocked, &index, "synthetic", &[partition], &mut losses),
        Err(CodecError::ResourceLimit(_))
    ));
    assert_eq!(fidelity, before);
    assert_eq!(losses, before_losses);
}

#[test]
fn classified_stream_append_collision_is_atomic_and_duplicate_wire_keys_are_rejected() {
    let wire = json!({"unrepresented":["model"],"annotations":{},"b1_spans":{"file":{"byte_len":8,"spans":[{"offset":3,"byte_len":2,"classes":["model"]}]}}});
    let mut left: SourceFidelity = serde_json::from_value(wire.clone()).unwrap();
    let right: SourceFidelity = serde_json::from_value(wire).unwrap();
    let before = left.clone();
    assert!(left.append(right).is_err());
    assert_eq!(left, before);
    assert!(serde_json::from_str::<SourceFidelity>(r#"{"unrepresented":[],"b1_spans":{"file":{"byte_len":1,"spans":[]},"file":{"byte_len":1,"spans":[]}}}"#).is_err());
}

#[test]
fn composition_moves_only_the_exact_owner_classified_map_admitted_by_its_proof() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdefghijkl", &arena, &DecodePolicy::service()).unwrap();
    let ir = document();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    let mut fidelity = fidelity(&ctx, root);
    let partitions = [partition(&ctx, &fidelity)];
    fidelity
        .finalize_b1(&ctx, &index, "synthetic", &partitions, &mut Vec::new())
        .unwrap();
    let original = fidelity.b1_spans()["file"].clone();
    let proof = fidelity
        .rescope_part_tree(&ctx, crate::CodecFormat::F3d, "f3d:xref/A/")
        .unwrap();
    let (_, _, _, _, _, coverage) = fidelity.into_parts();
    let mut destination = SourceFidelity::default();
    proof
        .append_b1_coverage(&mut destination, coverage)
        .unwrap();
    assert_eq!(destination.b1_spans().len(), 1);
    assert_eq!(destination.b1_spans()["f3d:xref/A/file"], original);
    assert_eq!(
        destination.unrepresented(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
    let mut source: SourceFidelity = serde_json::from_value(json!({
        "unrepresented":["model"],"b1_spans":{"file":{"byte_len":12,"spans":[{"offset":3,"byte_len":3,"classes":["model"]}]}}
    })).unwrap();
    let proof = source
        .rescope_part_tree(&ctx, crate::CodecFormat::F3d, "f3d:xref/B/")
        .unwrap();
    let mut forged = source.b1_spans().clone();
    forged.insert(
        "file".into(),
        serde_json::from_value(
            json!({"byte_len":12,"spans":[{"offset":3,"byte_len":3,"classes":["design"]}]}),
        )
        .unwrap(),
    );
    let before = destination.clone();
    assert!(proof.append_b1_coverage(&mut destination, forged).is_err());
    assert_eq!(destination, before);
}

#[test]
fn thousand_disjoint_spans_keep_exact_loss_keys_duplicates_and_image_classes() {
    let bytes = vec![0_u8; 2000];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let ir = document();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    let mut fidelity = SourceFidelity::default();
    for (id, start, end) in [(MODEL, 0, 1000), (DESIGN, 1000, 2000)] {
        ctx.record(id, root.child(start, end).unwrap(), || Ok(()))
            .unwrap();
    }
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    let framing = (0..1000).map(|i| (2 * i)..(2 * i + 1)).collect::<Vec<_>>();
    let partition = super::super::partition_stream(
        &ctx,
        "file",
        2000,
        fidelity.record_layout(),
        &framing,
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(partition.b1().len(), 1000);
    let mut losses = Vec::new();
    for i in 0..1000 {
        let offset = 2 * i + 1;
        losses.push(if i % 2 == 0 {
            loss("file", offset, 1)
        } else if i % 4 == 1 {
            loss("file", offset - 1, 2)
        } else {
            loss("other", offset, 1)
        });
        if i % 200 == 0 {
            losses.push(loss("file", offset, 1));
        }
        fidelity
            .insert_retained_record(
                UnknownId::mint(format!("test:source:image#{i}")).unwrap(),
                RetainedSourceRecord::from_bytes(
                    "file",
                    offset,
                    crate::RetainedBytes::Inline { data: vec![0] },
                )
                .unwrap(),
            )
            .unwrap();
    }
    losses.reverse();
    let original_losses = losses.clone();
    fidelity
        .finalize_b1(
            &ctx,
            &index,
            "synthetic",
            std::slice::from_ref(&partition),
            &mut losses,
        )
        .unwrap();
    assert_eq!(&losses[..original_losses.len()], original_losses.as_slice());
    let generic = &losses[original_losses.len()..];
    assert_eq!(generic.len(), 500);
    assert_eq!(
        generic
            .iter()
            .map(|note| note.provenance.as_ref().unwrap().offset)
            .collect::<Vec<_>>(),
        (0..1000)
            .filter(|i| i % 2 == 1)
            .map(|i| 2 * i + 1)
            .collect::<Vec<_>>()
    );
    for note in generic {
        assert_eq!(
            note.code,
            DecodeLossCode::SourceBytesUndecoded.note("").code
        );
        let provenance = note.provenance.as_ref().unwrap();
        assert_eq!(provenance.stream_name(), Some("file"));
        assert_eq!(provenance.byte_len(), Some(1));
    }
    for i in 0..1000 {
        let id = format!("test:source:image#{i}");
        assert_eq!(
            fidelity.retained_record(&id).unwrap().untyped(),
            &ArenaClasses::from([if i < 500 {
                ArenaClass::Model
            } else {
                ArenaClass::Design
            }])
        );
    }
    fidelity
        .check_b1_closure(&ctx, &index, "synthetic", &[partition], &losses)
        .unwrap();
}

#[test]
fn unsorted_actual_stream_names_and_partition_order_keep_exact_domains() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::service()).unwrap();
    ctx.register_named_slice(root, ByteRange { start: 1, end: 2 }, "z-member")
        .unwrap();
    ctx.register_named_slice(root, ByteRange { start: 0, end: 1 }, "a-member")
        .unwrap();
    let ir = CadIr::empty();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    let mut fidelity = SourceFidelity::default();
    let partitions = [
        super::super::partition_stream(
            &ctx,
            "z-member",
            1,
            fidelity.record_layout(),
            &[],
            &[],
            &[],
        )
        .unwrap(),
        super::super::partition_stream(&ctx, "file", 3, fidelity.record_layout(), &[], &[], &[])
            .unwrap(),
        super::super::partition_stream(
            &ctx,
            "a-member",
            1,
            fidelity.record_layout(),
            &[],
            &[],
            &[],
        )
        .unwrap(),
    ];
    let mut losses = Vec::new();
    fidelity
        .finalize_b1(&ctx, &index, "synthetic", &partitions, &mut losses)
        .unwrap();
    assert_eq!(losses.len(), 3);
    for (loss, (stream, byte_len)) in
        losses
            .iter()
            .zip([("z-member", 1), ("file", 3), ("a-member", 1)])
    {
        let source = loss.provenance.as_ref().unwrap();
        assert_eq!(source.stream_name(), Some(stream));
        assert_eq!(source.offset, 0);
        assert_eq!(source.byte_len(), Some(byte_len));
    }
    fidelity
        .check_b1_closure(&ctx, &index, "synthetic", &partitions, &losses)
        .unwrap();
}

#[test]
fn a_given_up_part_is_named_only_by_its_record_unreadable_loss() {
    // #215 §4.5 rule 3: "No other loss names a given-up part: its
    // `record.unreadable` loss alone names a span inside it." Records read
    // [0,2) and [6,10); the part [2,6) gives up. Its framing byte [4,5) splits
    // the part's B1 bytes into [2,4) and [5,6), so no B1 span equals the part.
    use crate::provenance::CodecFormat;
    use crate::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"abcdefghij", &arena, &DecodePolicy::service()).unwrap();
    for (id, start, end) in [(MODEL, 0, 2), (DESIGN, 6, 10)] {
        ctx.record(id, root.child(start, end).unwrap(), || {
            assert!(root.read_at(start, end - start).is_some());
            Ok(())
        })
        .unwrap();
    }
    let mut output = PartOutput::new(CodecFormat::Step);
    let spec = PartSpec::new(
        PartLevel::parse("step.simple_entity_instance").unwrap(),
        vec![4..5],
    );
    let given_up = ctx
        .decode_part(&mut output, root.child(2, 6).unwrap(), spec, |_| {
            Err::<(), _>(CodecError::malformed("damaged entity"))
        })
        .unwrap();
    assert_eq!(given_up, None);
    let mut fidelity = output.fidelity().clone();
    let mut losses = output.losses().to_vec();
    assert_eq!(losses.len(), 1);
    let unreadable = losses[0].provenance.as_ref().unwrap();
    assert_eq!(
        (unreadable.offset, unreadable.byte_len()),
        (2, Some(4)),
        "the loss gives the part's stream, offset and length (#217 §5.6)"
    );
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    let partitions = [super::super::partition_stream(
        &ctx,
        "file",
        10,
        fidelity.record_layout(),
        &[4..5],
        &[],
        &[2..6],
    )
    .unwrap()];
    assert_eq!(partitions[0].b1(), &[2..4, 5..6]);
    let ir = document();
    let index = ClassificationIndex::new(&ctx, &ir).unwrap();
    fidelity
        .finalize_b1(&ctx, &index, "step", &partitions, &mut losses)
        .unwrap();
    assert_eq!(losses.len(), 1, "no source.bytes-undecoded for the part");
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    fidelity
        .check_b1_closure(&ctx, &index, "step", &partitions, &losses)
        .unwrap();
}
