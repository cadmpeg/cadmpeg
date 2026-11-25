// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cadmpeg_ir::source_fidelity::CodecDigests;
use std::collections::HashSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureId, ParameterId};

use super::document::{differences, pair, Change, Difference, Document};
use super::{Judge, ReadBack, ReadBackClaims};
use crate::history::write::edits::{construction_edits, SourceEdits, UnwrittenEdits};
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
        let mut wire =
            cadmpeg_core::serde_erase::json::to_value(feature.evaluation.definition()).unwrap();
        let Some(leaf) = first_real(&mut wire) else {
            continue;
        };
        *leaf = serde_json::json!(leaf.as_f64().unwrap() + 1.0);
        if let Ok(definition) = cadmpeg_core::serde_erase::json::from_value(wire) {
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
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut edits = SourceEdits::new(input, None, Some(source), &[], &ctx).unwrap();
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
    judge_settled(source, input, claims, written, &HashSet::new())
}

/// `judge` with the features in `settled` read back as the stored solid
/// settles their travel.
fn judge_settled(
    source: &CadIr,
    input: &CadIr,
    claims: &ReadBackClaims,
    written: &CadIr,
    settled: &HashSet<FeatureId>,
) -> ReadBack {
    Judge::new(
        input,
        &CodecDigests::new(),
        cadmpeg_core::decode::ResourceLimits::desktop(),
        &|_| None,
    )
    .judge(
        claims,
        &UnwrittenEdits::default(),
        (source, &[], &[]),
        written,
        Some(settled),
        &mut |_| Vec::new(),
    )
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
    let replace = |text: &str| text.replace(&format!("#{from}:"), &format!("#{to}:"));
    crate::test_support::readback::edited_json(
        ir,
        &mut |value| {
            *value = serde_json::from_str(&replace(&value.to_string())).unwrap();
        },
        &replace,
    )
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
            cadmpeg_core::serde_erase::json::to_value(sketch)
                .unwrap()
                .pointer(pointer)
                .is_some()
        })
        .expect("a planar sketch states its origin");
    let mut wire =
        cadmpeg_core::serde_erase::json::to_value(&written.model.sketches[index]).unwrap();
    let origin = wire.pointer_mut(pointer).unwrap();
    *origin = serde_json::json!(origin.as_f64().unwrap() + 1.0);
    written.model.sketches[index] = cadmpeg_core::serde_erase::json::from_value(wire).unwrap();
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

/// The source part, the part with the travel of its first extrusion along
/// the profile normal with no reversal flag turned against the normal and,
/// with `other`, the first real leaf of its definition plus one, and that
/// extrusion's id.
fn travel_edited(other: bool) -> (CadIr, CadIr, FeatureId) {
    let source = ftc_06().ir().clone();
    let mut input = source.clone();
    let feature = input
        .model
        .features
        .iter_mut()
        .find(|feature| {
            let wire =
                cadmpeg_core::serde_erase::json::to_value(feature.evaluation.definition()).unwrap();
            super::settled_travel(&wire) == Some("/value/value/direction")
                && wire.pointer("/value/value/direction/kind")
                    == Some(&serde_json::json!("profile_normal"))
                && !crate::history::extrude_direction::reversal_flag(&feature.source_properties)
        })
        .expect("an extrusion along its profile normal");
    let mut wire =
        cadmpeg_core::serde_erase::json::to_value(feature.evaluation.definition()).unwrap();
    *wire.pointer_mut("/value/value/direction").unwrap() =
        serde_json::json!({"kind": "reversed_profile_normal"});
    if other {
        let leaf = first_real(&mut wire).unwrap();
        *leaf = serde_json::json!(leaf.as_f64().unwrap() + 1.0);
    }
    feature
        .evaluation
        .set_definition(cadmpeg_core::serde_erase::json::from_value(wire).unwrap());
    let id = feature.id.clone();
    (source, input, id)
}

