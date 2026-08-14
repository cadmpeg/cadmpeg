// SPDX-License-Identifier: Apache-2.0
//! Material-payload fixtures for crate tests.
#![allow(clippy::unwrap_used)]

use super::container::{make_block, sldprt_with_body};

pub(crate) fn sldprt_with_body_and_material(body: &[u8], name: &str, rgb: [u8; 3]) -> Vec<u8> {
    let mut f = sldprt_with_body(body);
    f.extend(make_block(0x40, "SWObjects", &material_payload(name, rgb)));
    f
}

pub(crate) fn material_payload(name: &str, rgb: [u8; 3]) -> Vec<u8> {
    let mut material = b"moVisualProperties_c".to_vec();
    material.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0]);
    material.extend_from_slice(&0u32.to_le_bytes());
    material.extend_from_slice(&0x00c0_c0c0u32.to_le_bytes());
    material.extend_from_slice(&[0xff, 0xfe, 0xff, 0x00]);
    material.extend_from_slice(&[0xff, 0xfe, 0xff, name.len() as u8]);
    for unit in name.encode_utf16() {
        material.extend_from_slice(&unit.to_le_bytes());
    }
    material
}
