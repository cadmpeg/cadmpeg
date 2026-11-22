// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureId, ParameterId};

use super::{canonical_text, ReadBack, ReadBackClaims};
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

fn judge(input: &CadIr, claims: &ReadBackClaims, written: &CadIr) -> ReadBack {
    let mut read_back = ReadBack::of_claims(&claims.features, Some(written));
    read_back.judge_part(input, claims, written);
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

    let read_back = judge(&input, &claims, &input);

    assert!(read_back.withhold.is_empty(), "{:?}", read_back.withhold);
    assert!(read_back.changed.is_empty(), "{:?}", read_back.changed);
}

/// A sibling plane that reads back as another kind, or a dependent that
/// moves, is a record no edit touches.
#[test]
fn a_written_edit_that_changes_an_unedited_feature_is_withheld() {
    let (source, input, id) = edited();
    let claims = claims(&source, &input, &id, true);
    let mut written = input.clone();
    let sibling = other(&mut written, &id);
    sibling.suppressed = Some(!sibling.suppressed.unwrap_or(false));

    let read_back = judge(&input, &claims, &written);

    assert_eq!(read_back.withhold.len(), 1, "{:?}", read_back.withhold);
    assert_eq!(read_back.withhold[0].0, id);
    assert!(
        read_back.withhold[0].1.contains("which no edit touches"),
        "{}",
        read_back.withhold[0].1
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

    let read_back = judge(&input, &claims, &written);

    assert_eq!(read_back.withhold.len(), 1, "{:?}", read_back.withhold);
    assert!(
        read_back.withhold[0]
            .1
            .contains("its parameter Depth added"),
        "{}",
        read_back.withhold[0].1
    );
}

/// With every edit kept as stored, a change that remains is named.
#[test]
fn a_change_with_no_written_edit_left_is_named() {
    let (source, input, id) = edited();
    let claims = claims(&source, &input, &id, false);
    let mut written = input.clone();
    let sibling = other(&mut written, &id);
    sibling.suppressed = Some(!sibling.suppressed.unwrap_or(false));

    let read_back = judge(&input, &claims, &written);

    assert!(read_back.withhold.is_empty(), "{:?}", read_back.withhold);
    assert_eq!(read_back.changed.len(), 1, "{:?}", read_back.changed);
}

#[test]
fn a_native_token_numbers_its_offsets_by_first_appearance() {
    let names = std::collections::HashMap::new();

    assert_eq!(
        canonical_text("sldprt:face:1234:5,6789:1234".into(), &names),
        "sldprt:face:N0:5,N1:N0"
    );
    assert_eq!(
        canonical_text("sldprt:face:1234,5678".into(), &names),
        "sldprt:face:1234,N0"
    );
}