/// A written travel edit of an extrusion that the stale-solid loss names
/// reads back as the stored solid settles it: that difference is named by
/// the stale-solid loss, and the edit is carried. Without the stale-solid
/// loss the same read-back withholds the edit.
#[test]
fn a_travel_the_stored_solid_settles_is_carried_under_the_stale_solid_loss() {
    let (source, input, id) = travel_edited(false);
    let claims = claims(&source, &input, &id, true);
    let settled = HashSet::from([id.clone()]);

    let read_back = judge_settled(&source, &input, &claims, &source, &settled);

    assert!(read_back.withhold.is_empty(), "{:?}", read_back.withhold);
    assert!(
        read_back.unexplained.is_empty(),
        "{:?}",
        read_back.unexplained
    );
    assert!(read_back.changed.is_empty(), "{:?}", read_back.changed);

    let read_back = judge(&source, &input, &claims, &source);
    assert_eq!(read_back.withhold.len(), 1, "{:?}", read_back.withhold);
    assert_eq!(read_back.withhold[0].0, id);
}

/// A second edit of the same extrusion that does not read back is withheld,
/// although the stored solid settles the travel.
#[test]
fn another_edit_of_a_feature_whose_travel_the_solid_settles_is_withheld() {
    let (source, input, id) = travel_edited(true);
    let claims = claims(&source, &input, &id, true);
    let settled = HashSet::from([id.clone()]);

    let read_back = judge_settled(&source, &input, &claims, &source, &settled);

    assert_eq!(read_back.withhold.len(), 1, "{:?}", read_back.withhold);
    assert_eq!(read_back.withhold[0].0, id);
}

/// The stored solid settles the travel of an edited extrusion that shapes
/// the solid only where its reversal flag reads back as the encoder writes
/// it from the caller's feature.
#[test]
fn a_travel_whose_reversal_flag_reads_back_otherwise_is_not_settled() {
    let (source, input, id) = travel_edited(false);
    let claims = claims(&source, &input, &id, true);

    let settled_in = |written: &CadIr| {
        let pairing = pair(
            &Document::of(&input).unwrap(),
            &Document::of(written).unwrap(),
        );
        super::settled_by_solid(&input, &source, &claims, &HashSet::new(), written, &pairing)
    };
    let settled = settled_in(&source);
    assert!(settled.contains(&id), "{settled:?}");

    let mut written = source.clone();
    let feature = written
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == id)
        .unwrap();
    feature.source_properties.insert(
        cadmpeg_core::text::NonBlankString::new("Reverse".to_owned()).unwrap(),
        "true".to_owned(),
    );
    let settled = settled_in(&written);
    assert!(!settled.contains(&id), "{settled:?}");
}

/// `ir` with the first string that `matches` outside a record's own id
/// replaced as `edit` states.
fn with_string(ir: &CadIr, matches: fn(&str) -> bool, edit: fn(&str) -> String) -> CadIr {
    fn walk(
        value: &mut serde_json::Value,
        matches: fn(&str) -> bool,
        edit: fn(&str) -> String,
    ) -> bool {
        match value {
            serde_json::Value::String(text) if matches(text) => {
                *text = edit(text);
                true
            }
            serde_json::Value::Array(items) => {
                items.iter_mut().any(|item| walk(item, matches, edit))
            }
            serde_json::Value::Object(fields) => fields
                .iter_mut()
                .filter(|(key, _)| key.as_str() != "id")
                .any(|(_, item)| walk(item, matches, edit)),
            _ => false,
        }
    }
    let homes = ["geometry", "model", "design", "process"];
    let mut edited = false;
    let ir = crate::test_support::readback::edited_json(
        ir,
        &mut |value| {
            if edited {
                return;
            }
            edited = match value.as_object() {
                Some(fields) if homes.iter().any(|home| fields.contains_key(*home)) => homes
                    .iter()
                    .any(|home| walk(&mut value[*home], matches, edit)),
                _ => walk(value, matches, edit),
            };
        },
        &str::to_owned,
    );
    assert!(edited, "a string to edit");
    ir
}

