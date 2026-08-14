// SPDX-License-Identifier: Apache-2.0
//! Synthetic display-list tessellation payloads for crate tests.
#![allow(clippy::unwrap_used)]

use super::{make_block, sldprt_with_body};

pub(crate) fn display_list_payload() -> Vec<u8> {
    fn descriptor(item_size: u32, kind: u32, count: u32, data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&item_size.to_le_bytes());
        b.extend_from_slice(&kind.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&count.to_le_bytes());
        b.extend_from_slice(data);
        b
    }
    let mut b = b"uoTempBodyTessData_c".to_vec();
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(b"uoTempFaceTessData_c");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend(descriptor(4, 8, 1, &3u32.to_le_bytes()));
    let mut positions = Vec::new();
    for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        positions.extend_from_slice(&value.to_le_bytes());
    }
    b.extend(descriptor(12, 100, 3, &positions));
    b.extend(descriptor(12, 100, 3, &[0u8; 36]));
    b.extend(descriptor(4, 8, 4, &[0; 16]));
    b.extend(descriptor(4, 8, 1, &4u32.to_le_bytes()));
    b.extend(descriptor(1, 8, 4, &[0; 4]));
    b
}

pub(crate) fn extended_display_list_payload() -> Vec<u8> {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let extension = [1_u32, 0, 0, 0x0020_1296, 0, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let at = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .expect("face tessellation class")
        + marker.len()
        + 8;
    payload.splice(at..at, extension);
    payload
}

pub(crate) fn sldprt_with_body_and_display_list(body: &[u8]) -> Vec<u8> {
    let mut f = sldprt_with_body(body);
    f.extend(make_block(
        0x41,
        "Contents/DisplayLists",
        &display_list_payload(),
    ));
    f
}
