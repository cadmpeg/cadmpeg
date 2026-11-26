// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::design::units::UnitSymbol;

use super::scene_units;
use crate::placement::NAME_MARKER;

fn name(text: &str) -> Vec<u8> {
    let mut out = NAME_MARKER.to_vec();
    out.push(u8::try_from(text.encode_utf16().count()).unwrap());
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

#[test]
fn the_first_unit_of_each_quantity_is_the_scene_unit() {
    let payload = [
        name("Millimeters"),
        name("mm"),
        name("Degrees"),
        name("Centimeters"),
        name("Kilograms"),
    ]
    .concat();
    let units = scene_units(&payload).unwrap();
    assert_eq!(units.length, UnitSymbol::Mm);
    assert_eq!(units.angle, UnitSymbol::Deg);
    assert_eq!(units.mass, Some(UnitSymbol::Kg));
}

#[test]
fn a_stream_without_a_length_unit_states_no_unit_system() {
    assert!(scene_units(&name("Degrees")).is_none());
}
