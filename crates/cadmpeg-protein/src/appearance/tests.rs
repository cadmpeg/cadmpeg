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
                    value: crate::property::PropertyValue::Distance {
                        unit,
                        value: cadmpeg_ir::scalar::FiniteReal::new(value)
                            .expect("finite source distance"),
                    },
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

fn distance_for_test(
    record: &crate::DecodedRecord,
) -> Result<Option<cadmpeg_ir::scalar::Length>, super::DistanceError> {
    let property = record
        .properties
        .get("test_Depth")
        .expect("fixture distance");
    super::distance_property(property.value())
}

/// The three length tags of the Distance quantity class each convert to
/// the IR's millimetres. `0x200e` is millimetre, not centimetre.
#[test]
fn distance_tags_convert_to_millimetres() {
    for (unit, value, expected) in [(0x2016, 1.0, 25.4), (0x200e, 0.5, 0.5), (0x200d, 0.5, 5.0)] {
        let record = distance_record(unit, value);
        assert_eq!(
            distance_for_test(&record).map(|value| value.map(cadmpeg_ir::scalar::Length::get)),
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
        record.properties["test_Depth"].value(),
        Some(&crate::property::PropertyValue::Distance {
            unit: 0x0002_1008,
            value: cadmpeg_ir::scalar::FiniteReal::ONE,
        })
    );
    assert_eq!(
        distance_for_test(&record),
        Err(super::DistanceError::UnknownUnit)
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
        let Some(value) = cadmpeg_ir::scalar::FiniteReal::new(value) else {
            assert!(
                !value.is_finite(),
                "source is refused before constructing a distance"
            );
            continue;
        };
        let record = distance_record(unit, value.get());
        assert_eq!(
            distance_for_test(&record),
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
                value: crate::property::PropertyValue::Float(
                    cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite source float"),
                ),
                connections: Vec::new(),
            },
        },
    )]);
    record
}

/// Mapping and bump floats refuse non-finite source values before construction.
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
            assert!(
                cadmpeg_ir::scalar::FiniteReal::new(value).is_none(),
                "{suffix} {value} is refused before constructing the property"
            );
        }
        let finite = float_record("BumpMapSchema", suffix, 0.5);
        assert!(texture_for_test(&finite).is_ok(), "{suffix}");
    }
}

