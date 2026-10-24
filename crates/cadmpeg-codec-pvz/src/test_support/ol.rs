// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.ol` geometry members built from the specification grammar.

use super::gzip;
use crate::layout::{ol_preamble_block, token};

/// One TLV record (§4.4).
pub(crate) fn tlv(tag: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_le_bytes().to_vec();
    bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// The stream index record of stream `id` (§4.5).
pub(crate) fn stream_index(id: u32) -> Vec<u8> {
    tlv(token::STREAM_EDGE_TESSELLATION, &(id & 0xff).to_le_bytes())
}

fn f32s(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn u32s(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// An appearance-values record (§5.1): base, diffuse, specular RGB,
/// shininess, emissive RGB, opacity.
pub(crate) fn appearance_values(values: [f32; 14]) -> Vec<u8> {
    tlv(5, &f32s(&values))
}

/// An appearance reference (§5.2) with a 16-byte padded name.
pub(crate) fn appearance_reference(name: &str, base_index: u32) -> Vec<u8> {
    let mut padded = name.as_bytes().to_vec();
    padded.resize(16, 0);
    let mut payload = u32s(&[16]);
    payload.extend(padded);
    payload.extend(u32s(&[base_index, 0xffff_ffff]));
    payload.push(0);
    payload.extend([0xff; 12]);
    tlv(7, &payload)
}

/// A patch header (§6.2) with one reference.
pub(crate) fn patch_header(appearance: u32, face_ref: u32) -> Vec<u8> {
    let mut payload = u32s(&[appearance, face_ref]);
    payload.push(0);
    payload.extend(u32s(&[1, face_ref]));
    tlv(token::TESS_PATCH_HEADER, &payload)
}

/// A vertex block (§6.3) of positions, with normals when given.
pub(crate) fn vertex_block(positions: &[[f32; 3]], normals: Option<&[[f32; 3]]>) -> Vec<u8> {
    let mut payload = vec![u8::from(normals.is_some())];
    payload.extend(u32s(&[u32::try_from(positions.len()).unwrap()]));
    for (index, position) in positions.iter().enumerate() {
        payload.extend(f32s(position));
        if let Some(normals) = normals {
            payload.extend(f32s(&normals[index]));
        }
    }
    tlv(token::TESS_VERTEX_BLOCK, &payload)
}

/// A primitive record of `tag` over `indexes` (§6.4, §7.2).
pub(crate) fn primitive(tag: u32, indexes: &[u32]) -> Vec<u8> {
    tlv(tag, &u32s(indexes))
}

/// An edge header (§7.2) for exact edge `edge`.
pub(crate) fn edge_header(edge: u32) -> Vec<u8> {
    let mut payload = u32s(&[edge, 0]);
    payload.extend(f32s(&[1.0]));
    tlv(token::EXACT_SINGULAR_USE, &payload)
}

/// A geometry member: the header, the directory, the preamble block when
/// `preamble` is set, and one GZIP payload per `(stream_id, records)` stream
/// in ascending identity order (§4.1–§4.3).
pub(crate) fn member(signature: &str, preamble: bool, streams: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let payloads: Vec<Vec<u8>> = streams.iter().map(|(_, plain)| gzip(plain)).collect();
    let entries = streams.len() + 1;
    let directory_end = 22 + 8 * entries;
    let mut offset = directory_end + if preamble { 19 } else { 0 };
    let mut bytes = signature.as_bytes().to_vec();
    bytes.extend(u32s(&[1, u32::try_from(4 + 8 * entries).unwrap()]));
    bytes.extend(u32s(&[u32::try_from(entries).unwrap()]));
    for ((id, _), payload) in streams.iter().zip(&payloads) {
        bytes.extend(u32s(&[*id, u32::try_from(offset).unwrap()]));
        offset += payload.len();
    }
    bytes.extend(u32s(&[
        token::STREAM_END_SENTINEL,
        u32::try_from(offset).unwrap(),
    ]));
    if preamble {
        bytes.extend(u32s(&[ol_preamble_block::TAG_VALUE, 3]));
        bytes.extend_from_slice(&[0x03, signature.as_bytes()[7] - b'0', 0x01]);
        bytes.extend_from_slice(&[0; 8]);
    }
    for payload in payloads {
        bytes.extend(payload);
    }
    bytes
}
