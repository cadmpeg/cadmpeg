// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::design::units::UnitSymbol;

use super::scene_units;
use crate::placement::NAME_MARKER;

fn name(text: &str) -> Vec<u8> {
    let mut out = NAME_MARKER.to_vec();
    out.push(u8::try_from(text.encode_utf16().count()).unwrap());
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

/// One unit entry: two separator bytes, the f64, and three adjacent strings.
fn entry(scale: f64, unit: &str, abbreviation: &str) -> Vec<u8> {
    [
        vec![0, 0],
        scale.to_le_bytes().to_vec(),
        name(unit),
        name(abbreviation),
        name("2,5"),
    ]
    .concat()
}

/// A `Document Data` prefix: the scene path, then the four unit entries.
fn document(length: Vec<u8>) -> Vec<u8> {
    [
        name("scene.ics"),
        length,
        entry(std::f64::consts::PI / 180.0, "Degrees", "deg"),
        entry(1.0, "Centimeters", "cm"),
        entry(1.0, "Kilograms", "kg"),
    ]
    .concat()
}

#[test]
fn the_first_length_entry_is_the_scene_length_unit() {
    let scene = scene_units(&document(entry(0.0254, "Inches", "in")));
    let units = scene.units.unwrap();
    assert_eq!(units.length, UnitSymbol::In);
    assert_eq!(units.angle, UnitSymbol::Deg);
    assert_eq!(units.mass, Some(UnitSymbol::Kg));
    assert!(scene.unrecognized.is_empty());
    assert!(scene.conflicts.is_empty());
}

#[test]
fn a_listed_name_selects_the_unit_and_a_contradicting_scale_is_noted() {
    let scene = scene_units(&document(entry(0.0254, "Millimeters", "mm")));
    assert_eq!(scene.units.unwrap().length, UnitSymbol::Mm);
    assert_eq!(scene.conflicts.len(), 1);
    assert!(scene.unrecognized.is_empty());
}

#[test]
fn an_unlisted_length_name_is_selected_by_its_si_scale() {
    let scene = scene_units(&document(entry(0.001, "Millimetres", "mm")));
    assert_eq!(scene.units.unwrap().length, UnitSymbol::Mm);
    assert_eq!(scene.unrecognized.len(), 1);
}

#[test]
fn an_unlisted_length_entry_never_takes_the_second_length_entry() {
    let scene = scene_units(&document(entry(0.1, "Decimeters", "dm")));
    assert!(scene.units.is_none());
    assert_eq!(scene.unrecognized.len(), 1);
}

#[test]
fn a_stream_without_unit_entries_states_no_unit_system_and_is_noted() {
    let scene = scene_units(&name("Degrees"));
    assert!(scene.units.is_none());
    assert_eq!(scene.unrecognized.len(), 1);
}