#[test]
fn texture_projection_admits_work_before_searching_properties() {
    let mut record = distance_record(0x200e, 1.0);
    record.schema = "UnifiedBitmapSchema".into();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let Err(error) = super::texture_asset(&ctx, &record) else {
        panic!("search is admitted");
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn a_suffix_names_the_whole_property_or_an_underscore_qualified_one() {
    for (id, matches) in [
        ("UScale", true),
        ("texture_UScale", true),
        ("textureUScale", false),
        ("UScale_x", false),
    ] {
        let mut record = float_record("UnifiedBitmapSchema", "UScale", 2.0);
        let (_, property) = record.properties.pop_first().expect("float property");
        record.properties.insert(id.into(), property);
        let super::TextureAssetResult::Usable(texture) =
            texture_for_test(&record).expect("service admission")
        else {
            panic!("usable texture");
        };
        let value = texture.mapping.u_scale.get();
        assert_eq!(value, if matches { 2.0 } else { 1.0 }, "{id}");
    }
}

#[test]
fn overflowed_texture_distance_diagnostic_admits_asset_guid() {
    let mut record = distance_record(0x2016, 1.0e308);
    record.schema = "BumpMapSchema".into();
    record.guid = "asset".repeat(256);
    let property = record
        .properties
        .remove("test_Depth")
        .expect("fixture distance");
    record.properties.insert("bumpmap_Depth".into(), property);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
    let Err(error) = super::texture_asset(&ctx, &record) else {
        panic!("overflowed distance must refuse");
    };
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("diagnostic storage must be admitted");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert_eq!(limit.operation, "Protein malformed detail");
    assert_eq!(ctx.resource_refusal(), Some(limit));
    let Err(cadmpeg_core::CodecError::Malformed(detail)) = texture_for_test(&record) else {
        panic!("service profile preserves malformed distance");
    };
    assert_eq!(
        detail,
        format!(
            "Protein asset {} distance bumpmap_Depth is non-finite after millimetre conversion",
            record.guid
        )
    );
}

fn equality_texture() -> super::TextureAsset {
    let record = float_record("BumpMapSchema", "UScale", 1.0);
    let super::TextureAssetResult::Usable(mut texture) =
        texture_for_test(&record).expect("texture")
    else {
        panic!("usable texture");
    };
    texture.asset_guid = "guid".into();
    texture.paths = vec!["path".repeat(256), "uri".repeat(512)];
    texture.urn = Some("urn".repeat(256));
    texture
}

#[test]
fn admitted_texture_equality_checks_every_field() {
    let expected = equality_texture();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("service context");
    assert!(expected
        .equal_for_decode(&ctx, &expected, "texture equality")
        .expect("identical"));
    for field in 0..8 {
        let mut other = expected.clone();
        match field {
            0 => other.asset_guid.push('x'),
            1 => other.schema.push('x'),
            2 => other.paths[0].push('x'),
            3 => other.paths.push("another".into()),
            4 => other.urn.as_mut().expect("URN").push('x'),
            5 => other.urn = None,
            6 => other.mapping.repeat_u = !other.mapping.repeat_u,
            7 => other.bump.as_mut().expect("bump").normal_map = true,
            _ => unreachable!(),
        }
        assert!(expected != other);
        assert!(
            !expected
                .equal_for_decode(&ctx, &other, "texture equality")
                .expect("unequal"),
            "field {field}"
        );
    }
}

#[test]
fn admitted_texture_equality_refuses_before_visiting_long_paths() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let texture = equality_texture();
    // Each equal text pair visits its byte length, and each path slot plus
    // the path iterator's end probe costs one step. Fixed fields are free.
    let work = cadmpeg_core::decode::u64_from_index(
        texture.asset_guid.len()
            + texture.schema.len()
            + texture.urn.as_ref().expect("URN").len()
            + texture.paths.iter().map(String::len).sum::<usize>()
            + texture.paths.len()
            + 1,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
    assert!(texture
        .equal_for_decode(&ctx, &texture, "texture equality")
        .expect("exact comparison work"));
    assert!(matches!(ctx.charge_work(1, "after exact equality"),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits && limit.used == work));
    policy.limits.max_work_units = work - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
    let error = texture
        .equal_for_decode(&ctx, &texture, "texture equality")
        .expect_err("end probe cannot fit");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal expected");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "texture equality");
    assert_eq!(limit.used, work - 1);
    assert_eq!(limit.additional, 1);
}

#[test]
fn admitted_texture_equality_skips_unvisited_paths() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let texture = equality_texture();
    let mut other = texture.clone();
    other.paths[0].replace_range(..1, "x");
    let work = cadmpeg_core::decode::u64_from_index(
        texture.asset_guid.len()
            + texture.schema.len()
            + texture.urn.as_ref().expect("URN").len()
            + 1
            + 1,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
    assert!(!texture
        .equal_for_decode(&ctx, &other, "early texture mismatch")
        .expect("only first path byte is visited"));
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
    other.paths.clear();
    assert!(!texture
        .equal_for_decode(&ctx, &other, "unequal path counts")
        .expect("collection lengths are constant work"));
}

#[test]
fn texture_projection_preserves_first_named_and_first_usable_fields() {
    use crate::property::{DecodedProperty, PropertyContent, PropertyValue};
    let mut record = float_record("UnifiedBitmapSchema", "UScale", 7.0);
    let field = |value| DecodedProperty {
        value_offset: 0,
        content: PropertyContent::Value {
            value,
            connections: Vec::new(),
        },
    };
    record
        .properties
        .insert("a_UScale".into(), field(PropertyValue::Boolean(false)));
    record.properties.insert(
        "a_Bitmap".into(),
        field(PropertyValue::String("wrong carrier".into())),
    );
    record.properties.insert(
        "b_Bitmap".into(),
        field(PropertyValue::TextureUri(vec!["first path".into()])),
    );
    record.properties.insert(
        "c_Bitmap".into(),
        field(PropertyValue::TextureUri(vec!["later path".into()])),
    );
    record.properties.insert(
        "a_Bitmap_urn".into(),
        field(PropertyValue::String(String::new())),
    );
    record.properties.insert(
        "b_Bitmap_urn".into(),
        field(PropertyValue::String("first urn".into())),
    );
    record.properties.insert(
        "c_Bitmap_urn".into(),
        field(PropertyValue::String("later urn".into())),
    );
    let super::TextureAssetResult::Usable(texture) = texture_for_test(&record).expect("projection")
    else {
        panic!("usable texture");
    };
    assert_eq!(texture.mapping.u_scale, cadmpeg_ir::scalar::FiniteReal::ONE);
    assert_eq!(texture.paths, ["first path"]);
    assert_eq!(texture.urn.as_deref(), Some("first urn"));
}

#[test]
fn texture_projection_visits_the_property_map_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut record = float_record("UnifiedBitmapSchema", "UScale", 7.0);
    let (_, property) = record.properties.pop_first().expect("fixture property");
    for index in 0..64 {
        record
            .properties
            .insert(format!("{index:03}{}", "a".repeat(1024)), property.clone());
    }
    // One complete map visit per property; literal suffix grammar is fixed.
    // Only the schema string is copied because GUID, paths and URN are absent.
    let work = cadmpeg_core::decode::u64_from_index(record.properties.len() + record.schema.len());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
    let super::TextureAssetResult::Usable(texture) = super::texture_asset(&ctx, &record)
        .expect("single traversal and retained fields fit exactly")
    else {
        panic!("usable texture");
    };
    assert_eq!(texture.mapping.u_scale, cadmpeg_ir::scalar::FiniteReal::ONE);
    assert!(matches!(ctx.charge_work(1, "after exact projection"),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits && limit.used == work));
    policy.limits.max_work_units = 63;
    record.schema = "UnifiedBitmapSchema".into();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
    assert!(matches!(super::texture_asset(&ctx, &record),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits
            && limit.used == 0 && limit.additional == 64 && limit.operation == "Protein texture property selection"));
}
