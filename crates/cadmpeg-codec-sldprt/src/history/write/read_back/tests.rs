// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureId, ParameterId};

use super::document::{differences, pair, Change, Difference, Document};
use super::{Judge, ReadBack, ReadBackClaims};
use crate::history::write::edits::{construction_edits, SourceEdits};
use crate::test_support::nist::ftc_06;

/// The source part, the part with the first real definition leaf of one
/// named feature plus one, and that feature's id.
fn edited() -> (CadIr, CadIr, FeatureId) {
    let source = ftc_06().ir().clone();
    let mut input = source.clone();
    for feature in &mut input.model.features {
        if feature.name.is_none() {
            continue;
        }
        let mut wire = serde_json::to_value(feature.evaluation.definition()).unwrap();
        let Some(leaf) = first_real(&mut wire) else {
            continue;
        };
        *leaf = serde_json::json!(leaf.as_f64().unwrap() + 1.0);
        if let Ok(definition) = serde_json::from_value(wire) {
            feature.evaluation.set_definition(definition);
            let id = feature.id.clone();
            return (source, input, id);
        }
    }
    panic!("no named feature with a real definition leaf");
}

fn first_real(value: &mut serde_json::Value) -> Option<&mut serde_json::Value> {
    match value {
        serde_json::Value::Number(number) if number.is_f64() => Some(value),
        serde_json::Value::Array(items) => items.iter_mut().find_map(first_real),
        serde_json::Value::Object(fields) => fields.values_mut().find_map(first_real),
        _ => None,
    }
}

/// The write's claims for the edit of feature `id`, each edit called written
/// or not.
fn claims(source: &CadIr, input: &CadIr, id: &FeatureId, written: bool) -> ReadBackClaims {
    let mut edits = SourceEdits::new(input, None, Some(source), &[]).unwrap();
    let find = |ir: &CadIr| {
        ir.model
            .features
            .iter()
            .find(|feature| &feature.id == id)
            .unwrap()
            .clone()
    };
    let (original, current) = (find(source), find(input));
    edits.claim(
        &original,
        &current,
        construction_edits(&original, &current)
            .into_iter()
            .map(|edit| (edit, written)),
        false,
    );
    edits.into_parts().claims
}

fn judge(source: &CadIr, input: &CadIr, claims: &ReadBackClaims, written: &CadIr) -> ReadBack {
    let mut read_back = ReadBack::of_claims(&claims.features, Some(written));
    Judge::new(input, &|_| None).judge_part(
        &mut read_back,
        claims,
        (source, &[]),
        written,
        &mut |_| Vec::new(),
    );
    read_back
}

/// A feature other than `id` with a name.
fn other<'a>(ir: &'a mut CadIr, id: &FeatureId) -> &'a mut cadmpeg_ir::features::Feature {
    ir.model
        .features
        .iter_mut()
        .find(|feature| &feature.id != id && feature.name.is_some())
        .unwrap()
}

#[test]
fn a_part_that_reads_back_as_the_document_withholds_nothing() {
    let (source, input, id) = edited();
    let claims = claims(&source, &input, &id, true);

    let read_back = judge(&source, &input, &claims, &input);

    assert!(read_back.withhold.is_empty(), "{:?}", read_back.withhold);
    assert!(read_back.changed.is_empty(), "{:?}", read_back.changed);
}

/// A feature no edit touches that reads back changed is unexplained: the
/// write withholds the written edit that changes it.
#[test]
fn a_change_of_an_unedited_feature_is_unexplained() {
    let (source, input, id) = edited();
    let claims = claims(&source, &input, &id, true);
    let mut written = input.clone();
    let sibling = other(&mut written, &id);
    sibling.suppressed = Some(!sibling.suppressed.unwrap_or(false));

    let read_back = judge(&source, &input, &claims, &written);

    assert!(read_back.withhold.is_empty(), "{:?}", read_back.withhold);
    assert_eq!(
        read_back.unexplained.len(),
        1,
        "{:?}",
        read_back.unexplained
    );
    assert!(
        read_back.unexplained[0].ends_with("at /suppressed"),
        "{}",
        read_back.unexplained[0]
    );
}

