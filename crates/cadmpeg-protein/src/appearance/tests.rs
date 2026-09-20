// SPDX-License-Identifier: Apache-2.0

fn distance_record(unit: u32, value: f64) -> crate::DecodedRecord {
    crate::DecodedRecord {
        ordinal: 0,
        logical_offset: 0,
        schema: "TestSchema".into(),
        guid: String::new(),
        base: String::new(),
        asset_lib_id: String::new(),
        properties: std::collections::BTreeMap::from([(
            "test_Depth".to_owned(),
            crate::property::DecodedProperty {
                value_offset: 0,
                content: crate::property::PropertyContent::Value {
                    value: crate::property::PropertyValue::Distance { unit, value },
                    connections: Vec::new(),
                },
            },
        )]),
    }
}

/// The three length tags of the Distance quantity class each convert to
/// the IR's millimetres. `0x200e` is millimetre, not centimetre.
#[test]
fn distance_tags_convert_to_millimetres() {
    for (unit, value, expected) in [(0x2016, 1.0, 25.4), (0x200e, 0.5, 0.5), (0x200d, 0.5, 5.0)] {
        let record = distance_record(unit, value);
        assert_eq!(
            super::distance_property(&record, "Depth"),
            Ok(Some(expected))
        );
    }
}

/// A Distance whose tag names a quantity other than length has no
/// millimetre reading and must not be silently taken as one.
#[test]
fn a_non_length_distance_tag_yields_no_value() {
    let record = distance_record(0x0002_1008, 1.0);
    assert_eq!(super::distance_property(&record, "Depth"), Err(0x0002_1008));
}
