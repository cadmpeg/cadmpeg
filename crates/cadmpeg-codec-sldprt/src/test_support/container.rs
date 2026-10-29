// SPDX-License-Identifier: Apache-2.0
//! Synthetic outer-container byte builders for crate tests.
#![allow(clippy::unwrap_used)]

use std::io::Write;

use crate::container::MARKER;

use super::parasolid::{owned_triangle, parasolid_payload, parasolid_with_body};

/// Nibble-swap a section name into its stored form (the swap is its own inverse,
/// so the decoder recovers the original).
fn swap_name(name: &str) -> Vec<u8> {
    name.bytes().map(|b| b.rotate_left(4)).collect()
}

fn raw_deflate(data: &[u8]) -> Vec<u8> {
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

pub(super) fn zlib(data: &[u8]) -> Vec<u8> {
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
    b.extend_from_slice(&(u32::try_from(comp.len()).expect("length fits u32")).to_le_bytes());
    b.extend_from_slice(&(u32::try_from(payload.len()).expect("length fits u32")).to_le_bytes());
    b.extend_from_slice(&(u32::try_from(preamble.len()).expect("length fits u32")).to_le_bytes());
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
    b.extend_from_slice(&(u32::try_from(swapped.len()).expect("length fits u32")).to_le_bytes()); // +22 name_len
    b.extend_from_slice(&swapped);
    b
}

pub(crate) fn anchored_cache_cell(
    stride: usize,
    displacement: u32,
    name: &str,
    fill: u8,
) -> Vec<u8> {
    assert!((5..=14).contains(&displacement));
    let logical = u32::try_from(stride)
        .unwrap()
        .checked_mul(displacement)
        .unwrap();
    let mut bytes = vec![fill; usize::try_from(displacement).unwrap()];
    bytes.extend(make_cache_cell(logical, name));
    assert!(bytes.len() <= stride);
    bytes.resize(stride, fill);
    bytes
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
    b.extend_from_slice(&(u32::try_from(swapped.len()).expect("length fits u32")).to_le_bytes()); // +22 name_len
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
    payload.extend_from_slice(&[0xff, 0xff, 1, 0, 21, 0]);
    payload.extend_from_slice(b"moTransRefPlaneData_c");
    payload.extend_from_slice(&[
        1, 0, 0, 0, 1, 0, 0, 0, 0xff, 0x80, 0, 0, 0x80, 0x40, 0, 0, 95, 0, 0, 0, 0, 0, 0, 0, 1,
    ]);
    for value in [0.01f64, 0.02, 0.03, 0.1, 0.2, 0.3, -0.1, -0.2, -0.3, 0.5] {
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
    native_envelope_fixture(&f)
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
    f.extend_from_slice(&anchored_cache_cell(100, 9, "Contents/DisplayLists", 0));
    f.extend_from_slice(&make_directory_entry(0x30, 2, "[Content_Types].xml"));
    f
}

/// Scan a synthetic input with its actual service-policy source context.
pub(crate) fn scan(bytes: &[u8]) -> crate::container::ContainerScan<'_> {
    let arena = Box::leak(Box::new(cadmpeg_core::decode::DecodeArena::new()));
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        bytes,
        arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("synthetic source fits service policy");
    crate::container::scan(&ctx, root).expect("synthetic container fits service policy")
}

pub(crate) fn native_envelope_fixture(source: &[u8]) -> Vec<u8> {
    use cadmpeg_core::decode::View;
    let indexed = scan(source);
    let mut output = source[..8].to_vec();
    let mut central = Vec::new();
    for block in &indexed.blocks {
        let pre = usize::try_from(View::u32_le_at(source, block.offset + 22).unwrap()).unwrap();
        let end = block.offset + 26 + pre + usize::try_from(block.comp_sz).unwrap();
        let relative = u32::try_from(output.len() - 8).unwrap();
        output.extend([0x5a, 0xd9, 0xc8, 0x87]);
        output.extend(&source[block.offset..end]);
        central.extend([0x45, 0x2b, 0x7e, 0x28, 0, 0]);
        central.extend(&source[block.offset..block.offset + 26]);
        central.extend([0u8; 4]);
        central.extend(1u16.to_le_bytes());
        central.extend([0u8; 4]);
        central.extend(relative.to_le_bytes());
        central.extend(&source[block.offset + 26..block.offset + 26 + pre]);
    }
    for cell in &indexed.cache_cells {
        output.extend(source.get(cell.physical_range.clone().unwrap()).unwrap());
    }
    let central_start = output.len();
    let central_size = u32::try_from(central.len()).unwrap();
    output.extend(central);
    output.extend([0x9f, 0x82, 0x5e, 0x02]);
    output.extend([0u8; 4]);
    let count = u16::try_from(indexed.blocks.len()).unwrap();
    output.extend(count.to_le_bytes());
    output.extend(count.to_le_bytes());
    output.extend(central_size.to_le_bytes());
    output.extend(u32::try_from(central_start - 8).unwrap().to_le_bytes());
    output.extend(0u16.to_le_bytes());
    output
}

pub(crate) fn native_padding_fixture(root_only: &[u8], fill: u8) -> Vec<u8> {
    use cadmpeg_core::decode::View;
    let root = root_only.len() - 22;
    let count = View::u16_le_at(root_only, root + 8).unwrap();
    let mut output = root_only[..root].to_vec();
    output.extend([fill; 2]);
    let base = output.len();
    output.extend([fill; 9]);
    output.extend(MARKER);
    output.extend(0u32.to_le_bytes());
    for value in [1890u32, 472, 945] {
        output.extend(value.to_le_bytes());
    }
    output.extend(1u16.to_le_bytes());
    output.extend(0u16.to_le_bytes());
    output.extend([0u8; 14]);
    output.push(b'X'.rotate_left(4));
    output.resize(base + 105, fill);
    output.truncate(output.len() - 2);
    let new_root = output.len();
    output.extend(&root_only[root..]);
    let size = View::u32_le_at(&output, new_root + 12).unwrap();
    output[new_root + 12..new_root + 16].copy_from_slice(&(size + 105).to_le_bytes());
    let footer = output.len();
    output.extend([fill; 5]);
    output.extend([0u8; 4]);
    output.extend(count.to_le_bytes());
    output.extend(count.to_le_bytes());
    output.extend(105u32.to_le_bytes());
    output.extend(1050u32.to_le_bytes());
    output.extend(0u16.to_le_bytes());
    output.resize(footer + 42, fill);
    output
}
