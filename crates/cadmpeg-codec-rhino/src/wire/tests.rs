// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use super::{read_finite, Uuid};
use crate::chunks::{BoundedReader, FramingError};

#[test]
fn canonical_json_sorts_nested_keys() {
    #[derive(serde::Serialize)]
    struct Nested {
        z: u32,
        a: u32,
    }
    #[derive(serde::Serialize)]
    struct Root {
        z: Nested,
        a: u32,
    }
    let value = Root {
        z: Nested { z: 2, a: 1 },
        a: 3,
    };
    let service = cadmpeg_test_support::service_decode_context();
    let text = super::admitted_canonical_json(&service, &value, "Rhino canonical JSON")
        .expect("service policy admits JSON");
    assert_eq!(text, r#"{"a":3,"z":{"a":1,"z":2}}"#);
}

/// A non-finite value is refused at its own first byte, not after the read.
#[test]
fn read_finite_refuses_a_nonfinite_value_at_its_first_byte() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut bytes = vec![0xa5, 0xa5, 0xa5];
    let value_offset = bytes.len();
    bytes.extend(f64::NAN.to_le_bytes());
    bytes.extend(1.5_f64.to_le_bytes());
    let mut reader = BoundedReader::new(&bytes, 3, bytes.len()).expect("bounded reader");
    let error = read_finite(&ctx, &mut reader, "witness").expect_err("nonfinite value");
    assert_eq!(
        error,
        FramingError::structural(value_offset, "witness is not finite")
    );

    let mut reader = BoundedReader::new(&bytes, 11, bytes.len()).expect("bounded reader");
    assert_eq!(
        read_finite(&ctx, &mut reader, "witness"),
        Ok(crate::test_support::finite(1.5))
    );
}

/// `to_wire` inverts `from_wire` on the mixed-endian group transposition.
#[test]
fn wire_and_canonical_forms_round_trip() {
    let canonical = Uuid::from_canonical([
        0x05, 0x59, 0x73, 0x3b, 0x53, 0x32, 0x49, 0xd1, 0xa9, 0x36, 0x05, 0x32, 0xac, 0x76, 0xad,
        0xe5,
    ]);
    let wire = canonical.to_wire();
    assert_eq!(
        wire,
        [
            0x3b, 0x73, 0x59, 0x05, 0x32, 0x53, 0xd1, 0x49, 0xa9, 0x36, 0x05, 0x32, 0xac, 0x76,
            0xad, 0xe5,
        ]
    );
    assert_eq!(Uuid::from_wire(wire), canonical);
}

#[test]
fn parses_mixed_endian_uuid_and_nil_uuid() {
    let uuid = Uuid::from_wire([
        0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    assert_eq!(uuid.to_string(), "4ed7d4dd-e947-11d3-bfe5-0010830122f0");
    assert!(!uuid.is_nil());
    assert!(Uuid::nil().is_nil());
    assert_eq!(
        Uuid::nil().to_string(),
        "00000000-0000-0000-0000-000000000000"
    );
}

/// A non-finite coordinate and an overflowing product are both refused, so the
/// single product test covers every non-finite input.
#[test]
fn scaled_coordinate_refuses_nonfinite_inputs_and_overflowing_products() {
    let scale = crate::test_support::millimeter_scale(25.4);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(super::scaled_coordinate(value, scale), None, "{value}");
    }
    assert_eq!(
        super::scaled_coordinate(2.0, scale),
        Some(crate::test_support::finite(50.8))
    );

    let huge = crate::test_support::millimeter_scale(f64::MAX);
    assert_eq!(super::scaled_coordinate(f64::MAX, huge), None);
}

#[test]
fn loss_message_retained_bytes_are_charged_once() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // Seven bytes hold one retained loss message.
    policy.limits.max_retained_bytes = 7;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let note = super::admitted_loss(
        &ctx,
        crate::loss::RhinoLossCode::IntegrityFailure,
        format_args!("warning"),
        "test loss message",
    )
    .expect("one message fits");
    assert_eq!(note.message, "warning");
    let error = ctx
        .charge_retained(1, "test next message byte")
        .expect_err("message fills the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.used == 7 && limit.additional == 1)
    );
}

#[test]
fn canonical_json_preserves_ordinary_raw_value_named_keys() {
    let value = serde_json::json!({
        "$serde_json::private::RawValue": "not-json",
        "nested": {"$serde_json::private::RawValue": "[0]"},
    });
    let ctx = cadmpeg_test_support::service_decode_context();
    let text = super::admitted_canonical_json(&ctx, &value, "canonical raw-named key")
        .expect("the keys are ordinary object keys");
    assert_eq!(
        text,
        r#"{"$serde_json::private::RawValue":"not-json","nested":{"$serde_json::private::RawValue":"[0]"}}"#
    );
}

#[test]
fn canonical_json_keeps_the_last_duplicate_key() {
    struct DuplicateKeys;
    impl serde::Serialize for DuplicateKeys {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(3))?;
            map.serialize_entry("z", &9)?;
            map.serialize_entry("a", &1)?;
            map.serialize_entry("a", &2)?;
            map.end()
        }
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::admitted_canonical_json(&ctx, &DuplicateKeys, "duplicate JSON").unwrap(),
        r#"{"a":2,"z":9}"#
    );
}

#[test]
fn fixed_finite_field_error_requires_no_work_units() {
    let data = f64::NAN.to_le_bytes();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&data, 0, data.len()).unwrap();
    let error = read_finite(&ctx, &mut reader, "plot weight").unwrap_err();
    assert!(
        matches!(error, FramingError::Structural { offset: 0, message }
        if message == "plot weight is not finite")
    );
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}