/// The differences between FTC-06 and FTC-06 written at another block
/// offset with one string edited.
fn differences_with_string(matches: fn(&str) -> bool, edit: fn(&str) -> String) -> Vec<String> {
    let source = ftc_06().ir().clone();
    let block = history_block(&source);
    let written = with_string(&moved(&source, &block, "1"), matches, edit);
    let (stated, read) = (
        Document::of(&source).unwrap(),
        Document::of(&written).unwrap(),
    );
    let pairing = pair(&stated, &read);
    differences(&stated, &read, &pairing)
        .iter()
        .map(Difference::label)
        .collect()
}

/// The last run of digits of `text` plus one.
fn last_number_plus_one(text: &str) -> String {
    let start = text
        .rfind(|character: char| !character.is_ascii_digit())
        .map_or(0, |index| index + 1);
    let number = text[start..].parse::<u64>().unwrap();
    format!("{}{}", &text[..start], number + 1)
}

#[test]
fn a_changed_relation_kind_is_one_difference() {
    let found = differences_with_string(
        |text| text.starts_with("sldprt:sketch-relation:"),
        last_number_plus_one,
    );

    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn one_changed_digit_of_an_edge_selection_is_one_difference() {
    let found = differences_with_string(
        |text| text.starts_with("sldprt:feature-input:edge-selection-vectors:"),
        |text| {
            let at = text
                .find(|character: char| character.is_ascii_digit())
                .unwrap();
            let digit = text.as_bytes()[at] - b'0';
            format!("{}{}{}", &text[..at], (digit + 1) % 10, &text[at + 1..])
        },
    );

    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_changed_solver_line_index_is_one_difference() {
    let found = differences_with_string(
        |text| text.starts_with("sldprt:") && text.contains("solver-line"),
        last_number_plus_one,
    );

    assert_eq!(found.len(), 1, "{found:?}");
}

/// A leaf the caller changed that reads back as the source is a difference,
/// and the read-back names the feature's tree place as misread.
#[test]
fn a_reordered_feature_that_reads_back_in_source_order_is_a_difference() {
    let source = ftc_06().ir().clone();
    let mut input = source.clone();
    let mut named = input
        .model
        .features
        .iter_mut()
        .filter(|feature| feature.name.is_some());
    let (first, second) = (named.next().unwrap(), named.next().unwrap());
    std::mem::swap(&mut first.ordinal, &mut second.ordinal);
    let ids = [first.id.clone(), second.id.clone()];

    let read_back = judge(&source, &input, &ReadBackClaims::default(), &source);

    assert_eq!(
        read_back
            .unexplained
            .iter()
            .filter(|label| label.ends_with("at /ordinal"))
            .count(),
        2,
        "{:?}",
        read_back.unexplained
    );
    for id in ids {
        assert!(
            read_back
                .misread
                .contains(&(id.clone(), "ordinal".to_owned())),
            "{:?}",
            read_back.misread
        );
    }
}

/// A difference the write of the unedited source reads back as well is
/// named at once, by arena; holding an edit back cannot remove it.
#[test]
fn a_difference_the_unedited_write_has_is_named_by_arena() {
    let (source, input, id) = edited();
    let claims = claims(&source, &input, &id, true);
    let mut written = input.clone();
    let sibling = other(&mut written, &id);
    sibling.suppressed = Some(!sibling.suppressed.unwrap_or(false));
    let sibling = sibling.id.clone();
    let mut round_trip = source.clone();
    let twin = round_trip
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id == sibling)
        .unwrap();
    twin.suppressed = Some(!twin.suppressed.unwrap_or(false));

    let read_back = Judge::new(
        &input,
        &CodecDigests::new(),
        cadmpeg_core::decode::ResourceLimits::desktop(),
        &|_| None,
    )
    .judge(
        &claims,
        &UnwrittenEdits::default(),
        (&source, &[], &[]),
        &written,
        Some(&HashSet::new()),
        &mut |stated| {
            let read = Document::of(&round_trip).unwrap();
            differences(stated, &read, &pair(stated, &read))
        },
    );

    assert!(read_back.withhold.is_empty(), "{:?}", read_back.withhold);
    assert!(
        read_back.unexplained.is_empty(),
        "{:?}",
        read_back.unexplained
    );
    assert_eq!(read_back.defects.len(), 1, "{:?}", read_back.defects);
    assert!(
        read_back.defects[0].starts_with("model.features: 1 difference(s)")
            && read_back.defects[0].contains(sibling.as_str()),
        "{}",
        read_back.defects[0]
    );
}

/// A claimed feature is found through the pairing: a write that renumbers
/// every feature still reads its edit back.
#[test]
fn a_claimed_feature_whose_ordinal_moves_is_found_through_the_pairing() {
    let (source, input, id) = edited();
    let claims = claims(&source, &input, &id, true);
    let mut written = input.clone();
    for feature in &mut written.model.features {
        feature.ordinal += 1;
    }

    let read_back = judge(&source, &input, &claims, &written);

    assert!(read_back.withhold.is_empty(), "{:?}", read_back.withhold);
    assert!(
        read_back
            .unexplained
            .iter()
            .all(|label| label.ends_with("at /ordinal")),
        "{:?}",
        read_back.unexplained
    );
}

#[test]
fn a_record_an_edit_reaches_reads_back_with_the_edit_applied() {
    let numbers = [(152.4, 153.4)];
    let texts = [("6".to_owned(), "6.03937007874016".to_owned())];
    let agrees = |stated: serde_json::Value, written: serde_json::Value| {
        super::substitutes(&numbers, &texts, &stated, &written)
    };

    assert!(agrees(serde_json::json!(152.4), serde_json::json!(153.4)));
    assert!(agrees(
        serde_json::json!("6"),
        serde_json::json!("6.03937007874016")
    ));
    assert!(agrees(
        serde_json::json!("6[in]"),
        serde_json::json!("6.03937007874016[in]")
    ));
    assert!(agrees(
        serde_json::json!("152.4mm"),
        serde_json::json!("153.4mm")
    ));
    assert!(!agrees(serde_json::json!(152.4), serde_json::json!(160.0)));
    assert!(!agrees(serde_json::json!("6"), serde_json::json!("153.4")));
    assert!(!agrees(
        serde_json::json!("6[in]"),
        serde_json::json!("6.03937007874016[mm]")
    ));
    assert!(!agrees(serde_json::json!(100.0), serde_json::json!(101.0)));
    let numbers = [(0.0, 1.0)];
    assert!(super::substitutes(
        &numbers,
        &[],
        &serde_json::json!("0mm,0mm,-209.55mm"),
        &serde_json::json!("1mm,0mm,-209.55mm")
    ));
    assert!(!super::substitutes(
        &numbers,
        &[],
        &serde_json::json!("0mm,0mm,-209.55mm"),
        &serde_json::json!("1mm,1mm,-209.55mm")
    ));
}

/// A written part that does not decode again holds each written edit back,
/// and with none written it is named.
#[test]
fn a_written_part_that_does_not_decode_is_named() {
    let (source, input, id) = edited();

    let unwritten = ReadBack::unreadable(&claims(&source, &input, &id, false), "fails".into());
    let written = ReadBack::unreadable(&claims(&source, &input, &id, true), "fails".into());

    assert!(unwritten.withhold.is_empty());
    assert_eq!(unwritten.unread.as_deref(), Some("fails"));
    assert_eq!(written.withhold.len(), 1, "{:?}", written.withhold);
    assert!(written.unread.is_none());
}

/// The read-back names a writer difference and an unread written part with
/// their own losses.
#[test]
fn a_writer_difference_and_an_unread_part_are_named_by_their_losses() {
    use crate::loss::SldprtLossCode;

    let (source, input, _) = edited();
    let read_back = ReadBack {
        defects: vec!["model.features: 1 difference(s) (model.features x at /y)".into()],
        unread: Some("does not decode again (fails)".into()),
        ..ReadBack::default()
    };

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let notes = SourceEdits::new(&input, None, Some(&source), &[], &ctx)
        .unwrap()
        .into_parts()
        .unwritten
        .render(&read_back);

    for code in [
        SldprtLossCode::WriterReadBackDiffers,
        SldprtLossCode::WriterReadBackUnread,
    ] {
        assert_eq!(
            notes.iter().filter(|note| note.code == code.kind()).count(),
            1,
            "{notes:?}"
        );
    }
}
