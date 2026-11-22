// SPDX-License-Identifier: Apache-2.0
//! The part's length unit and length precision: the length unit object of
//! the `moUnitsTable_c` each configuration's `moPart_c` writes in
//! `Contents/Config-N` (`docs/formats/sldprt.md`).

use cadmpeg_core::decode::View;
use cadmpeg_ir::design::units::UnitSymbol;

use crate::container::{ContainerScan, Section};

/// The units table class, as its declaration names it.
const TABLE_CLASS: &[u8] = b"moUnitsTable_c";
/// The declaration of the length unit class that opens the table's length
/// unit object: a new-class tag, schema 1, the name's length, and the name.
const LENGTH_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x13\x00moLengthUserUnits_c";
/// A new-class tag and schema 1, which open a class declaration; the u16
/// length of the class name and the name follow.
const NEW_CLASS: &[u8] = b"\xff\xff\x01\x00";
/// The suffix of every unit class name.
const UNIT_CLASS_SUFFIX: &[u8] = b"Units_c";
/// The unit class besides the length unit that holds a length unit pointer.
const DENSITY_CLASS: &[u8] = b"moDensityUnits_c";
/// The bytes of a unit object's base before its unit code: in a compound
/// document, and in a block-container document.
const COMPOUND_BASE: usize = 48;
const BLOCK_BASE: usize = 62;
/// The offset in a unit object's fields of the last of the base's four u32
/// words, and the value it holds.
const LAST_WORD: usize = 44;
const LAST_WORD_VALUE: u32 = 1;

/// The length unit every configuration's units table states, or `None`
/// where a configuration states none or two state different ones.
pub(crate) fn length_unit(scan: &ContainerScan) -> Option<UnitSymbol> {
    agreed(scan, configuration_length_unit)
}

/// The number of decimal places at which every configuration's units table
/// shows lengths: the first u32 of the length unit object's base. `None`
/// where a configuration states none or two state different ones.
pub(crate) fn length_places(scan: &ContainerScan) -> Option<u32> {
    agreed(scan, |payload, _| configuration_length_places(payload))
}

/// The value `read` takes from every configuration's stream, given the size
/// of a unit object's base in its container: `None` where a configuration
/// states none, or two configurations state different values.
fn agreed<T: Copy + PartialEq>(
    scan: &ContainerScan,
    read: impl Fn(&[u8], usize) -> Option<T>,
) -> Option<T> {
    let mut found = None;
    for section in scan.sections() {
        if !section.name().is_some_and(is_configuration) {
            continue;
        }
        let base = match section {
            Section::Compound(_) => COMPOUND_BASE,
            Section::Block(_) => BLOCK_BASE,
        };
        let value = read(section.payload(), base)?;
        if found.is_some_and(|known| known != value) {
            return None;
        }
        found = Some(value);
    }
    found
}

/// Whether `stream` is a configuration's own stream, `Contents/Config-N`,
/// in either separator and any letter case.
fn is_configuration(stream: &str) -> bool {
    const PREFIX: &str = "contents/config-";
    let stream = stream.replace('\\', "/");
    stream.len() > PREFIX.len()
        && stream.is_char_boundary(PREFIX.len())
        && stream[..PREFIX.len()].eq_ignore_ascii_case(PREFIX)
        && stream[PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit())
}

/// The offset of the fields of one configuration's length unit object:
/// after the units table's u16 object count, the object that declares the
/// length unit class. The table writes it first, or after the other unit
/// classes it declares; a class declared before it that is not a unit class,
/// or that holds a length unit pointer itself, leaves the object unplaced. A
/// base whose four u32 words do not end in [`LAST_WORD_VALUE`] is another
/// form, which is not read.
fn length_fields(payload: &[u8]) -> Option<usize> {
    let table = find(payload, TABLE_CLASS)?.checked_add(TABLE_CLASS.len())?;
    let objects = table.checked_add(2)?;
    let object = objects.checked_add(find(payload.get(objects..)?, LENGTH_DECLARATION)?)?;
    let fields = object.checked_add(LENGTH_DECLARATION.len())?;
    (declares_other_units(payload.get(objects..object)?)
        && View::u32_le_at(payload, fields.checked_add(LAST_WORD)?) == Some(LAST_WORD_VALUE))
    .then_some(fields)
}

/// The length unit of one configuration's units table: the u16 unit code
/// that follows the length unit object's `base`-byte base.
fn configuration_length_unit(payload: &[u8], base: usize) -> Option<UnitSymbol> {
    unit(View::u16_le_at(
        payload,
        length_fields(payload)?.checked_add(base)?,
    )?)
}

/// The length precision of one configuration's units table: the first u32
/// of the length unit object's base, the number of decimal places at which
/// the document shows lengths.
fn configuration_length_places(payload: &[u8]) -> Option<u32> {
    View::u32_le_at(payload, length_fields(payload)?)
}

/// Whether every class that `objects`, the table's objects before its length
/// unit, declare is a unit class that holds no length unit pointer.
fn declares_other_units(mut objects: &[u8]) -> bool {
    while let Some(tag) = find(objects, NEW_CLASS) {
        let Some((name, rest)) = declared_class(objects, tag) else {
            return false;
        };
        if !name.ends_with(UNIT_CLASS_SUFFIX) || name == DENSITY_CLASS {
            return false;
        }
        objects = rest;
    }
    true
}

/// The class name of the declaration at `tag` in `bytes`, and the bytes
/// after it.
fn declared_class(bytes: &[u8], tag: usize) -> Option<(&[u8], &[u8])> {
    let length = tag.checked_add(NEW_CLASS.len())?;
    let name = length.checked_add(2)?;
    let end = name.checked_add(usize::from(View::u16_le_at(bytes, length)?))?;
    Some((bytes.get(name..end)?, bytes.get(end..)?))
}

/// The unit a length unit code names.
fn unit(code: u16) -> Option<UnitSymbol> {
    Some(match code {
        0 => UnitSymbol::Mm,
        1 => UnitSymbol::Cm,
        3 => UnitSymbol::In,
        _ => return None,
    })
}

fn find(payload: &[u8], needle: &[u8]) -> Option<usize> {
    payload
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests;
