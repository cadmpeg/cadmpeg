// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::design::units::UnitSymbol;

use super::{property_sets, unit_system, Property, Value};
use crate::test_support::{property_set, property_set_of_form, RecordValue};

fn sets(bytes: &[u8]) -> Vec<super::PropertySet> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let sets = property_sets(&ctx, bytes);
    ctx.finish_session().unwrap();
    sets
}

fn units(length: u32, angle: u32) -> Vec<u8> {
    property_set(
        2,
        &[
            ("DefaultDistancePrimary", RecordValue::Word(2, length)),
            ("DefaultDistancePrecision", RecordValue::Word(2, 2)),
            ("DefaultAnglePrimary", RecordValue::Word(2, angle)),
            ("StorePreview", RecordValue::Word(6, 1)),
            ("StepValue", RecordValue::Real(0.01)),
            ("PartStyle", RecordValue::Word(1, 5)),
        ],
    )
}

#[test]
fn property_set_walks_every_value_type() {
    let found = sets(&units(61, 357));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].index, 2);
    assert_eq!(found[0].properties.len(), 6);
    assert_eq!(
        found[0].properties[4],
        Property {
            name: "StepValue".into(),
            value: Value::Real(0.01),
        }
    );
    assert_eq!(found[0].properties[5].value, Value::Word(5));
}

#[test]
fn length_and_angle_codes_select_the_unit_system() {
    for (code, unit) in [
        (59, UnitSymbol::M),
        (61, UnitSymbol::Mm),
        (62, UnitSymbol::Cm),
        (64, UnitSymbol::In),
    ] {
        for (angle, angle_unit) in [(357, UnitSymbol::Deg), (77, UnitSymbol::Rad)] {
            let mut losses = Vec::new();
            let system = unit_system(&sets(&units(code, angle)), &mut losses).unwrap();
            assert_eq!(system.length, unit);
            assert_eq!(system.angle, angle_unit);
            assert!(losses.is_empty());
        }
    }
}

#[test]
fn unknown_unit_code_is_a_loss() {
    let mut losses = Vec::new();
    assert!(unit_system(&sets(&units(1000, 357)), &mut losses).is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "units.code-unknown");
}

#[test]
fn malformed_set_is_skipped_and_a_later_set_is_read() {
    // A set whose second record has an unknown value type does not walk;
    // the following set still yields the unit system.
    let mut bytes = property_set(
        1,
        &[
            ("First", RecordValue::Word(2, 0)),
            ("Second", RecordValue::Word(9, 0)),
        ],
    );
    bytes.extend(units(64, 77));
    let found = sets(&bytes);
    assert_eq!(found.len(), 1);
    let mut losses = Vec::new();
    let system = unit_system(&found, &mut losses).unwrap();
    assert_eq!(
        (system.length, system.angle),
        (UnitSymbol::In, UnitSymbol::Rad)
    );
}

#[test]
fn truncated_set_yields_no_set() {
    let bytes = units(61, 357);
    assert!(sets(&bytes[..bytes.len() - 3]).is_empty());
}

#[test]
fn missing_length_record_is_a_loss() {
    let bytes = property_set(2, &[("DefaultAnglePrimary", RecordValue::Word(2, 357))]);
    let mut losses = Vec::new();
    assert!(unit_system(&sets(&bytes), &mut losses).is_none());
    assert_eq!(losses[0].code.local_code(), "units.unread");
}

#[test]
fn set_form_does_not_change_the_record_grammar() {
    for form in [0, 1, 2] {
        let bytes = property_set_of_form(
            form,
            2,
            &[
                ("DefaultDistancePrimary", RecordValue::Word(2, 62)),
                ("DefaultAnglePrimary", RecordValue::Word(2, 357)),
            ],
        );
        let mut losses = Vec::new();
        let system = unit_system(&sets(&bytes), &mut losses).unwrap();
        assert_eq!(system.length, UnitSymbol::Cm);
    }
}
