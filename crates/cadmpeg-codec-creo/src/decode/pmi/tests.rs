// SPDX-License-Identifier: Apache-2.0

use super::{datum_part, text_magnitude};

const VALUE_TOLERANCE: f64 = 1.0e-12;

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
