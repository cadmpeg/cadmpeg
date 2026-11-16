// SPDX-License-Identifier: Apache-2.0

use super::{datum_part, text_magnitude};

const VALUE_TOLERANCE: f64 = 1.0e-12;

#[test]
fn frame_decorations_keep_first_values_and_reject_unknown_logicals() {
    use super::attributes::{Attribute, Value};
    use super::{add_attribute, Tolerance};

    let attribute = |name, value| Attribute {
        name,
        value,
        owner: Some(42),
        owner_type: Some(3),
        unit: b"",
        offset: 17,
    };
    let mut tolerance = Tolerance::default();
    add_attribute(
        &mut tolerance,
        attribute(b"PTC_GTOL_ALLAROUND_SYMBOL", Value::Logical(Some(false))),
    );
    add_attribute(
        &mut tolerance,
        attribute(b"PTC_GTOL_ALLAROUND_SYMBOL", Value::Logical(Some(true))),
    );
    assert_eq!(tolerance.all_around, Some(false));
    assert!(tolerance.duplicate);
    add_attribute(
        &mut tolerance,
        attribute(b"PTC_GTOL_ALLAROUND_SYMBOL", Value::Logical(None)),
    );
    assert!(tolerance.invalid_decoration);
    add_attribute(
        &mut tolerance,
        attribute(b"PTC_GTOL_TEXT_BELOW", Value::Text("A↔B".as_bytes())),
    );
    add_attribute(
        &mut tolerance,
        attribute(b"PTC_GTOL_TEXT_BELOW", Value::Text(b"other")),
    );
    assert_eq!(tolerance.text_below, Some(("A↔B".as_bytes(), 17)));
}

#[test]
fn reads_composite_segment_magnitudes_from_the_text() {
    for (text, expected) in [(&b"\xe8\xe2\x8c\x800.3"[..], 0.3), (&b".2"[..], 0.2)] {
        let value = text_magnitude(text).expect("numeric segment magnitude");
        assert!((value - expected).abs() < VALUE_TOLERANCE);
    }
    assert!(text_magnitude(b".").is_none());
    assert!(text_magnitude(b"unknown").is_none());
}

#[test]
fn reads_datum_material_modifiers_from_labels() {
    assert_eq!(
        datum_part("EⓂ"),
        Some(("E", Some("maximum_material_requirement")))
    );
    assert_eq!(
        datum_part("HⓁ"),
        Some(("H", Some("least_material_requirement")))
    );
    assert_eq!(datum_part("A"), Some(("A", None)));
    assert_eq!(datum_part("A?"), None);
}
