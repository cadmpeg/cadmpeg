// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeDepth, DecodePolicy, ResourceDimension};
use serde_json::{json, Value};

use super::super::provenance::NativeOwnership;
use super::*;
use crate::ids::{Identity, UnknownId};
use crate::native::NativeRecord;
use crate::source_fidelity::{RecordLayout, RetainedBytes, RetainedSourceRecord};
use crate::unknown::NativeUnknownRecord;

const MODEL_ID: &str = "test:model:product#0";
const DESIGN_ID: &str = "test:design:configuration#0";
const NATIVE_MODEL: &str = "test:source:record#model";
const NATIVE_DESIGN: &str = "test:source:record#design";

fn document() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(
        serde_json::from_value(json!({
            "id":MODEL_ID,"kind":"part","native_ref":NATIVE_MODEL
        }))
        .expect("valid product"),
    );
    ir.model.configurations.push(
        serde_json::from_value(json!({
            "id":DESIGN_ID,"native_ref":NATIVE_DESIGN
        }))
        .expect("valid design configuration"),
    );
    ir
}

fn layout(fidelity: &mut SourceFidelity, id: &str, start: u64, end: u64) {
    fidelity
        .insert_record_layout(
            id.into(),
            serde_json::from_value::<RecordLayout>(json!({
                "stream":"file","offset":start,"byte_len":end-start,
                "reads":{},"pointers":{},"payloads":{}
            }))
            .expect("valid record extent"),
        )
        .expect("distinct record layout");
}

fn native(ir: &mut CadIr, id: &str, fields: Value) {
    let Value::Object(fields) = fields else {
        panic!("native fields must be an object")
    };
    let record = NativeRecord::new(Identity::new(id).expect("valid native identity"), fields)
        .expect("valid native fields");
    ir.native
        .namespace_mut("test")
        .arenas_mut()
        .entry("objects".into())
        .or_default()
        .push(record);
}

struct FixtureOwners;
impl NativeOwnerResolver for FixtureOwners {
    fn visit_owners(
        &self,
        _: &DecodeContext<'_>,
        ir: &CadIr,
        visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for namespace in ir.native.0.values() {
            for (arena, records) in namespace.arenas() {
                if arena == "unknowns" {
                    continue;
                }
                for record in records {
                    let owner = serde_json::from_value::<Option<Identity>>(
                        record.field("owner").unwrap_or(Value::Null),
                    )
                    .expect("typed fixture owner");
                    visitor(NativeOwnership::new(
                        Identity::new(record.id()).expect("fixture identity"),
                        owner,
                    ))?;
                }
            }
        }
        Ok(())
    }
}

fn classify(ir: &CadIr, fidelity: &SourceFidelity, b1: Range<u64>) -> Vec<ClassifiedSpan> {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let framing: Vec<_> = [0..b1.start, b1.end..12]
        .into_iter()
        .filter(|range| !range.is_empty())
        .collect();
    let partition = super::super::partition_stream(
        &ctx,
        "file",
        12,
        fidelity.record_layout(),
        &framing,
        &[],
        &[],
    )
    .expect("valid source partition");
    let output = ClassificationIndex::new_with_owner_resolver(&ctx, ir, &FixtureOwners)
        .expect("complete typed fixture ownership")
        .classify_partition(&ctx, fidelity, "file", &partition)
        .expect("valid owner classification");
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].range(), b1);
    output
}

#[test]
fn neutral_owner_and_native_reverse_links_use_the_registered_class() {
    let ir = document();
    for (id, class) in [
        (MODEL_ID, ArenaClass::Model),
        (DESIGN_ID, ArenaClass::Design),
        (NATIVE_MODEL, ArenaClass::Model),
        (NATIVE_DESIGN, ArenaClass::Design),
    ] {
        let mut fidelity = SourceFidelity::default();
        layout(&mut fidelity, id, 0, 12);
        assert_eq!(
            classify(&ir, &fidelity, 4..6)[0].classes(),
            ArenaClasses::from([class])
        );
    }
}

