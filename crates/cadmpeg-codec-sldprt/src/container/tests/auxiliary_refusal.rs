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

fn weldment_part(payload: &[u8]) -> cadmpeg_ir::unknown::part::Part<'_> {
    cadmpeg_ir::unknown::part::Part::at("sldprt", "parasolid-streams", 0, payload, [])
}

#[test]
fn malformed_compound_owner_retains_exact_physical_payload() {
    let mut payload = entry();
    *payload.last_mut().unwrap() = 2;
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let mut given_up = Vec::new();
    let stream = crate::container::compound_stream(
        &ctx,
        "ThirdPty/WELDMENT_STORAGE",
        3,
        0,
        payload.clone(),
        None,
        &mut given_up,
    )
    .unwrap();
    assert_eq!(stream.payload, payload);
    assert!(stream.ps_streams.is_empty());
    let [part] = given_up.as_slice() else {
        panic!("the owner's Parasolid content is one given-up part");
    };
    assert_eq!(
        part.loss().code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(part
        .loss()
        .message
        .contains("\"ThirdPty/WELDMENT_STORAGE\""));
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
            &ctx,
            &payload,
            &cadmpeg_ir::stream_name!("ThirdPty/WELDMENT_STORAGE"),
            Some("ThirdPty/WELDMENT_STORAGE"),
            weldment_part(&payload),
            &mut Vec::new(),
        ),
        Err(CodecError::ResourceLimit(_))
    ));
}
