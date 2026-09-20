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
    assert_eq!(
        super::distance_property(&record, "Depth"),
        Err(super::DistanceError::UnknownUnit(0x0002_1008))
    );
}

#[test]
fn numerical_audit_distance_conversion_rejects_nonfinite_results() {
    for (unit, value) in [
        (0x2016, 1.0e308),
        (0x200d, f64::MAX),
        (0x200e, f64::INFINITY),
        (0x200e, f64::NAN),
    ] {
        let record = distance_record(unit, value);
        assert_eq!(
            super::distance_property(&record, "Depth"),
            Err(super::DistanceError::NonFinite)
        );
    }
    for schema in ["UnifiedBitmapSchema", "BumpMapSchema"] {
        for suffix in [
            "RealWorldOffsetX",
            "RealWorldOffsetY",
            "RealWorldScaleX",
            "RealWorldScaleY",
            "bumpmap_Depth",
        ] {
            if schema != "BumpMapSchema" && suffix == "bumpmap_Depth" {
                continue;
            }
            let mut record = distance_record(0x2016, 1.0e308);
            record.schema = schema.into();
            let property = record.properties.remove("test_Depth").unwrap();
            record.properties.insert(suffix.into(), property);
            assert!(super::texture_asset(&record).is_err(), "{schema} {suffix}");
        }
    }
}
