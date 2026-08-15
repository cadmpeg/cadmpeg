// SPDX-License-Identifier: Apache-2.0
//! Synthetic outer-container byte builders for crate tests.
#![allow(clippy::unwrap_used)]

use std::io::Write;

use crate::container::MARKER;

use super::parasolid::{owned_triangle, parasolid_payload, parasolid_with_body};

/// Nibble-swap a section name into its stored form (the swap is its own inverse,
/// so the decoder recovers the original).
pub(crate) fn swap_name(name: &str) -> Vec<u8> {
    name.bytes().map(|b| b.rotate_left(4)).collect()
}

pub(crate) fn raw_deflate(data: &[u8]) -> Vec<u8> {
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

pub(crate) fn zlib(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// Assemble one CRC-validated block frame carrying `payload`, named `section`.
pub(crate) fn make_block(type_id: u32, section: &str, payload: &[u8]) -> Vec<u8> {
    let comp = raw_deflate(payload);
    let preamble = swap_name(section);
    let mut b = Vec::new();
    b.extend_from_slice(&MARKER);
    b.extend_from_slice(&type_id.to_le_bytes());
    b.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
    b.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    b.extend_from_slice(&(preamble.len() as u32).to_le_bytes());
    b.extend_from_slice(&preamble);
    b.extend_from_slice(&comp);
    b
}

/// A cache-cell grid entry: the marker, the `2L / L/2 / L` size triple, a name
/// length, and the nibble-swapped name.
pub(crate) fn make_cache_cell(logical_len: u32, name: &str) -> Vec<u8> {
    let swapped = swap_name(name);
    let mut b = Vec::new();
    b.extend_from_slice(&MARKER);
    b.extend_from_slice(&0u32.to_le_bytes()); // +6 type_id
    b.extend_from_slice(&(logical_len * 2).to_le_bytes()); // +10 2L
    b.extend_from_slice(&(logical_len / 2).to_le_bytes()); // +14 L/2
    b.extend_from_slice(&logical_len.to_le_bytes()); // +18 L
    b.extend_from_slice(&(swapped.len() as u32).to_le_bytes()); // +22 name_len
    b.extend_from_slice(&swapped);
    b
}

/// A tail section-directory entry naming an OPC part.
pub(crate) fn make_directory_entry(type_id: u32, size: u32, name: &str) -> Vec<u8> {
    let swapped = swap_name(name);
    let mut b = Vec::new();
    b.extend_from_slice(&MARKER);
    b.extend_from_slice(&type_id.to_le_bytes()); // +6
    b.extend_from_slice(&0u32.to_le_bytes()); // +10 zero
    b.extend_from_slice(&size.to_le_bytes()); // +14 size
    b.extend_from_slice(&0u32.to_le_bytes()); // +18 zero
    b.extend_from_slice(&(swapped.len() as u32).to_le_bytes()); // +22 name_len
    b.extend_from_slice(&[0u8; 14]); // +26 descriptor
    b.extend_from_slice(&swapped); // +40 name
    b.extend_from_slice(&[0xe5, 0x4b, 0x57, 0x5b, 0x00, 0x00]); // trailer
    b
}

/// A `.sldprt` whose partition block carries `triangle_body`.
pub(crate) fn sldprt_with_body(body: &[u8]) -> Vec<u8> {
    let mut f = outer_header();
    f.extend_from_slice(&make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", body),
    ));
    f
}

pub(crate) fn add_solidworks_version(source: &mut Vec<u8>, version: u32) {
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        format!(r#"<?xml version="1.0"?><swSolidWorks swVersion="{version}"/>"#).as_bytes(),
    ));
}

pub(crate) fn sldprt_with_body_and_envelope(body: &[u8]) -> Vec<u8> {
    let mut f = sldprt_with_body(body);
    let mut payload = b"moBBoxCenterData_c".to_vec();
    payload.extend_from_slice(&1u32.to_le_bytes());
    for value in [0.01f64, 0.02, -0.03, 0.04] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(b"moDefaultRefPlnData_c");
    for value in [0.001f64, 0.002, 0.003, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(b"moTransRefPlaneData_c");
    payload.extend_from_slice(&[0xff; 8]);
    for value in [0.01f64, 0.02, 0.03, 0.1, 0.2, 1.0, 0.0, -1.0, 0.5] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(b"moPart_c");
    let mut part = [0u8; 13];
    part[0..4].copy_from_slice(&42u32.to_le_bytes());
    part[8..12].copy_from_slice(&2026u32.to_le_bytes());
    payload.extend_from_slice(&part);
    payload.extend_from_slice(b"moConfigurationMgr_c");
    let mut configuration = [0u8; 125];
    configuration[66..70].copy_from_slice(&17u32.to_le_bytes());
    configuration[107] = 3;
    configuration[117..125].copy_from_slice(&132_537_600_000_000_000u64.to_le_bytes());
    payload.extend_from_slice(&configuration);
    payload.extend_from_slice(b"moLengthUserUnits_c");
    payload.extend_from_slice(&[0xff, 0xfe, 0xff, 4, b'I', 0, b'N', 0]);
    f.extend(make_block(0x43, "SWObjects", &payload));
    f.extend(make_block(
        0x44,
        "Units",
        br#"<Metadata><Property Name="SW_UnitsLinear" Value="0"/></Metadata>"#,
    ));
    f
}

pub(crate) fn sldprt_with_partition_and_deltas(partition: &[u8], deltas: &[u8]) -> Vec<u8> {
    let mut f = outer_header();
    let mut payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", partition);
    payload.extend(parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        deltas,
    ));
    f.extend_from_slice(&make_block(0x20, "Contents/Config-0-Partition", &payload));
    f
}

pub(crate) fn sldprt_with_colliding_sites() -> Vec<u8> {
    let mut f = outer_header();
    f.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 700, 0.0),
        ),
    ));
    f.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 701, 10.0),
        ),
    ));
    f
}

/// The 8-byte outer header (`file_id`, then big-endian `version == 4`).
pub(crate) fn outer_header() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&0x0000_0001u32.to_le_bytes());
    b.extend_from_slice(&0x0000_0004u32.to_be_bytes());
    b
}

/// A synthetic `.sldprt`: header, a PNG-preview block, a Parasolid block, a
/// cache cell, and a tail-directory entry.
pub(crate) fn synthetic_sldprt() -> Vec<u8> {
    let mut f = outer_header();
    f.extend_from_slice(&make_block(
        0x10,
        "PreviewPNG",
        &[0x89, b'P', b'N', b'G', 1, 2, 3, 4],
    ));
    f.extend_from_slice(&make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_payload("partition body", "SCH_SW_33103_11000"),
    ));
    f.extend_from_slice(&make_cache_cell(90, "Contents/DisplayLists"));
    f.extend_from_slice(&make_directory_entry(0x30, 2, "[Content_Types].xml"));
    f
}
