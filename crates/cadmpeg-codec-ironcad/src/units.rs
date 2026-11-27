// SPDX-License-Identifier: Apache-2.0
//! Scene units of the `Document Data` stream (`ironcad.md` §4).

use cadmpeg_core::decode::View;
use cadmpeg_ir::design::units::{UnitSymbol, UnitSystem};

use crate::placement::{find, NAME_MARKER};

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

/// The SI scale of each unit that an entry's scale can select.
const UNIT_SCALES: [(f64, Quantity, UnitSymbol); 10] = [
    (0.001, Quantity::Length, UnitSymbol::Mm),
    (0.01, Quantity::Length, UnitSymbol::Cm),
    (1.0, Quantity::Length, UnitSymbol::M),
    (0.0254, Quantity::Length, UnitSymbol::In),
    (0.3048, Quantity::Length, UnitSymbol::Ft),
    (
        std::f64::consts::PI / 180.0,
        Quantity::Angle,
        UnitSymbol::Deg,
    ),
    (1.0, Quantity::Angle, UnitSymbol::Rad),
    (1.0, Quantity::Mass, UnitSymbol::Kg),
    (0.001, Quantity::Mass, UnitSymbol::G),
    (0.453_592_37, Quantity::Mass, UnitSymbol::Lb),
];

/// Relative agreement of a stored SI scale with the scale of a unit.
const EPS_SCALE_RELATIVE: f64 = 1.0e-9;

/// Strings in one unit entry: the name, the abbreviation, and the precision.
const ENTRY_STRINGS: usize = 3;

/// Entry positions of the scene length, angle, and mass units. The entry at
/// position 2 is the second length entry.
const SCENE_ENTRIES: [(usize, Quantity); 3] = [
    (0, Quantity::Length),
    (1, Quantity::Angle),
    (3, Quantity::Mass),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Quantity {
    Length,
    Angle,
    Mass,
}

impl Quantity {
    const fn label(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Angle => "angle",
            Self::Mass => "mass",
        }
    }
}

/// One unit entry: the f64 before its name, and its name.
#[derive(Debug, PartialEq)]
struct Entry {
    scale: Option<f64>,
    name: String,
}

/// The scene units, a note for each scene entry whose name the table does
/// not hold, and a note for each scene entry whose stored scale contradicts
/// its name.
#[derive(Debug, Default)]
pub(crate) struct SceneUnits {
    pub(crate) units: Option<UnitSystem>,
    pub(crate) unrecognized: Vec<String>,
    pub(crate) conflicts: Vec<String>,
}

/// The scene unit system: the units of the first length entry, the angle
/// entry, and the mass entry.
pub(crate) fn scene_units(payload: &[u8]) -> SceneUnits {
    let entries = entries(payload);
    let mut scene = SceneUnits::default();
    if entries.is_empty() {
        scene
            .unrecognized
            .push("the stream holds no unit entry; the units are not stated".to_owned());
        return scene;
    }
    let mut resolved = [None; 3];
    for ((position, quantity), slot) in SCENE_ENTRIES.into_iter().zip(&mut resolved) {
        let Some(entry) = entries.get(position) else {
            continue;
        };
        let by_name = UNIT_NAMES
            .iter()
            .find(|(name, of, _)| *name == entry.name && *of == quantity)
            .map(|(.., symbol)| *symbol);
        let by_scale = entry.scale.and_then(|scale| unit_of_scale(scale, quantity));
        if let Some(symbol) = by_name {
            *slot = by_name;
            if by_scale != Some(symbol) {
                scene.conflicts.push(format!(
                    "the {} entry names `{}` and stores the SI scale {}; the name selects `{}`",
                    quantity.label(),
                    entry.name,
                    entry
                        .scale
                        .map_or_else(|| "none".to_owned(), |scale| scale.to_string()),
                    symbol.name()
                ));
            }
            continue;
        }
        *slot = by_scale;
        scene.unrecognized.push(match by_scale {
            Some(symbol) => format!(
                "the {} entry names `{}`; its SI scale selects `{}`",
                quantity.label(),
                entry.name,
                symbol.name()
            ),
            None => format!(
                "the {} entry names `{}` with no known SI scale; the unit is not stated",
                quantity.label(),
                entry.name
            ),
        });
    }
    let [length, angle, mass] = resolved;
    scene.units = length.zip(angle).map(|(length, angle)| UnitSystem {
        length,
        angle,
        mass,
        time: None,
    });
    scene
}

/// The unit of `quantity` whose SI scale is `scale`.
fn unit_of_scale(scale: f64, quantity: Quantity) -> Option<UnitSymbol> {
    UNIT_SCALES
        .iter()
        .find(|(si, of, _)| *of == quantity && (scale - si).abs() <= EPS_SCALE_RELATIVE * si.abs())
        .map(|(.., symbol)| *symbol)
}

/// The unit entries in stored order: each run of three adjacent strings,
/// with the f64 that precedes its first string.
fn entries(payload: &[u8]) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut run: Vec<(usize, String)> = Vec::new();
    let mut run_end = None;
    let mut at = 0;
    while let Some(found) = find(payload, &NAME_MARKER, at) {
        let Some(&count) = payload.get(found + NAME_MARKER.len()) else {
            break;
        };
        let start = found + NAME_MARKER.len() + 1;
        let end = start + 2 * usize::from(count);
        let Some(text) = (0..usize::from(count))
            .map(|index| View::u16_le_at(payload, start + 2 * index))
            .collect::<Option<Vec<u16>>>()
            .map(|units| String::from_utf16_lossy(&units))
        else {
            break;
        };
        if run_end != Some(found) {
            close_run(payload, &mut run, &mut entries);
        }
        run.push((found, text));
        run_end = Some(end);
        at = end;
    }
    close_run(payload, &mut run, &mut entries);
    entries
}

/// Records `run` as an entry when it holds the three strings of one.
fn close_run(payload: &[u8], run: &mut Vec<(usize, String)>, entries: &mut Vec<Entry>) {
    if run.len() == ENTRY_STRINGS {
        let (at, name) = run.swap_remove(0);
        let scale = at
            .checked_sub(std::mem::size_of::<f64>())
            .and_then(|offset| View::f64_le_at(payload, offset));
        entries.push(Entry { scale, name });
    }
    run.clear();
}

#[cfg(test)]
mod tests;