#[test]
fn innermost_owner_excludes_outer_classes_but_adds_its_direct_native_owner() {
    let mut ir = document();
    let child = "test:source:record#child";
    native(&mut ir, child, json!({"owner":NATIVE_DESIGN}));
    native(&mut ir, NATIVE_DESIGN, json!({"owner":NATIVE_MODEL}));
    native(&mut ir, NATIVE_MODEL, json!({"owner":null}));
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, NATIVE_MODEL, 0, 12);
    layout(&mut fidelity, child, 3, 8);
    assert_eq!(
        classify(&ir, &fidelity, 4..6)[0].classes(),
        ArenaClasses::from([ArenaClass::Design]),
        "the child's direct owner contributes; its grandparent does not"
    );
    assert_eq!(
        classify(&ir, &fidelity, 8..10)[0].classes(),
        ArenaClasses::from([ArenaClass::Model])
    );
}

#[test]
fn equal_innermost_extents_union_classes_independent_of_id_order() {
    let ir = document();
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, NATIVE_MODEL, 2, 9);
    layout(&mut fidelity, NATIVE_DESIGN, 2, 9);
    assert_eq!(
        classify(&ir, &fidelity, 4..6)[0].classes(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
}

#[test]
fn a_maximal_b1_span_crossing_inner_boundaries_uses_the_containing_owner() {
    let ir = document();
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, NATIVE_MODEL, 0, 12);
    layout(&mut fidelity, NATIVE_DESIGN, 3, 6);
    assert_eq!(
        classify(&ir, &fidelity, 2..8)[0].classes(),
        ArenaClasses::from([ArenaClass::Model])
    );
}

#[test]
fn empty_union_falls_back_to_both() {
    let ir = document();
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, "test:source:record#no-link", 0, 12);
    assert_eq!(
        classify(&ir, &fidelity, 4..6)[0].classes(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
}

#[test]
fn digest_only_unknown_links_contribute_on_overlap_but_not_adjacency() {
    let mut ir = document();
    let id = UnknownId::mint("test:source:unknown#0").expect("valid unknown identity");
    ir.native.namespace_mut("test").arenas_mut().insert(
        "unknowns".into(),
        vec![NativeRecord::from(&NativeUnknownRecord {
            id: id.clone(),
            links: vec![Identity::new(NATIVE_DESIGN).expect("valid link")],
        })],
    );
    let mut fidelity = SourceFidelity::default();
    fidelity
        .insert_retained_record(
            id,
            RetainedSourceRecord::from_bytes(
                "file",
                3,
                RetainedBytes::Digest {
                    byte_len: 3,
                    sha256: crate::hash::digest::Sha256Digest::digest(b"abc"),
                },
            )
            .expect("valid retained extent"),
        )
        .expect("distinct retained record");
    assert_eq!(
        classify(&ir, &fidelity, 2..8)[0].classes(),
        ArenaClasses::from([ArenaClass::Design])
    );
    assert_eq!(
        classify(&ir, &fidelity, 6..8)[0].classes(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
    assert_eq!(
        classify(&ir, &fidelity, 1..3)[0].classes(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
    layout(&mut fidelity, NATIVE_MODEL, 0, 12);
    assert_eq!(
        classify(&ir, &fidelity, 2..8)[0].classes(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design]),
        "unknown links add to the containing owner's class"
    );
    assert_eq!(
        classify(&ir, &fidelity, 6..8)[0].classes(),
        ArenaClasses::from([ArenaClass::Model]),
        "adjacent unknown links do not add classes"
    );
}

#[test]
fn equal_length_partition_for_another_stream_is_rejected() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let partition =
        super::super::partition_stream(&ctx, "other", 12, &BTreeMap::new(), &[], &[], &[])
            .expect("other stream");
    assert_eq!(partition.stream(), "other");
    let error = classify_partition(
        &ctx,
        &document(),
        &SourceFidelity::default(),
        "file",
        &partition,
    )
    .expect_err("same length does not establish stream identity");
    assert!(error.to_string().contains("different stream"));
}

#[test]
fn crossing_record_extents_classify_by_the_smallest_containing_extent() {
    // #214 §3.1: the owner is "the innermost record whose `record_layout`
    // extent contains the span". For crossing extents that is the smallest
    // containing extent; containing extents of equal length union classes.
    let ir = document();
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, NATIVE_MODEL, 0, 8);
    layout(&mut fidelity, NATIVE_DESIGN, 3, 12);
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let partition = super::super::partition_stream(
        &ctx,
        "file",
        12,
        fidelity.record_layout(),
        &[0..4, 6..12],
        &[],
        &[],
    )
    .expect("valid byte partition");
    let spans = classify_partition(&ctx, &ir, &fidelity, "file", &partition)
        .expect("crossing extents do not refuse the decode");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].range(), 4..6);
    assert_eq!(
        spans[0].classes(),
        ArenaClasses::from([ArenaClass::Model]),
        "[0,8) is shorter than [3,12)"
    );

    let mut tied = SourceFidelity::default();
    layout(&mut tied, NATIVE_MODEL, 0, 8);
    layout(&mut tied, NATIVE_DESIGN, 2, 10);
    let partition = super::super::partition_stream(
        &ctx,
        "file",
        12,
        tied.record_layout(),
        &[0..4, 6..12],
        &[],
        &[],
    )
    .expect("valid byte partition");
    let spans = classify_partition(&ctx, &ir, &tied, "file", &partition)
        .expect("crossing extents of equal length");
    assert_eq!(spans[0].range(), 4..6);
    assert_eq!(
        spans[0].classes(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design]),
        "[0,8) and [2,10) tie at length 8"
    );
}

