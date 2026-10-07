// SPDX-License-Identifier: Apache-2.0
use super::super::NativeRecord;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use serde::Serialize;

#[test]
fn borrowed_comparison_checks_large_payloads_without_cloning_them() {
    #[derive(Serialize)]
    struct Record<'a> {
        id: &'a str,
        data: &'a [u8],
        text: &'a str,
    }
    let mut data = vec![123; 100_000];
    let expected = Record {
        id: "test:native:record#payload",
        data: &data,
        text: "escape \" ☃",
    };
    let stored = NativeRecord::from_typed(&expected).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 100;
    policy.limits.max_work_units = 500_000;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert!(stored.matches_typed_for_decode(&ctx, &expected).unwrap());
    assert!(ctx.finish_session().is_ok());
    data[99_999] = 124;
    let changed = Record {
        id: "test:native:record#payload",
        data: &data,
        text: "escape \" ☃",
    };
    assert!(!stored
        .matches_typed_for_decode(&super::super::test_ctx(), &changed)
        .unwrap());
}

#[test]
fn borrowed_comparison_preserves_every_json_shape_and_rejects_drift() {
    #[derive(Serialize)]
    enum Variant {
        Unit,
        New(u64),
        Tuple(u8, bool),
        Struct { text: &'static str },
    }
    #[derive(Serialize)]
    struct Record<'a> {
        id: &'a str,
        variants: [Variant; 4],
        maybe: Option<u32>,
        raw: &'a serde_json::value::RawValue,
        map: std::collections::BTreeMap<i32, bool>,
    }
    let raw =
        serde_json::value::RawValue::from_string("{\"payload\":[1,null,\"x\"]}".into()).unwrap();
    let expected = Record {
        id: "test:native:record#shapes",
        variants: [
            Variant::Unit,
            Variant::New(7),
            Variant::Tuple(3, true),
            Variant::Struct { text: "word" },
        ],
        maybe: None,
        raw: &raw,
        map: [(-3, true), (7, false)].into(),
    };
    let stored = NativeRecord::from_typed(&expected).unwrap();
    assert!(stored
        .matches_typed_for_decode(&super::super::test_ctx(), &expected)
        .unwrap());
    let json = serde_json::to_value(&stored).unwrap();
    for changed in [
        {
            let mut value = json.clone();
            value.as_object_mut().unwrap().remove("maybe");
            value
        },
        {
            let mut value = json.clone();
            value["extra"] = serde_json::json!(1);
            value
        },
        {
            let mut value = json.clone();
            value["variants"][2]["Tuple"][1] = serde_json::json!(false);
            value
        },
        {
            let mut value = json.clone();
            value["map"]["7"] = serde_json::json!(true);
            value
        },
    ] {
        let changed: NativeRecord = serde_json::from_value(changed).unwrap();
        assert!(!changed
            .matches_typed_for_decode(&super::super::test_ctx(), &expected)
            .unwrap());
    }
}

#[test]
fn borrowed_comparison_preserves_refusals_and_rejects_nonfinite_expected_numbers() {
    #[derive(Serialize)]
    struct Nonfinite<'a> {
        id: &'a str,
        value: f64,
    }

    let expected = serde_json::json!({ "id": "test:native:record#limits", "data": [1,2,3] });
    let stored: NativeRecord = serde_json::from_value(expected.clone()).unwrap();
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let super::super::NativeConvertError::Resource(cadmpeg_core::CodecError::ResourceLimit(
            first,
        )) = stored
            .matches_typed_for_decode(&ctx, &expected)
            .unwrap_err()
        else {
            panic!("resource refusal")
        };
        assert_eq!(first.dimension, dimension);
        assert_eq!(ctx.resource_refusal(), Some(first));
        assert!(stored.matches_typed_for_decode(&ctx, &expected).is_err());
    }
    assert!(stored
        .matches_typed_for_decode(
            &super::super::test_ctx(),
            &Nonfinite {
                id: "test:native:record#limits",
                value: f64::NAN
            }
        )
        .is_err());
}

#[test]
fn borrowed_comparison_handles_raw_record_roots_and_rejects_duplicate_keys() {
    struct Duplicate;
    impl Serialize for Duplicate {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("id", "test:native:record#raw")?;
            map.serialize_entry("id", "test:native:record#raw")?;
            map.end()
        }
    }

    let raw = serde_json::value::RawValue::from_string(
        r#"{"id":"test:native:record#raw","data":[1,2,3]}"#.into(),
    )
    .unwrap();
    let stored = NativeRecord::from_typed(&raw).unwrap();
    assert!(stored
        .matches_typed_for_decode(&super::super::test_ctx(), &raw)
        .unwrap());
    assert!(stored
        .matches_typed_for_decode(&super::super::test_ctx(), &Duplicate)
        .is_err());
}

#[test]
fn borrowed_comparison_enforces_native_depth_even_when_the_actual_record_differs() {
    let stored = NativeRecord::from_typed(&serde_json::json!({
        "id": "test:native:record#depth", "data": null,
    }))
    .unwrap();
    let mut data = serde_json::Value::Null;
    for _ in 0..=super::super::MAX_NATIVE_NESTING_DEPTH {
        data = serde_json::Value::Array(vec![data]);
    }
    let expected = serde_json::json!({ "id": stored.id(), "data": data });
    assert!(stored
        .matches_typed_for_decode(&super::super::test_ctx(), &expected)
        .is_err());
}
