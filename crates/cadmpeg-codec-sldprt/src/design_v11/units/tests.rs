// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::design::units::UnitSymbol;

use super::{
    configuration_length_places, configuration_length_unit, BLOCK_BASE, COMPOUND_BASE,
    DENSITY_CLASS, LAST_WORD, LAST_WORD_VALUE, LENGTH_DECLARATION, NEW_CLASS, TABLE_CLASS,
};

/// A configuration stream whose units table opens with a length unit of
/// unit code `code` after a `base`-byte base.
fn table(base: usize, code: u16) -> Vec<u8> {
    table_after(&[], base, code)
}

/// A configuration stream whose units table writes one object of each class
/// in `before`, with a `base`-byte base and a u16, then a length unit of
/// unit code `code` after a `base`-byte base.
fn table_after(before: &[&[u8]], base: usize, code: u16) -> Vec<u8> {
    let mut payload = b"\xff\xff\x01\x00\x0e\x00".to_vec();
    payload.extend_from_slice(TABLE_CLASS);
    payload.extend_from_slice(&16u16.to_le_bytes());
    for class in before {
        payload.extend_from_slice(NEW_CLASS);
        payload.extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
        payload.extend_from_slice(class);
        payload.extend(vec![0; base + 2]);
    }
    payload.extend_from_slice(LENGTH_DECLARATION);
    let mut fields = vec![0xaa; base];
    fields[LAST_WORD..LAST_WORD + 4].copy_from_slice(&LAST_WORD_VALUE.to_le_bytes());
    payload.extend(fields);
    payload.extend_from_slice(&code.to_le_bytes());
    payload.extend_from_slice(&[0, 0]);
    payload
}

#[test]
fn the_unit_code_follows_the_base_of_the_container() {
    for base in [COMPOUND_BASE, BLOCK_BASE] {
        for (code, unit) in [
            (0, UnitSymbol::Mm),
            (1, UnitSymbol::Cm),
            (3, UnitSymbol::In),
        ] {
            assert_eq!(
                configuration_length_unit(&table(base, code), base),
                Some(unit)
            );
        }
    }
    assert_eq!(
        configuration_length_unit(&table(BLOCK_BASE, 3), COMPOUND_BASE),
        None
    );
}

#[test]
fn an_unread_code_or_another_first_object_states_no_unit() {
    assert_eq!(
        configuration_length_unit(&table(BLOCK_BASE, 2), BLOCK_BASE),
        None
    );
    assert_eq!(
        configuration_length_unit(&table(BLOCK_BASE, u16::MAX), BLOCK_BASE),
        None
    );
    let mut other = table(BLOCK_BASE, 0);
    let at = other
        .windows(LENGTH_DECLARATION.len())
        .position(|window| window == LENGTH_DECLARATION)
        .unwrap();
    other[at + 6] = b'x';
    assert_eq!(configuration_length_unit(&other, BLOCK_BASE), None);
    assert_eq!(
        configuration_length_unit(b"moUnitsTable_c", BLOCK_BASE),
        None
    );
    let mut three_words = table(COMPOUND_BASE, 0);
    let fields = three_words.len() - 4 - COMPOUND_BASE;
    three_words[fields + LAST_WORD] = 0;
    assert_eq!(configuration_length_unit(&three_words, COMPOUND_BASE), None);
}

#[test]
fn a_length_unit_after_the_other_unit_classes_states_its_code() {
    let before: [&[u8]; 3] = [
        b"moAngleUserUnits_c",
        b"moTimeUnits_c",
        b"moFrequencyUserUnits_c",
    ];
    for base in [COMPOUND_BASE, BLOCK_BASE] {
        for (code, unit) in [(0, UnitSymbol::Mm), (3, UnitSymbol::In)] {
            assert_eq!(
                configuration_length_unit(&table_after(&before, base, code), base),
                Some(unit)
            );
        }
    }
}

#[test]
fn a_length_unit_after_another_class_or_a_density_unit_states_no_unit() {
    for class in [&b"moVisualProperties_c"[..], DENSITY_CLASS] {
        let before = [&b"moAngleUserUnits_c"[..], class];
        assert_eq!(
            configuration_length_unit(&table_after(&before, BLOCK_BASE, 0), BLOCK_BASE),
            None
        );
    }
    let mut cut = table_after(&[b"moAngleUserUnits_c"], BLOCK_BASE, 0);
    let length = NEW_CLASS.len() + TABLE_CLASS.len() + 2 + 2 + NEW_CLASS.len();
    cut[length..length + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(configuration_length_unit(&cut, BLOCK_BASE), None);
}

#[test]
fn the_length_precision_is_the_first_word_of_the_length_unit_base() {
    for base in [COMPOUND_BASE, BLOCK_BASE] {
        for before in [&[][..], &[&b"moAngleUserUnits_c"[..]][..]] {
            let mut payload = table_after(before, base, 0);
            let fields = payload.len() - 4 - base;
            payload[fields..fields + 4].copy_from_slice(&2u32.to_le_bytes());
            assert_eq!(configuration_length_places(&payload), Some(2));
            payload[fields + LAST_WORD] = 0;
            assert_eq!(configuration_length_places(&payload), None);
        }
    }
    let after_density = table_after(&[DENSITY_CLASS], BLOCK_BASE, 0);
    assert_eq!(configuration_length_places(&after_density), None);
}
