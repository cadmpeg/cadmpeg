// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::design::units::UnitSymbol;

use super::{
    configuration_length_unit, BLOCK_BASE, COMPOUND_BASE, LAST_WORD, LAST_WORD_VALUE,
    LENGTH_DECLARATION, TABLE_CLASS,
};

/// A configuration stream whose units table opens with a length unit of
/// unit code `code` after a `base`-byte base.
fn table(base: usize, code: u16) -> Vec<u8> {
    let mut payload = b"\xff\xff\x01\x00\x0e\x00".to_vec();
    payload.extend_from_slice(TABLE_CLASS);
    payload.extend_from_slice(&16u16.to_le_bytes());
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
