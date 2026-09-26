// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::ids::UnknownId;
use crate::tessellation::{ChannelAddressing, TessellationChannel};
use crate::unknown::UnknownRecord;

fn assert_base64_round_trip_and_rejection<T>(value: &T, path: &[&str])
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut json = serde_json::to_value(value).unwrap();
    let mut at = &json;
    for step in path {
        at = &at[*step];
    }
    assert_eq!(*at, serde_json::json!("AQID"));
    assert_eq!(serde_json::from_value::<T>(json.clone()).unwrap(), *value);
    let mut at = &mut json;
    for step in path {
        at = &mut at[*step];
    }
    *at = serde_json::Value::String("%%%".into());
    assert!(serde_json::from_value::<T>(json).is_err());
}

#[test]
fn byte_payloads_use_nonempty_base64_and_reject_invalid_text() {
    assert_base64_round_trip_and_rejection(
        &UnknownRecord::retained(
            UnknownId::mint("synthetic:test:unknown#0").expect("valid identity"),
            0,
            vec![1, 2, 3],
            Vec::new(),
        ),
        &["retention", "data"],
    );
    assert_base64_round_trip_and_rejection(
        &TessellationChannel::new(ChannelAddressing::Vertex {}, 3, 0, 0, vec![1, 2, 3])
            .expect("valid channel"),
        &["data"],
    );
}

#[test]
fn native_base64_stream_refuses_retained_limit_and_preserves_json_bytes() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[derive(Serialize)]
    struct Record<'a> {
        id: &'static str,
        #[serde(serialize_with = "crate::bytes::serialize")]
        payload: &'a [u8],
    }

    let bytes = [0x5a; 769];
    let record = Record {
        id: "test:native:base64#1",
        payload: &bytes,
    };
    let expected = format!(
        "{{\"id\":\"test:native:base64#1\",\"payload\":\"{}\"}}",
        STANDARD.encode(&bytes)
    );
    assert_eq!(serde_json::to_string(&record).unwrap(), expected);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(expected.len()).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::native::arena_from(
        &limited,
        [Ok::<_, crate::native::NativeConvertError>(&record)],
    )
    .unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let stored = crate::native::arena_from(
        &service,
        [Ok::<_, crate::native::NativeConvertError>(&record)],
    )
    .unwrap();
    assert_eq!(serde_json::to_string(&stored[0]).unwrap(), expected);
}
