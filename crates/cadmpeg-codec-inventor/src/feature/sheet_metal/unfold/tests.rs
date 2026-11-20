// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::record_identity::RecordTypeId;
use crate::test_support::test_fixtures::parse;
use cadmpeg_ir::ids::IdentityKey;

const VERSION: u8 = 29;
pub(crate) const RULE_ORDINAL: u32 = 12;

fn text(bytes: &mut Vec<u8>, value: &str) {
    let units: Vec<u16> = value.encode_utf16().collect();
    bytes.extend_from_slice(&u32::try_from(units.len()).expect("length").to_le_bytes());
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

fn rule_bytes(name: &str, expression: &str) -> Vec<u8> {
    versioned_rule_bytes(VERSION, name, expression)
}

fn versioned_rule_bytes(version: u8, name: &str, expression: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0x0123_u16.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    if version > LAST_UNPADDED_MAJOR {
        bytes.extend_from_slice(&0_u16.to_le_bytes());
    }
    bytes.extend_from_slice(&3_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0x8000_0007_u32.to_le_bytes());
    text(&mut bytes, name);
    text(&mut bytes, "");
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    text(&mut bytes, &format!("1:{name}\n"));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    text(&mut bytes, expression);
    text(&mut bytes, "0.61 ul");
    bytes.extend_from_slice(&[0x02, 0x00, 0x00, 0x30]);
    bytes
}

fn selection_bytes(category: &str, entries: &[(&str, u32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0x0456_u16.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(category.len()).expect("length").to_le_bytes());
    bytes.extend_from_slice(category.as_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0x8000_0003_u32.to_le_bytes());
    bytes.extend_from_slice(&0x3000_0006_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(entries.len()).expect("count").to_le_bytes());
    if !entries.is_empty() {
        bytes.extend_from_slice(&0x11_u32.to_le_bytes());
        bytes.extend_from_slice(&0x0e_u32.to_le_bytes());
    }
    for (name, index) in entries {
        text(&mut bytes, name);
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes.extend_from_slice(&[0x09, 0x04, 0, 0, 0, 0, 0, 0]);
    bytes
}

fn token() -> IdentityKey {
    IdentityKey::try_new("fixture").expect("token")
}

pub(crate) fn rule(name: &str, expression: &str) -> Rule {
    let payload = parse(&rule_bytes(name, expression), |ctx, view| {
        parse_rule(ctx, view, VERSION)
    })
    .expect("rule");
    Located::new(
        payload,
        RecordTypeId::from_bytes(RULE_TYPE),
        token(),
        RULE_ORDINAL,
    )
}

pub(crate) fn selection(entries: &[(&str, u32)]) -> Selection {
    let payload = parse(
        &selection_bytes("SheetMetalUnfold", entries),
        parse_selection,
    )
    .expect("selection")
    .expect("unfold category");
    Located::new(
        payload,
        RecordTypeId::from_bytes(SELECTION_TYPE),
        token(),
        3,
    )
}

fn neutral_factor(rules: &[Rule], selections: &[Selection]) -> Option<FiniteReal> {
    active_rule(rules, selections).and_then(|rule| factor(&rule.expression))
}

fn value(expected: f64) -> FiniteReal {
    FiniteReal::new(expected).expect("finite")
}

#[test]
fn rule_layout_reads_name_key_and_both_expression_texts() {
    let rule = rule("Unfold_A", "0.37 ul");
    assert_eq!(rule.name, "Unfold_A");
    assert_eq!(rule.key, "1:Unfold_A\n");
    assert_eq!(rule.form, 1);
    assert_eq!(rule.expression, "0.37 ul");
    assert_eq!(rule.display, "0.61 ul");
    assert_eq!(rule.suffix.as_ref().len(), 4);
}

#[test]
fn rule_layout_has_the_padding_word_only_after_version_23() {
    for version in [FIRST_SUPPORTED_MAJOR, LAST_UNPADDED_MAJOR] {
        let rule = parse(
            &versioned_rule_bytes(version, "Unfold_A", "0.29"),
            |ctx, view| parse_rule(ctx, view, version),
        )
        .expect("unpadded rule");
        assert_eq!(rule.expression, "0.29");
        assert!(!parse(&rule_bytes("Unfold_A", "0.29"), |ctx, view| {
            parse_rule(ctx, view, version)
        })
        .is_ok_and(|rule| rule.expression == "0.29"));
    }
    assert!(parse(&rule_bytes("Unfold_A", "0.29"), |ctx, view| {
        parse_rule(ctx, view, FIRST_SUPPORTED_MAJOR - 1)
    })
    .is_err());
}

#[test]
fn selection_of_another_category_is_not_an_unfold_selection() {
    let parsed = parse(
        &selection_bytes("SheetMetalRules", &[("A", 4)]),
        parse_selection,
    )
    .expect("category record");
    assert!(parsed.is_none());
}

#[test]
fn suffixed_unitless_text_is_the_neutral_factor() {
    let rules = [rule("Unfold_A", "0.37 ul")];
    let selections = [selection(&[("Unfold_A", RULE_ORDINAL + 1)])];
    assert_eq!(neutral_factor(&rules, &selections), Some(value(0.37)));
}

#[test]
fn bare_decimal_text_is_the_neutral_factor() {
    let rules = [rule("Unfold_A", "0.29")];
    let selections = [selection(&[("Unfold_A", RULE_ORDINAL + 1)])];
    assert_eq!(neutral_factor(&rules, &selections), Some(value(0.29)));
}

#[test]
fn malformed_text_has_no_neutral_factor() {
    for text in [
        "0.37 mm", "", " ul", ".", "-0.37", "+0.37", "3.7e-1", "inf", "0.3.7", "0,37",
    ] {
        let rules = [rule("Unfold_A", text)];
        let selections = [selection(&[("Unfold_A", RULE_ORDINAL + 1)])];
        assert_eq!(neutral_factor(&rules, &selections), None, "{text:?}");
    }
}

#[test]
fn name_or_index_disagreement_has_no_neutral_factor() {
    let rules = [rule("Unfold_A", "0.37 ul")];
    for entries in [
        [("Unfold_B", RULE_ORDINAL + 1)],
        [("Unfold_A", RULE_ORDINAL)],
        [("Unfold_A", 0)],
    ] {
        assert_eq!(neutral_factor(&rules, &[selection(&entries)]), None);
    }
}

#[test]
fn several_entries_or_selections_have_no_neutral_factor() {
    let rules = [rule("Unfold_A", "0.37 ul")];
    let entry = ("Unfold_A", RULE_ORDINAL + 1);
    assert_eq!(
        neutral_factor(&rules, &[selection(&[entry, ("Unfold_B", 40)])]),
        None
    );
    assert_eq!(
        neutral_factor(&rules, &[selection(&[entry]), selection(&[entry])]),
        None
    );
    assert_eq!(neutral_factor(&rules, &[selection(&[])]), None);
    assert_eq!(neutral_factor(&rules, &[]), None);
}
