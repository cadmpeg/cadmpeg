// SPDX-License-Identifier: Apache-2.0

use super::NativeBytes;
use crate::native::{test_ctx, NativeRecord};

#[test]
fn hexadecimal_payload_round_trips_every_byte() {
    let bytes = NativeBytes::from((0..=u8::MAX).collect::<Vec<_>>());
    let json = serde_json::to_string(&bytes).unwrap();
    assert_eq!(json.len(), 514);
    assert!(json[1..json.len() - 1]
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert_eq!(serde_json::from_str::<NativeBytes>(&json).unwrap(), bytes);
    assert_eq!(
        serde_json::to_string(&NativeBytes::from(&[0, 15, 255][..])).unwrap(),
        "\"000fff\""
    );
    assert_eq!(
        serde_json::from_str::<NativeBytes>("\"ABcd\"")
            .unwrap()
            .as_ref(),
        &[0xab, 0xcd]
    );
}

#[test]
fn hexadecimal_payload_rejects_other_wire_forms() {
    for json in [
        "[]", "[0,255]", "null", "0", "{}", "true", "\"0\"", "\"gg\"", "\"é\"", "\"  \"",
        "\"0x00\"",
    ] {
        assert!(serde_json::from_str::<NativeBytes>(json).is_err(), "{json}");
    }
    assert_eq!(
        serde_json::from_str::<NativeBytes>("\"\"")
            .unwrap()
            .as_ref(),
        [0_u8; 0].as_slice()
    );
    assert_eq!(
        serde_json::from_str::<NativeBytes<[u8; 2]>>("\"00ff\"")
            .unwrap()
            .into_inner(),
        [0, 255]
    );
    assert!(serde_json::from_str::<NativeBytes<[u8; 2]>>("\"00\"").is_err());
}

#[test]
fn canonical_payload_is_a_string() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'a str,
        bytes: NativeBytes<&'a [u8]>,
    }
    let bytes = [0, 15, 255];
    let record = NativeRecord::from_typed_for_decode(
        &test_ctx(),
        &Record {
            id: "test:native:bytes#1",
            bytes: NativeBytes::from(bytes.as_slice()),
        },
        None,
    )
    .unwrap();
    assert_eq!(record.field("bytes").unwrap(), "000fff");
}

#[test]
fn canonical_payload_uses_string_storage_and_no_byte_slots() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        bytes: NativeBytes<&'a [u8]>,
    }
    let payload = [0x5a; 4096];
    let record = Record {
        id: "test:native:bytes#1",
        bytes: NativeBytes::from(payload.as_slice()),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 8;
    policy.limits.max_retained_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let stored = NativeRecord::from_typed_for_decode(&ctx, &record, None).unwrap();
    let text = stored.field("bytes").unwrap();
    assert_eq!(text.as_str().unwrap(), "5a".repeat(payload.len()));
    ctx.finish_session().unwrap();

    let arena = DecodeArena::new();
    policy.limits.max_retained_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(NativeRecord::from_typed_for_decode(&ctx, &record, None).is_err());
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
    );
}

#[test]
fn reconstructed_byte_lane_streams_as_one_string() {
    let bytes = (0..=u8::MAX).cycle().take(1025);
    let wire = serde_json::to_string(&NativeBytes::iter_wire(bytes.clone())).unwrap();
    let decoded = serde_json::from_str::<NativeBytes>(&wire).unwrap();
    assert_eq!(decoded.into_inner(), bytes.collect::<Vec<_>>());
    assert_eq!(
        serde_json::to_string(&NativeBytes::iter_wire(std::iter::empty())).unwrap(),
        "\"\""
    );
}
