// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn construction() -> DesignThreadConstruction {
    serde_json::from_value(serde_json::json!({
        "form": "standard_extended", "designation_offset": 100,
        "designation": "M10x1.5", "nominal_size_text": "10", "nominal_size": 10.0,
        "profile": "ISO Metric profile", "major_diameter": 1.0,
        "minor_diameter": 0.8, "pitch_diameter": 0.9, "pitch": 0.15,
        "modeled": true, "face_group_record_indices": [10],
        "extended": {"class": "6g", "external": true,
            "full_length": true, "right_handed": false}
    }))
    .unwrap()
}

#[test]
fn thread_spec_carries_class_hand_and_physical_diameters() {
    let construction = construction();
    let projected = spec(&construction, Some(&CosmeticThreadExtent::Through {}));
    let standard = projected.standard.unwrap();
    assert_eq!(standard.standard.as_str(), "ISO Metric profile");
    assert_eq!(
        standard.size,
        Some(Dimension::Literal(construction.designation))
    );
    assert_eq!(
        standard.class.as_ref().map(NonBlankString::as_str),
        Some("6g")
    );
    assert_eq!(projected.hand, Some(Hand::Left));
    for (actual, millimetres) in [
        (projected.major_diameter, 10.0),
        (projected.minor_diameter, 8.0),
        (projected.pitch_diameter, 9.0),
        (projected.pitch, 1.5),
    ] {
        assert_eq!(
            actual,
            PositiveLength::new(millimetres).map(Dimension::Literal)
        );
    }
    assert_eq!(projected.modeled, Some(Dimension::Literal(true)));
    assert_eq!(projected.length, Some(ThreadLength::Full));
}

#[test]
fn absent_extended_fields_stay_absent_and_blind_length_stays_literal() {
    let mut construction = construction();
    construction.extended = None;
    let length = PositiveLength::new(12.0).unwrap();
    let projected = spec(&construction, Some(&CosmeticThreadExtent::Blind { length }));
    assert_eq!(projected.hand, None);
    assert_eq!(projected.standard.unwrap().class, None);
    assert_eq!(
        projected.length,
        Some(ThreadLength::Blind(Dimension::Literal(length)))
    );
    assert_eq!(spec(&construction, None).length, None);
}

#[test]
fn compact_thread_class_comes_from_its_fixed_prefix() {
    let mut construction = construction();
    construction.extended = None;
    for form in [
        crate::records::feature::thread::DesignThreadForm::Compact(None),
        crate::records::feature::thread::DesignThreadForm::CompactOwnersLeading,
        crate::records::feature::thread::DesignThreadForm::CompactOwnersTrailing,
    ] {
        construction.form = form;
        let projected = spec(&construction, None);
        assert_eq!(
            projected
                .standard
                .unwrap()
                .class
                .as_ref()
                .map(NonBlankString::as_str),
            Some("6H")
        );
        assert_eq!(projected.hand, None);
    }
}
