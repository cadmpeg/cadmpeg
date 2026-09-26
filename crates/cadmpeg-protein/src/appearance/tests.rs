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

fn texture_for_test(
    record: &crate::DecodedRecord,
) -> Result<super::TextureAssetResult, cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    super::texture_asset(&ctx, record)
}

/// The three length tags of the Distance quantity class each convert to
/// the IR's millimetres. `0x200e` is millimetre, not centimetre.
#[test]
fn distance_tags_convert_to_millimetres() {
    for (unit, value, expected) in [(0x2016, 1.0, 25.4), (0x200e, 0.5, 0.5), (0x200d, 0.5, 5.0)] {
        let record = distance_record(unit, value);
        assert_eq!(
            super::distance_property(&record, "Depth")
                .map(|value| value.map(cadmpeg_ir::scalar::Length::get)),
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
fn unknown_texture_distance_unit_has_no_usable_texture_or_zero_scale() {
    let mut record = distance_record(0x0002_1008, 7.0);
    record.schema = "UnifiedBitmapSchema".into();
    record.properties = std::collections::BTreeMap::from([(
        "texture_RealWorldScaleX".into(),
        record
            .properties
            .remove("test_Depth")
            .expect("fixture has a distance"),
    )]);
    assert!(matches!(
        texture_for_test(&record),
        Ok(super::TextureAssetResult::UnknownDistanceUnit { count: 1 })
    ));
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
            let property = record
                .properties
                .remove("test_Depth")
                .expect("the synthetic record carries test_Depth");
            record.properties.insert(suffix.into(), property);
            assert!(texture_for_test(&record).is_err(), "{schema} {suffix}");
        }
    }
}

/// A texture record of `schema` that states the float `value` under `suffix`.
fn float_record(schema: &str, suffix: &str, value: f64) -> crate::DecodedRecord {
    let mut record = distance_record(0x200e, 1.0);
    record.schema = schema.into();
    record.properties = std::collections::BTreeMap::from([(
        format!("test_{suffix}"),
        crate::property::DecodedProperty {
            value_offset: 0,
            content: crate::property::PropertyContent::Value {
                value: crate::property::PropertyValue::Float(value),
                connections: Vec::new(),
            },
        },
    )]);
    record
}

/// The projection admits every mapping and bump float where it builds the
/// payload, so a non-finite one refuses the asset.
#[test]
fn a_non_finite_texture_float_property_is_refused() {
    for suffix in [
        "UOffset",
        "VOffset",
        "UScale",
        "VScale",
        "WAngle",
        "bumpmap_NormalScale",
    ] {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let record = float_record("BumpMapSchema", suffix, value);
            let Err(error) = texture_for_test(&record) else {
                panic!("{suffix} {value} is refused");
            };
            assert!(
                error
                    .to_string()
                    .contains(&format!("property {suffix} is non-finite")),
                "{suffix}: {error}"
            );
        }
        let finite = float_record("BumpMapSchema", suffix, 0.5);
        assert!(texture_for_test(&finite).is_ok(), "{suffix}");
    }
}
