// SPDX-License-Identifier: Apache-2.0
//! Direct auxiliary stream bounds in a compound member.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn compound_weldment_member_preserves_outer_bytes_but_bounds_its_xt_entry() {
    let xt = crate::test_support::parasolid::parasolid_with_body(
        "auxiliary XT",
        "SCH_2900195_28101_13006",
        &[0, 1, 0, 1],
    );
    let mut payload = 1_u32.to_le_bytes().to_vec();
    payload.extend(u32::try_from(xt.len()).unwrap().to_le_bytes());
    payload.extend(&xt);
    payload.extend([0x5a; 19]);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let stream = super::super::compound_stream(
        &ctx,
        "ThirdPty/WELDMENT_STORAGE".to_owned(),
        7,
        0,
        payload.clone(),
        None,
    )
    .unwrap();
    assert_eq!(stream.payload, payload);
    assert_eq!(stream.ps_streams.len(), 1);
    assert_eq!(stream.ps_streams[0].offset, 8);
    assert_eq!(stream.ps_streams[0].payload, xt);
}
