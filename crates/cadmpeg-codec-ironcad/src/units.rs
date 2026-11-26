// SPDX-License-Identifier: Apache-2.0
//! Scene units of the `Document Data` stream (`ironcad.md` §4).

use cadmpeg_ir::design::units::{UnitSymbol, UnitSystem};

use crate::placement::{find, name_at, NAME_MARKER};

/// The unit that each stored unit name selects.
const UNIT_NAMES: [(&str, Quantity, UnitSymbol); 10] = [
    ("Millimeters", Quantity::Length, UnitSymbol::Mm),
    ("Centimeters", Quantity::Length, UnitSymbol::Cm),
    ("Meters", Quantity::Length, UnitSymbol::M),
    ("Inches", Quantity::Length, UnitSymbol::In),
    ("Feet", Quantity::Length, UnitSymbol::Ft),
    ("Degrees", Quantity::Angle, UnitSymbol::Deg),
    ("Radians", Quantity::Angle, UnitSymbol::Rad),
    ("Kilograms", Quantity::Mass, UnitSymbol::Kg),
    ("Grams", Quantity::Mass, UnitSymbol::G),
    ("Pounds", Quantity::Mass, UnitSymbol::Lb),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quantity {
    Length,
    Angle,
    Mass,
}

/// The scene unit system: the first stored length, angle, and mass unit.
pub(crate) fn scene_units(payload: &[u8]) -> Option<UnitSystem> {
    let mut length = None;
    let mut angle = None;
    let mut mass = None;
    let mut at = 0;
    while let Some(found) = find(payload, &NAME_MARKER, at) {
        at = found + NAME_MARKER.len();
        let Some(name) = name_at(payload, found) else {
            continue;
        };
        let Some((_, quantity, symbol)) = UNIT_NAMES.iter().find(|(text, ..)| *text == name) else {
            continue;
        };
        let slot = match quantity {
            Quantity::Length => &mut length,
            Quantity::Angle => &mut angle,
            Quantity::Mass => &mut mass,
        };
        slot.get_or_insert(*symbol);
    }
    Some(UnitSystem {
        length: length?,
        angle: angle?,
        mass,
        time: None,
    })
}

#[cfg(test)]
mod tests;
