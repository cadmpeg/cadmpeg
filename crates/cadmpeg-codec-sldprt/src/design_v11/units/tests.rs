// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::design::units::UnitSymbol;

use super::{
    configuration_length_unit, Base, LengthUnit, BLOCK_BASES, COMPOUND_BASES, DENSITY_CLASS,
    LENGTH_DECLARATION, NEW_CLASS, TABLE_CLASS,
};
use crate::layout::{
    units_length_unit_block as block, units_length_unit_compound_four_word as four_word,
    units_length_unit_compound_three_word as three_word,
};

/// The length unit symbol of the units table in `payload`.
fn symbol(payload: &[u8], bases: &[Base]) -> Option<UnitSymbol> {
    configuration_length_unit(payload, bases).map(|unit| unit.symbol)
}

/// A unit base: display decimal places, a u32, value, two u32 and a real,
/// then the u32 `words`, then `tail` more bytes.
fn base(words: &[u32], tail: usize) -> Vec<u8> {
    let mut bytes = vec![0xaa; 32];
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend(vec![0xbb; tail]);
    bytes
}

fn three_word_base() -> Vec<u8> {
    base(&[8, 0, 1], 0)
}

fn four_word_base() -> Vec<u8> {
    base(&[8, 0, 1, 1], 0)
}

fn block_base() -> Vec<u8> {
    base(&[8, 0, 1, 1], block::UNIT_CODE - four_word::UNIT_CODE)
}

/// A unit object of `class`: its class declaration, `base`, then the bytes
/// the class adds.
fn object(class: &[u8], base: &[u8], added: &[u8]) -> Vec<u8> {
    let mut bytes = NEW_CLASS.to_vec();
    bytes.extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(class);
    bytes.extend_from_slice(base);
    bytes.extend_from_slice(added);
    bytes
}

/// A length unit of unit code `code` with a null dual length unit.
fn length(base: &[u8], code: u16) -> Vec<u8> {
    let mut added = code.to_le_bytes().to_vec();
    added.extend_from_slice(&[0, 0]);
    object(b"moLengthUserUnits_c", base, &added)
}

fn angle(base: &[u8]) -> Vec<u8> {
    object(b"moAngleUserUnits_c", base, &[0, 0])
}

fn number(base: &[u8]) -> Vec<u8> {
    object(b"moNumberUserUnits_c", base, &[])
}

/// A configuration stream whose units table writes `objects`, then the
/// declaration of a density unit.
fn table(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = b"\xff\xff\x01\x00\x0e\x00".to_vec();
    payload.extend_from_slice(TABLE_CLASS);
    payload.extend_from_slice(&16u16.to_le_bytes());
    for object in objects {
        payload.extend_from_slice(object);
    }
    payload.extend_from_slice(NEW_CLASS);
    payload.extend_from_slice(&u16::try_from(DENSITY_CLASS.len()).unwrap().to_le_bytes());
    payload.extend_from_slice(DENSITY_CLASS);
    payload
}

/// A units table that opens with a length unit of unit code `code`.
fn first(base: &[u8], code: u16) -> Vec<u8> {
    table(&[length(base, code), angle(base), number(base)])
}

#[test]
fn the_unit_code_follows_the_base_the_number_unit_measures() {
    assert_eq!(three_word_base().len(), three_word::UNIT_CODE);
    assert_eq!(four_word_base().len(), four_word::UNIT_CODE);
    assert_eq!(block_base().len(), block::UNIT_CODE);
    for (base, bases) in [
        (three_word_base(), &COMPOUND_BASES[..]),
        (four_word_base(), &COMPOUND_BASES[..]),
        (block_base(), &BLOCK_BASES[..]),
    ] {
        for (code, unit) in [
            (0, UnitSymbol::Mm),
            (1, UnitSymbol::Cm),
            (2, UnitSymbol::M),
            (3, UnitSymbol::In),
        ] {
            assert_eq!(symbol(&first(&base, code), bases), Some(unit));
        }
    }
}

#[test]
fn the_length_unit_base_opens_with_the_display_decimal_places() {
    for (mut base, decimals, bases) in [
        (
            three_word_base(),
            three_word::DISPLAY_DECIMALS,
            &COMPOUND_BASES[..],
        ),
        (
            four_word_base(),
            four_word::DISPLAY_DECIMALS,
            &COMPOUND_BASES[..],
        ),
        (block_base(), block::DISPLAY_DECIMALS, &BLOCK_BASES[..]),
    ] {
        base[decimals..decimals + 4].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(
            configuration_length_unit(&first(&base, 2), bases),
            Some(LengthUnit {
                symbol: UnitSymbol::M,
                decimals: 3
            })
        );
    }
}