/// A written edit that states a value under a parameter the record did not
/// hold states that value twice.
#[test]
fn a_written_edit_that_adds_a_parameter_is_withheld() {
    let (source, input, id) = edited();
    let claims = claims(&source, &input, &id, true);
    let mut written = input.clone();
    let mut added = written.model.parameters[0].clone();
    added.id = ParameterId::mint("sldprt:test:added#Depth").expect("identity grammar");
    added.name = "Depth".into();
    added.owner = Some(id.clone());
    written.model.parameters.push(added);

    let read_back = judge(&source, &input, &claims, &written);

    assert_eq!(read_back.withhold.len(), 1, "{:?}", read_back.withhold);
    assert!(
        read_back.withhold[0]
            .1
            .contains("its parameter Depth added"),
        "{}",
        read_back.withhold[0].1
    );
}

/// With every edit kept as stored, a change that remains is unexplained:
/// no written edit is left to hold back, and the write names it.
#[test]
fn a_change_with_no_written_edit_left_is_unexplained() {
    let (source, input, id) = edited();
    let claims = claims(&source, &input, &id, false);
    let mut written = input.clone();
    let sibling = other(&mut written, &id);
    sibling.suppressed = Some(!sibling.suppressed.unwrap_or(false));

    let read_back = judge(&source, &input, &claims, &written);

    assert!(read_back.withhold.is_empty(), "{:?}", read_back.withhold);
    assert_eq!(
        read_back.unexplained.len(),
        1,
        "{:?}",
        read_back.unexplained
    );
}

/// `ir` with every id that names block `from` naming block `to`, as a write
/// that moves the block re-lays it.
fn moved(ir: &CadIr, from: &str, to: &str) -> CadIr {
    let text = serde_json::to_string(ir)
        .unwrap()
        .replace(&format!("#{from}:"), &format!("#{to}:"));
    serde_json::from_str(&text).unwrap()
}

/// The block that holds the feature history: the offset in the id of the
/// first feature.
fn history_block(ir: &CadIr) -> String {
    let id = ir.model.features[0].id.as_str();
    id[id.find('#').unwrap() + 1..]
        .split(':')
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn records_pair_across_moved_block_offsets() {
    let source = ftc_06().ir().clone();
    let block = history_block(&source);
    let written = moved(&source, &block, "1");
    let (stated, read) = (
        Document::of(&source).unwrap(),
        Document::of(&written).unwrap(),
    );

    let pairing = pair(&stated, &read);
    let found = differences(&stated, &read, &pairing);

    assert!(
        found.is_empty(),
        "{:?}",
        found.iter().map(Difference::label).collect::<Vec<_>>()
    );
}

/// A sketch that reads back moved is a difference of that sketch, whatever
/// its id.
#[test]
fn a_moved_sketch_is_a_difference_of_that_sketch() {
    let source = ftc_06().ir().clone();
    let block = history_block(&source);
    let mut written = moved(&source, &block, "1");
    let pointer = "/placement/frame/origin/z";
    let index = written
        .model
        .sketches
        .iter()
        .position(|sketch| {
            serde_json::to_value(sketch)
                .unwrap()
                .pointer(pointer)
                .is_some()
        })
        .expect("a planar sketch states its origin");
    let mut wire = serde_json::to_value(&written.model.sketches[index]).unwrap();
    let origin = wire.pointer_mut(pointer).unwrap();
    *origin = serde_json::json!(origin.as_f64().unwrap() + 1.0);
    written.model.sketches[index] = serde_json::from_value(wire).unwrap();
    let (stated, read) = (
        Document::of(&source).unwrap(),
        Document::of(&written).unwrap(),
    );

    let pairing = pair(&stated, &read);
    let found = differences(&stated, &read, &pairing);

    assert_eq!(
        found.len(),
        1,
        "{:?}",
        found.iter().map(Difference::label).collect::<Vec<_>>()
    );
    assert_eq!(found[0].arena, "model.sketches");
    assert_eq!(
        found[0].id.as_deref(),
        Some(source.model.sketches[index].id.as_str())
    );
    assert!(matches!(
        &found[0].change,
        Change::Leaf { pointer: at, .. } if at == pointer
    ));
}
