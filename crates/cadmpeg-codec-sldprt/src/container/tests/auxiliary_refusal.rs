// SPDX-License-Identifier: Apache-2.0
use crate::test_support::parasolid::parasolid_with_body;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn entry() -> Vec<u8> {
    let xt = parasolid_with_body("auxiliary XT", "SCH_2900195_28101_13006", &[0, 1, 0, 1]);
    let mut result = 1u32.to_le_bytes().to_vec();
    result.extend_from_slice(&u32::try_from(xt.len()).unwrap().to_le_bytes());
    result.extend(xt);
    result
}

#[test]
fn malformed_compound_owner_retains_exact_physical_payload() {
    let mut payload = entry();
    *payload.last_mut().unwrap() = 2;
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let stream = crate::container::compound_stream(
        &ctx,
        "ThirdPty/WELDMENT_STORAGE".to_owned(),
        3,
        0,
        payload.clone(),
        None,
    )
    .unwrap();
    assert_eq!(stream.payload, payload);
    assert!(stream.ps_auxiliary_refusal.is_some());
    assert!(stream.ps_streams.is_empty());
}

#[test]
fn optional_owner_does_not_swallow_retention_resource_limits() {
    let payload = entry();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    assert!(matches!(
        crate::container::extract_streams_for_owner(
            &payload,
            Some("ThirdPty/WELDMENT_STORAGE"),
            &ctx
        ),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn malformed_nonoptional_owner_still_fails() {
    let error = CodecError::Malformed("owned malformed stream".to_owned());
    let result = crate::container::contain_optional_parasolid_refusal(
        Some("Contents/Config-0-Partition"),
        Err(error),
    );
    assert!(
        matches!(result, Err(CodecError::Malformed(message)) if message == "owned malformed stream")
    );
    let error = CodecError::NotImplemented("unsupported transport".to_owned());
    let result = crate::container::contain_optional_parasolid_refusal(
        Some("ThirdPty/WELDMENT_STORAGE"),
        Err(error),
    );
    assert!(
        matches!(result, Err(CodecError::NotImplemented(message)) if message == "unsupported transport")
    );
}