#[test]
fn a_display_at_most_one_coarse_rounding_step_from_zero_is_withheld() {
    let unit = |symbol, decimals| LengthUnit { symbol, decimals };
    let metre = |decimals| unit(UnitSymbol::M, decimals);
    for display in [0.0, -0.0, 0.01, -0.01] {
        assert!(metre(2).withholds(display), "{display}");
    }
    for display in [0.02, -0.02, 0.011, 1.0] {
        assert!(!metre(2).withholds(display), "{display}");
    }
    assert!(metre(0).withholds(1.0));
    assert!(!metre(0).withholds(2.0));
    // A step at or below the linear resolution, 1e-5 mm, is exact.
    for display in [0.0, 0.00001] {
        assert!(!unit(UnitSymbol::Mm, 5).withholds(display), "{display}");
    }
    assert!(unit(UnitSymbol::Mm, 4).withholds(0.0));
    assert!(unit(UnitSymbol::In, 6).withholds(0.0));
    assert!(!unit(UnitSymbol::In, 7).withholds(0.0));
    assert!(!metre(8).withholds(0.0));
    assert!(metre(7).withholds(0.0));
    assert!(!metre(u32::MAX).withholds(0.0));
}

#[test]
fn a_four_word_base_whose_last_word_is_zero_reads_the_code_after_it() {
    let base = base(&[0, 1, 0, 0], 0);
    assert_eq!(
        symbol(&first(&base, 3), &COMPOUND_BASES),
        Some(UnitSymbol::In)
    );
}

#[test]
fn a_number_unit_of_no_base_of_the_container_states_no_unit() {
    assert_eq!(symbol(&first(&block_base(), 3), &COMPOUND_BASES), None);
    assert_eq!(symbol(&first(&four_word_base(), 3), &BLOCK_BASES), None);
    let base = four_word_base();
    assert_eq!(
        symbol(&table(&[length(&base, 0), angle(&base)]), &COMPOUND_BASES),
        None
    );
    let class_reference = vec![0x01, 0x80, 0, 0];
    assert_eq!(
        symbol(
            &table(&[length(&base, 0), number(&base), class_reference]),
            &COMPOUND_BASES
        ),
        None
    );
}

#[test]
fn an_unread_code_or_another_first_object_states_no_unit() {
    for code in [4, u16::MAX] {
        assert_eq!(symbol(&first(&block_base(), code), &BLOCK_BASES), None);
    }
    let mut other = first(&block_base(), 0);
    let at = other
        .windows(LENGTH_DECLARATION.len())
        .position(|window| window == LENGTH_DECLARATION)
        .unwrap();
    other[at + 6] = b'x';
    assert_eq!(symbol(&other, &BLOCK_BASES), None);
    assert_eq!(symbol(b"moUnitsTable_c", &BLOCK_BASES), None);
}

/// A units table that writes its length unit of unit code `code` after the
/// unit objects `before`.
fn after(before: &[Vec<u8>], base: &[u8], code: u16) -> Vec<u8> {
    let mut objects = before.to_vec();
    objects.push(length(base, code));
    table(&objects)
}

#[test]
fn a_length_unit_after_the_other_unit_classes_states_its_code() {
    for (base, bases) in [
        (four_word_base(), &COMPOUND_BASES[..]),
        (block_base(), &BLOCK_BASES[..]),
    ] {
        let before = [
            angle(&base),
            number(&base),
            object(b"moTimeUnits_c", &base, &[0, 0]),
            object(b"moFrequencyUserUnits_c", &base, &[]),
        ];
        for (code, unit) in [(0, UnitSymbol::Mm), (3, UnitSymbol::In)] {
            assert_eq!(symbol(&after(&before, &base, code), bases), Some(unit));
        }
    }
}

#[test]
fn a_length_unit_after_another_class_or_a_density_unit_states_no_unit() {
    let base = block_base();
    for class in [&b"moVisualProperties_c"[..], DENSITY_CLASS] {
        let before = [angle(&base), number(&base), object(class, &base, &[0, 0])];
        assert_eq!(symbol(&after(&before, &base, 0), &BLOCK_BASES), None);
    }
    let mut cut = after(&[angle(&base), number(&base)], &base, 0);
    let length = NEW_CLASS.len() + 2 + TABLE_CLASS.len() + 2 + NEW_CLASS.len();
    cut[length..length + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(symbol(&cut, &BLOCK_BASES), None);
}
