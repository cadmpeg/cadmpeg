// SPDX-License-Identifier: Apache-2.0
//! The part's length unit: the length unit object of the `moUnitsTable_c`
//! each configuration's `moPart_c` writes in `Contents/Config-N`
//! (`docs/formats/sldprt.md`).

use cadmpeg_core::decode::View;
use cadmpeg_ir::design::units::UnitSymbol;

use crate::brep::graph::LINEAR_RESOLUTION_MM;
use crate::container::{ContainerScan, Section};
use crate::layout::{
    units_length_unit_block as block, units_length_unit_compound_four_word as four_word,
    units_length_unit_compound_three_word as three_word,
};

/// The units table class, as its declaration names it.
const TABLE_CLASS: &[u8] = b"moUnitsTable_c";
/// The declaration of the length unit class that opens the table's length
/// unit object: a new-class tag, schema 1, the name's length, and the name.
const LENGTH_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x13\x00moLengthUserUnits_c";
/// The declaration of the number unit class, whose object is one base long.
const NUMBER_DECLARATION: &[u8] = b"\xff\xff\x01\x00\x13\x00moNumberUserUnits_c";
/// A new-class tag and schema 1, which open a class declaration; the u16
/// length of the class name and the name follow.
const NEW_CLASS: &[u8] = b"\xff\xff\x01\x00";
/// The suffix of every unit class name.
const UNIT_CLASS_SUFFIX: &[u8] = b"Units_c";
/// The unit class besides the length unit that holds a length unit pointer.
const DENSITY_CLASS: &[u8] = b"moDensityUnits_c";
/// Relative tolerance within which a displayed number equals one rounding
/// step, and a rounding step equals the linear resolution.
const EPS_DISPLAY_STEP: f64 = 1.0e-9;

/// One base form of a unit object: the offsets, from the end of the class
/// declaration, of the length unit's display decimal places and unit code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Base {
    display_decimals: usize,
    unit_code: usize,
}

/// The base forms of a unit object: three and four words in a compound
/// document, and the block-container base.
const COMPOUND_BASES: [Base; 2] = [
    Base {
        display_decimals: three_word::DISPLAY_DECIMALS,
        unit_code: three_word::UNIT_CODE,
    },
    Base {
        display_decimals: four_word::DISPLAY_DECIMALS,
        unit_code: four_word::UNIT_CODE,
    },
];
const BLOCK_BASES: [Base; 1] = [Base {
    display_decimals: block::DISPLAY_DECIMALS,
    unit_code: block::UNIT_CODE,
}];

/// The unit in which a document displays lengths, and the decimal places to
/// which Keywords rounds a displayed length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LengthUnit {
    pub(crate) symbol: UnitSymbol,
    pub(crate) decimals: u32,
}

impl LengthUnit {
    /// Whether the displayed number `display` holds no value of the length
    /// it rounds: it is at most one rounding step from zero, and the step is
    /// coarser than the linear resolution, `1e-5` mm. A display whose step
    /// is at or below the linear resolution is exact, zero included.
    pub(crate) fn withholds(self, display: f64) -> bool {
        let step = i32::try_from(self.decimals).map_or(0.0, |decimals| 10f64.powi(-decimals));
        let (millimetres, _) = self.symbol.canonical();
        step * millimetres > LINEAR_RESOLUTION_MM * (1.0 + EPS_DISPLAY_STEP)
            && display.abs() <= step * (1.0 + EPS_DISPLAY_STEP)
    }
}

/// The length unit every configuration's units table states, or `None`
/// where a configuration states none or two state different ones. Where
/// configurations state different decimal places, the fewest hold, the
/// coarsest rounding of any of them.
pub(crate) fn length_unit(scan: &ContainerScan) -> Option<LengthUnit> {
    let mut found: Option<LengthUnit> = None;
    for section in scan.sections() {
        if !section.name().is_some_and(is_configuration) {
            continue;
        }
        let bases: &[Base] = match section {
            Section::Compound(_) => &COMPOUND_BASES,
            Section::Block(_) => &BLOCK_BASES,
        };
        let unit = configuration_length_unit(section.payload(), bases)?;
        found = Some(match found {
            None => unit,
            Some(known) if known.symbol == unit.symbol => LengthUnit {
                symbol: unit.symbol,
                decimals: known.decimals.min(unit.decimals),
            },
            Some(_) => return None,
        });
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

/// The length unit of one configuration's units table: after the table's
/// u16 object count, the object that declares the length unit class, whose
/// base opens with the display decimal places and whose u16 unit code
/// follows its base. The table writes it first, or after the other unit
/// classes it declares; a class declared before it that is not a unit class,
/// or that holds a length unit pointer itself, leaves the object unplaced
/// and states no unit. The base is the one of `bases` that the table's
/// number unit object measures.
fn configuration_length_unit(payload: &[u8], bases: &[Base]) -> Option<LengthUnit> {
    let table = find(payload, TABLE_CLASS)?.checked_add(TABLE_CLASS.len())?;
    let objects = payload.get(table.checked_add(2)?..)?;
    let object = find(objects, LENGTH_DECLARATION)?;
    if !declares_other_units(objects.get(..object)?) {
        return None;
    }
    let fields = object.checked_add(LENGTH_DECLARATION.len())?;
    let base = base(objects, bases)?;
    let symbol = unit(View::u16_le_at(
        objects,
        fields.checked_add(base.unit_code)?,
    )?)?;
    let decimals = View::u32_le_at(objects, fields.checked_add(base.display_decimals)?)?;
    Some(LengthUnit { symbol, decimals })
}

/// The one of `bases` whose length is that of the number unit object in
/// `objects`: the class declaration of the next object follows it.
fn base(objects: &[u8], bases: &[Base]) -> Option<Base> {
    let fields = find(objects, NUMBER_DECLARATION)?.checked_add(NUMBER_DECLARATION.len())?;
    let mut lengths = bases.iter().copied().filter(|base| {
        fields
            .checked_add(base.unit_code)
            .and_then(|next| objects.get(next..))
            .is_some_and(|next| next.starts_with(NEW_CLASS))
    });
    let base = lengths.next()?;
    lengths.next().is_none().then_some(base)
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
        2 => UnitSymbol::M,
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