#[test]
fn exact_output_vector_budget_boundary_is_enforced_without_entities_charge() {
    let ir = CadIr::empty();
    let fidelity = SourceFidelity::default();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let partition =
        super::super::partition_stream(&ctx, "file", 12, &BTreeMap::new(), &[], &[], &[])
            .expect("partition");
    let bytes = u64_from_index(std::mem::size_of::<ClassifiedSpan>());
    for (limit, success) in [(bytes, true), (bytes - 1, false)] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        policy.limits.max_entities = 0;
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
        let result = classify_partition(&ctx, &ir, &fidelity, "file", &partition);
        if success {
            assert_eq!(
                result.expect("exact output budget")[0].classes(),
                ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
            );
        } else {
            let CodecError::ResourceLimit(limit) = result.expect_err("one byte short") else {
                panic!("budget refusal must retain resource classification")
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.additional, bytes);
        }
    }
}

#[test]
fn a_shared_native_ref_unions_actual_model_and_design_records() {
    let mut ir = document();
    ir.model.configurations[0].native_ref = Some(NATIVE_MODEL.into());
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, NATIVE_MODEL, 0, 12);
    assert_eq!(
        classify(&ir, &fidelity, 4..6)[0].classes(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
}

#[test]
fn digest_only_unknown_extent_must_fit_its_named_stream() {
    let mut ir = document();
    let id = UnknownId::mint("test:source:unknown#outside").expect("valid identity");
    ir.native.namespace_mut("test").arenas_mut().insert(
        "unknowns".into(),
        vec![NativeRecord::from(&NativeUnknownRecord {
            id: id.clone(),
            links: vec![Identity::new(NATIVE_DESIGN).expect("valid link")],
        })],
    );
    let mut fidelity = SourceFidelity::default();
    fidelity
        .insert_retained_record(
            id,
            RetainedSourceRecord::from_bytes(
                "file",
                10,
                RetainedBytes::Digest {
                    byte_len: 3,
                    sha256: crate::hash::digest::Sha256Digest::digest(b"abc"),
                },
            )
            .expect("valid offset domain"),
        )
        .expect("distinct retained record");
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let partition =
        super::super::partition_stream(&ctx, "file", 12, &BTreeMap::new(), &[], &[], &[])
            .expect("partition");
    let error = classify_partition(&ctx, &ir, &fidelity, "file", &partition)
        .expect_err("digest source length must fit");
    assert!(error
        .to_string()
        .contains("unknown stub extent exceeds stream"));
}

#[test]
fn reusable_index_keeps_borrowed_owner_facts_across_streams() {
    let ir = document();
    let mut fidelity = SourceFidelity::default();
    layout(&mut fidelity, NATIVE_DESIGN, 0, 12);
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let index = ClassificationIndex::new(&ctx, &ir).expect("build typed index once");
    let file =
        super::super::partition_stream(&ctx, "file", 12, fidelity.record_layout(), &[], &[], &[])
            .expect("file partition");
    let other =
        super::super::partition_stream(&ctx, "other", 12, fidelity.record_layout(), &[], &[], &[])
            .expect("other partition");
    assert_eq!(
        index
            .classify_partition(&ctx, &fidelity, "file", &file)
            .expect("file owners")[0]
            .classes(),
        ArenaClasses::from([ArenaClass::Design])
    );
    assert_eq!(
        index
            .classify_partition(&ctx, &fidelity, "other", &other)
            .expect("no owners on other stream")[0]
            .classes(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
}
